use super::i18n::text as tr;
use std::{borrow::Cow, io};

use openpaintr::Pixel;

use super::selection::{Point, Selection};

#[cfg(target_os = "linux")]
mod files;
#[cfg(target_os = "linux")]
mod wayland;

#[cfg(target_os = "linux")]
const MAX_IMAGE_BYTES: u64 = 512 * 1024 * 1024;

pub struct Image {
    pub size: Point,
    pub origin: Point,
    pub pixels: Vec<Pixel>,
}

impl Image {
    pub fn clamped_origin(&self, canvas: Point) -> Point {
        if self.origin.0 >= canvas.0 || self.origin.1 >= canvas.1 {
            (0, 0)
        } else {
            self.origin
        }
    }
}

pub struct Clipboard {
    system: Option<arboard::Clipboard>,
    #[cfg(target_os = "linux")]
    wayland: Option<wayland::Clipboard>,
    local: Option<Image>,
    system_exported: bool,
    pub mask: Option<(Selection, Point)>,
}

impl Clipboard {
    pub fn new() -> Self {
        Self {
            system: None,
            #[cfg(target_os = "linux")]
            wayland: None,
            local: None,
            system_exported: false,
            mask: None,
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn attach_window(&mut self, window: &eframe::CreationContext<'_>) {
        self.wayland = wayland::Clipboard::new(window);
    }

    fn system(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.system.is_none() {
            self.system = arboard::Clipboard::new().ok();
        }
        self.system.as_mut()
    }

    pub fn put(&mut self, image: Image) -> bool {
        #[cfg(target_os = "linux")]
        if let Some(wayland) = &mut self.wayland {
            wayland.supersede_selection();
        }
        let system = self.system();
        self.system_exported = system.is_some_and(|system| {
            let Ok(bytes) = Pixel::to_rgba_bytes(&image.pixels) else {
                return false;
            };
            system
                .set_image(arboard::ImageData {
                    width: image.size.0 as usize,
                    height: image.size.1 as usize,
                    bytes: Cow::Owned(bytes),
                })
                .is_ok()
        });
        self.local = Some(image);
        self.system_exported
    }

    pub fn get(&mut self) -> io::Result<Image> {
        if let Some((size, pixels)) = self.system_image()? {
            let origin = self
                .local
                .as_ref()
                .filter(|local| local.size == size && local.pixels == pixels)
                .map_or((0, 0), |local| local.origin);
            return Ok(Image {
                size,
                origin,
                pixels,
            });
        }
        let local = self
            .local
            .as_ref()
            .ok_or_else(|| io::Error::other(tr("clipboard-image-missing")))?;
        Ok(Image {
            size: local.size,
            origin: local.origin,
            pixels: local.pixels.clone(),
        })
    }

    fn system_image(&mut self) -> io::Result<Option<(Point, Vec<Pixel>)>> {
        #[cfg(target_os = "linux")]
        if let Some(wayland) = &mut self.wayland
            && let Some(image) = wayland.get_image()?
        {
            return Ok(Some(image));
        }
        if self.system_exported || self.local.is_none() {
            let Some(system) = self.system() else {
                return Ok(None);
            };
            let image = system.get_image();
            #[cfg(target_os = "linux")]
            if matches!(image, Err(arboard::Error::ContentNotAvailable))
                && let Ok(paths) = system.get().file_list()
                && let [path] = paths.as_slice()
                && let Some(image) = files::read_image(path)?
            {
                return Ok(Some(image));
            }
            let image = image.map_err(io::Error::other)?;
            let w = u32::try_from(image.width).map_err(io::Error::other)?;
            let h = u32::try_from(image.height).map_err(io::Error::other)?;
            if w == 0
                || h == 0
                || u64::from(w)
                    .checked_mul(u64::from(h))
                    .and_then(|size| size.checked_mul(4))
                    != Some(image.bytes.len() as u64)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    tr("clipboard-image-invalid"),
                ));
            }
            return Ok(Some(((w, h), Pixel::from_rgba_bytes(&image.bytes)?)));
        }
        Ok(None)
    }
}
