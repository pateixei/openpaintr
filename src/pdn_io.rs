use std::io::{self, Read, Write};

use gfx::FMT_ARGB;
use img_core::{ERR_LIMIT, ERR_NOMEM, ERR_UNSUPPORTED, ImgDec, Surface};
use img_pdn::{DocRef, LayerRef};

use crate::{
    Document, Layer, LayerData, Pixel, check_layer_pixel_limit, diagnostic::msg,
    try_vec_with_capacity,
};

// Satisfy JFLib's platform linker without using its application entry point.
rt::test_main!();

const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
pub(super) const MAX_PIXELS: u64 = 64 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct PdnMetadata {
    unit: u32,
    dpux: f64,
    dpuy: f64,
    nvc: Vec<u8>,
    icc: Vec<u8>,
}

impl PdnMetadata {
    pub(super) fn with_dpi(dpi_x: f64, dpi_y: f64) -> Self {
        Self {
            dpux: dpi_x,
            dpuy: dpi_y,
            ..Self::default()
        }
    }

    pub(super) fn dpi(&self) -> (f64, f64) {
        let factor = match self.unit {
            img_pdn::PDU_CM => 2.54,
            img_pdn::PDU_INCH => 1.0,
            _ => return (96.0, 96.0),
        };
        let resolution = |value: f64| {
            let value = value * factor;
            if value.is_finite() && value > 0.0 {
                value
            } else {
                96.0
            }
        };
        (resolution(self.dpux), resolution(self.dpuy))
    }
}

impl Default for PdnMetadata {
    fn default() -> Self {
        Self {
            unit: img_pdn::PDU_INCH,
            dpux: 96.0,
            dpuy: 96.0,
            nvc: Vec::new(),
            icc: Vec::new(),
        }
    }
}

impl Document {
    pub fn read_pdn(input: impl Read) -> io::Result<Self> {
        let mut bytes = Vec::new();
        input.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(crate::Diagnostic::io(
                io::ErrorKind::InvalidData,
                msg::PDN_FILE_SIZE,
            ));
        }
        let options = ImgDec {
            maxpx: MAX_PIXELS,
            maxmem: MAX_PIXELS * 4,
            ..ImgDec::default()
        };
        let source = img_pdn::decode(&bytes, Some(&options)).map_err(|code| {
            let (kind, message) = match code {
                ERR_UNSUPPORTED => (io::ErrorKind::Unsupported, msg::PDN_VARIANT),
                ERR_LIMIT => (io::ErrorKind::InvalidData, msg::PDN_DECODE_LIMIT),
                ERR_NOMEM => (io::ErrorKind::OutOfMemory, msg::PDN_OUT_OF_MEMORY),
                _ => (io::ErrorKind::InvalidData, msg::PDN_INVALID),
            };
            crate::Diagnostic::io(kind, message)
        })?;
        drop(bytes);

        let mut document = Self {
            width: source.w,
            height: source.h,
            layers: Vec::new(),
            pdn_metadata: PdnMetadata {
                unit: source.unit,
                dpux: source.dpux,
                dpuy: source.dpuy,
                nvc: source.nvc,
                icc: source.meta.map_or_else(Vec::new, |metadata| metadata.icc),
            },
        };
        if document.width == 0 || document.height == 0 || source.layers.is_empty() {
            return Err(crate::Diagnostic::io(
                io::ErrorKind::InvalidData,
                msg::PDN_EMPTY,
            ));
        }
        let pixel_count = check_layer_pixel_limit(
            document.width,
            document.height,
            source.layers.len(),
            io::ErrorKind::InvalidData,
            msg::PDN_LAYER_LIMIT,
        )?;
        let row_bytes = document.width as usize * 4;
        for layer in source.layers {
            let surface = layer.sf;
            let stride = usize::try_from(surface.stride).unwrap_or_default();
            if surface.w as u32 != document.width
                || surface.h as u32 != document.height
                || stride < row_bytes
                || surface.px.len() / stride < document.height as usize
            {
                return Err(crate::Diagnostic::io(
                    io::ErrorKind::InvalidData,
                    msg::PDN_LAYER_SIZE,
                ));
            }
            let name = layer.name.strip_suffix(&[0]).unwrap_or(&layer.name);
            let mut pixels = try_vec_with_capacity(pixel_count as usize)?;
            for row in surface
                .px
                .chunks_exact(stride)
                .take(document.height as usize)
            {
                for &bgra in row[..row_bytes].as_chunks::<4>().0 {
                    pixels.push(Pixel::from_bgra(bgra));
                }
            }
            document.layers.push(Layer(LayerData {
                name: String::from_utf8_lossy(name).into_owned(),
                visible: layer.visible != 0,
                opacity: layer.opacity,
                blend: layer.blend,
                pixels,
                pdn_background: layer.isbg,
                pdn_metadata: layer.meta,
            }));
        }
        Ok(document)
    }

    pub fn write_pdn(&self, output: impl Write) -> io::Result<()> {
        self.write_pdn_with_effort(output, 1)
    }

    pub fn write_pdn_with_effort(&self, mut output: impl Write, effort: u32) -> io::Result<()> {
        check_layer_pixel_limit(
            self.width,
            self.height,
            self.layers.len(),
            io::ErrorKind::InvalidInput,
            msg::PDN_LAYER_LIMIT,
        )?;

        let mut surfaces = Vec::new();
        for layer in &self.layers {
            if layer.name.contains('\0') || layer.blend > img_pdn::PDNB_XOR {
                return Err(crate::Diagnostic::io(
                    io::ErrorKind::InvalidInput,
                    msg::PDN_LAYER_NAME_MODE,
                ));
            }
            let mut px = try_vec_with_capacity(layer.pixels.len() * 4)?;
            for &pixel in &layer.pixels {
                px.extend_from_slice(&pixel.to_bgra());
            }
            surfaces.push(Surface {
                w: self.width as _,
                h: self.height as _,
                stride: (self.width * 4) as _,
                fmt: FMT_ARGB,
                px,
                ..Surface::default()
            });
        }
        let metadata = &self.pdn_metadata;
        let source = DocRef {
            w: self.width,
            h: self.height,
            unit: metadata.unit,
            dpux: metadata.dpux,
            dpuy: metadata.dpuy,
            nvc: &metadata.nvc,
            icc: &metadata.icc,
            layers: self
                .layers
                .iter()
                .zip(&surfaces)
                .map(|(layer, sf)| LayerRef {
                    sf,
                    name: layer.name.as_bytes(),
                    visible: u8::from(layer.visible),
                    opacity: layer.opacity,
                    blend: layer.blend,
                    isbg: layer.pdn_background,
                    meta: &layer.pdn_metadata,
                })
                .collect(),
        };
        let options = img_core::ImgOpt {
            effort: effort.clamp(1, 9),
            ..Default::default()
        };
        let bytes =
            img_pdn::encode_ex(&source, Some(&options), img_pdn::PDNE_BLENDOP).map_err(|code| {
                match code {
                    ERR_NOMEM => {
                        crate::Diagnostic::io(io::ErrorKind::OutOfMemory, msg::PDN_SAVE_MEMORY)
                    }
                    _ => crate::Diagnostic::io(io::ErrorKind::InvalidInput, msg::PDN_ENCODE),
                }
            })?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(crate::Diagnostic::io(
                io::ErrorKind::InvalidInput,
                msg::PDN_FILE_SIZE,
            ));
        }
        output.write_all(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use gfx::FMT_ARGB;
    use img_core::Surface;
    use img_pdn::{PDNB_MULTIPLY, PdnDoc, PdnLayer};

    use super::*;

    #[test]
    fn reads_layer_order_visibility_opacity_and_bgra_pixels() -> io::Result<()> {
        let layer = |name: &str, color: [u8; 4], visible, opacity, blend| PdnLayer {
            name: name.as_bytes().to_vec(),
            visible,
            opacity,
            blend,
            sf: Surface {
                w: 1,
                h: 1,
                stride: 4,
                fmt: FMT_ARGB,
                px: color.to_vec(),
                ..Surface::default()
            },
            ..PdnLayer::default()
        };
        let fixture = PdnDoc {
            w: 1,
            h: 1,
            layers: vec![
                layer("Fundo", [64, 128, 255, 255], 1, 255, 0),
                layer("Sobreposição", [0, 255, 0, 255], 0, 128, PDNB_MULTIPLY),
            ],
            ..PdnDoc::default()
        };
        let encoded = img_pdn::encode(&fixture.view(), None).expect("encode test fixture");
        let mut document = Document::read_pdn(encoded.as_slice())?;
        assert_eq!(document.layer_count(), 2);
        assert_eq!(document.layer_name(0), Some("Fundo"));
        assert_eq!(document.layer_name(1), Some("Sobreposição"));
        assert_eq!(document.layer_properties(1), Some((false, 128)));
        assert_eq!(document.layer_blend_mode(1), Some("Multiply"));
        assert_eq!(document.pixel(1, 0, 0), Some(Pixel([0, 255, 0, 255])));
        assert_eq!(document.composite()?, [Pixel([255, 128, 64, 255])]);
        document.set_layer_properties(1, true, 128)?;
        assert_eq!(document.composite()?, [Pixel([127, 128, 31, 255])]);

        document.set_pixel(1, 0, 0, Pixel([23, 42, 66, 139]))?;
        let mut saved = Vec::new();
        document.write_pdn(&mut saved)?;
        assert!(saved.starts_with(b"PDN3"));
        let reopened = Document::read_pdn(saved.as_slice())?;
        assert_eq!(reopened.dimensions(), (1, 1));
        assert_eq!(reopened.layer_name(0), Some("Fundo"));
        assert_eq!(reopened.layer_name(1), Some("Sobreposição"));
        assert_eq!(reopened.layer_properties(1), Some((true, 128)));
        assert_eq!(reopened.layer_blend_mode(1), Some("Multiply"));
        assert_eq!(reopened.pixel(1, 0, 0), Some(Pixel([23, 42, 66, 139])));
        assert_eq!(reopened.composite()?, document.composite()?);
        Ok(())
    }

    #[test]
    fn rejects_truncated_pdn() {
        assert_eq!(
            Document::read_pdn(&b"PDN3\x7f\0\0"[..])
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}
