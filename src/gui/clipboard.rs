use std::{borrow::Cow, io};

use openpaintr::Pixel;

use super::selection::{Point, Selection};

pub struct Image {
    pub size: Point,
    pub origin: Point,
    pub pixels: Vec<Pixel>,
}

pub struct Clipboard {
    system: Option<arboard::Clipboard>,
    local: Option<Image>,
    system_exported: bool,
    pub mask: Option<(Selection, Point)>,
}

impl Clipboard {
    pub fn new() -> Self {
        Self {
            system: None,
            local: None,
            system_exported: false,
            mask: None,
        }
    }

    fn system(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.system.is_none() {
            self.system = arboard::Clipboard::new().ok();
        }
        self.system.as_mut()
    }

    pub fn put(&mut self, image: Image) -> bool {
        let system = self.system();
        self.system_exported = system.is_some_and(|system| {
            let bytes = image.pixels.iter().flat_map(|pixel| pixel.0).collect();
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
        if self.system_exported || self.local.is_none() {
            let system = self.system();
            if let Some(system) = system {
                let image = system.get_image().map_err(io::Error::other)?;
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
                        "imagem da área de transferência inválida",
                    ));
                }
                let pixels: Vec<_> = image
                    .bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .copied()
                    .map(Pixel)
                    .collect();
                let origin = self
                    .local
                    .as_ref()
                    .filter(|local| local.size == (w, h) && local.pixels == pixels)
                    .map_or((0, 0), |local| local.origin);
                return Ok(Image {
                    size: (w, h),
                    origin,
                    pixels,
                });
            }
        }
        let local = self
            .local
            .as_ref()
            .ok_or_else(|| io::Error::other("Área de transferência sem imagem acessível"))?;
        Ok(Image {
            size: local.size,
            origin: local.origin,
            pixels: local.pixels.clone(),
        })
    }
}
