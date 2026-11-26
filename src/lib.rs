#![forbid(unsafe_code)]

use std::io::{self, Write};

use gfx::blendmode::{BM_NORMAL, BM_OPAC_SHIFT, bm_row_inplace};

mod pdn_io;
mod png_io;

pub use png_io::{PngCompression, PngDepth};

pub const BLEND_MODES: [&str; 14] = [
    "Normal",
    "Multiply",
    "Additive",
    "Color Burn",
    "Color Dodge",
    "Reflect",
    "Glow",
    "Overlay",
    "Difference",
    "Negation",
    "Lighten",
    "Darken",
    "Screen",
    "Xor",
];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Pixel(pub [u8; 4]);

impl Pixel {
    pub fn over(self, background: Self, opacity: u8) -> Self {
        let front_alpha = u32::from(self.0[3]) * u32::from(opacity);
        let front_weight = front_alpha * 255;
        let back_weight = u32::from(background.0[3]) * (255 * 255 - front_alpha);
        let weight = front_weight + back_weight;
        let alpha = ((weight + 32_512) / 65_025) as u8;

        if alpha == 0 {
            return Self::default();
        }

        let mut channels = [0, 0, 0, alpha];
        for (index, channel) in channels[..3].iter_mut().enumerate() {
            let value = u32::from(self.0[index]) * front_weight
                + u32::from(background.0[index]) * back_weight;
            *channel = ((value + weight / 2) / weight) as u8;
        }
        Self(channels)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerData {
    pub name: String,
    pub visible: bool,
    pub opacity: u8,
    pub blend: u8,
    pub pixels: Vec<Pixel>,
    pub(crate) pdn_background: u8,
    pub(crate) pdn_metadata: Vec<u8>,
}

struct Layer {
    name: String,
    visible: bool,
    opacity: u8,
    blend: u8,
    pixels: Vec<Pixel>,
    pdn_background: u8,
    pdn_metadata: Vec<u8>,
}

impl Layer {
    fn to_data(&self) -> io::Result<LayerData> {
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(self.pixels.len())
            .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
        pixels.extend_from_slice(&self.pixels);
        Ok(LayerData {
            name: self.name.clone(),
            visible: self.visible,
            opacity: self.opacity,
            blend: self.blend,
            pixels,
            pdn_background: self.pdn_background,
            pdn_metadata: self.pdn_metadata.clone(),
        })
    }

    fn from_data(data: LayerData) -> Self {
        Self {
            name: data.name,
            visible: data.visible,
            opacity: data.opacity,
            blend: data.blend,
            pixels: data.pixels,
            pdn_background: data.pdn_background,
            pdn_metadata: data.pdn_metadata,
        }
    }
}

pub struct Document {
    width: u32,
    height: u32,
    layers: Vec<Layer>,
    pdn_metadata: pdn_io::PdnMetadata,
}

impl Document {
    pub fn new(width: u32, height: u32) -> io::Result<Self> {
        if width == 0 || height == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "tela vazia"));
        }
        let mut document = Self {
            width,
            height,
            layers: Vec::new(),
            pdn_metadata: Default::default(),
        };
        document.add_layer("Fundo")?;
        Ok(document)
    }

    pub fn add_layer(&mut self, name: impl Into<String>) -> io::Result<usize> {
        let pixels = self.allocate_pixels()?;
        self.layers.push(Layer {
            name: name.into(),
            visible: true,
            opacity: 255,
            blend: img_pdn::PDNB_NORMAL,
            pixels,
            pdn_background: 0,
            pdn_metadata: Vec::new(),
        });
        Ok(self.layers.len() - 1)
    }

    pub fn layer_name(&self, layer: usize) -> Option<&str> {
        self.layers.get(layer).map(|layer| layer.name.as_str())
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn resolution_dpi(&self) -> (f64, f64) {
        self.pdn_metadata.dpi()
    }

    pub fn layer_pixels(&self, layer: usize) -> Option<&[Pixel]> {
        self.layers.get(layer).map(|layer| layer.pixels.as_slice())
    }

    pub fn swap_layer_pixels(&mut self, layer: usize, pixels: &mut Vec<Pixel>) -> io::Result<()> {
        let layer = self.layer_mut(layer)?;
        if layer.pixels.len() != pixels.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "tamanho de camada inválido",
            ));
        }
        std::mem::swap(&mut layer.pixels, pixels);
        Ok(())
    }

    pub fn remap(
        &self,
        width: u32,
        height: u32,
        mut sample: impl FnMut(&[Pixel], u32, u32) -> Pixel,
    ) -> io::Result<Self> {
        if width == 0 || height == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "tela vazia"));
        }
        if u64::from(width) * u64::from(height) > pdn_io::MAX_PIXELS / self.layers.len() as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "limite total de pixels das camadas excedido",
            ));
        }
        let mut mapped = Self {
            width,
            height,
            layers: Vec::new(),
            pdn_metadata: self.pdn_metadata.clone(),
        };
        for layer in &self.layers {
            let mut pixels = mapped.allocate_pixels()?;
            for (index, pixel) in pixels.iter_mut().enumerate() {
                *pixel = sample(
                    &layer.pixels,
                    (index % width as usize) as u32,
                    (index / width as usize) as u32,
                );
            }
            mapped.layers.push(Layer {
                name: layer.name.clone(),
                visible: layer.visible,
                opacity: layer.opacity,
                blend: layer.blend,
                pixels,
                pdn_background: layer.pdn_background,
                pdn_metadata: layer.pdn_metadata.clone(),
            });
        }
        Ok(mapped)
    }

    pub fn flattened(&self) -> io::Result<Self> {
        let pixels = self.composite()?;
        let mut document = Self {
            width: self.width,
            height: self.height,
            layers: Vec::new(),
            pdn_metadata: self.pdn_metadata.clone(),
        };
        document.layers.push(Layer {
            name: "Imagem composta".to_owned(),
            visible: true,
            opacity: 255,
            blend: img_pdn::PDNB_NORMAL,
            pixels,
            pdn_background: 0,
            pdn_metadata: Vec::new(),
        });
        Ok(document)
    }

    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    pub fn layer_properties(&self, layer: usize) -> Option<(bool, u8)> {
        self.layers
            .get(layer)
            .map(|layer| (layer.visible, layer.opacity))
    }

    pub fn layer_blend_id(&self, layer: usize) -> Option<u8> {
        self.layers.get(layer).map(|layer| layer.blend)
    }

    pub fn layer_blend_mode(&self, layer: usize) -> Option<&'static str> {
        BLEND_MODES
            .get(usize::from(self.layers.get(layer)?.blend))
            .copied()
    }

    pub fn set_layer_name(&mut self, layer: usize, name: impl Into<String>) -> io::Result<()> {
        let name = name.into();
        if name.trim().is_empty() || name.contains('\0') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "nome de camada inválido",
            ));
        }
        self.layer_mut(layer)?.name = name;
        Ok(())
    }

    pub fn set_layer_blend_mode(&mut self, layer: usize, blend: u8) -> io::Result<()> {
        if blend > img_pdn::PDNB_XOR {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "modo de mesclagem inválido",
            ));
        }
        self.layer_mut(layer)?.blend = blend;
        Ok(())
    }

    pub fn clone_layer_data(&self, layer: usize) -> io::Result<LayerData> {
        self.layers
            .get(layer)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "camada inexistente"))?
            .to_data()
    }

    pub fn replace_layer_data(&mut self, index: usize, data: LayerData) -> io::Result<LayerData> {
        self.validate_layer_data(&data)?;
        let slot = self.layer_mut(index)?;
        let previous = slot.to_data()?;
        *slot = Layer::from_data(data);
        Ok(previous)
    }

    pub fn insert_layer_data(&mut self, index: usize, data: LayerData) -> io::Result<()> {
        if index > self.layers.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "posição de camada inválida",
            ));
        }
        self.validate_layer_data(&data)?;
        let pixel_count = u64::from(self.width) * u64::from(self.height);
        let next_layers = (self.layers.len() + 1) as u64;
        if pixel_count > pdn_io::MAX_PIXELS / next_layers {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "limite total de pixels das camadas excedido",
            ));
        }
        self.layers.insert(index, Layer::from_data(data));
        Ok(())
    }

    pub fn remove_layer(&mut self, index: usize) -> io::Result<LayerData> {
        if self.layers.len() <= 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "o documento deve manter ao menos uma camada",
            ));
        }
        if index >= self.layers.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "camada inexistente",
            ));
        }
        let removed = self.layers.remove(index);
        Ok(LayerData {
            name: removed.name,
            visible: removed.visible,
            opacity: removed.opacity,
            blend: removed.blend,
            pixels: removed.pixels,
            pdn_background: removed.pdn_background,
            pdn_metadata: removed.pdn_metadata,
        })
    }

    pub fn duplicate_layer(&mut self, index: usize) -> io::Result<usize> {
        let mut copy = self.clone_layer_data(index)?;
        copy.name = format!("{} (cópia)", copy.name);
        copy.pdn_background = 0;
        let target = index + 1;
        self.insert_layer_data(target, copy)?;
        Ok(target)
    }

    pub fn move_layer(&mut self, from: usize, to: usize) -> io::Result<()> {
        if from >= self.layers.len() || to >= self.layers.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "índice de camada inválido",
            ));
        }
        if from == to {
            return Ok(());
        }
        let layer = self.layers.remove(from);
        self.layers.insert(to, layer);
        Ok(())
    }

    pub fn merge_layer_down(&mut self, index: usize) -> io::Result<(LayerData, LayerData)> {
        if index == 0 || index >= self.layers.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "não há camada abaixo para mesclar",
            ));
        }
        let bottom_before = self.clone_layer_data(index - 1)?;
        let top = self.remove_layer(index)?;
        if top.visible && top.opacity > 0 {
            let count = self.layers[index - 1].pixels.len();
            let mut dst = vec![0u8; count * 4];
            let mut src = vec![0u8; count * 4];
            if bottom_before.visible {
                if bottom_before.opacity == 255 {
                    pixels_to_bytes(&bottom_before.pixels, &mut dst);
                    pixels_to_bytes(&top.pixels, &mut src);
                    let mode_op = blend_mode_op(top.blend, top.opacity);
                    bm_row_inplace(&mut dst, &src, mode_op);
                } else {
                    pixels_to_bytes(&bottom_before.pixels, &mut src);
                    let base_op = blend_mode_op(img_pdn::PDNB_NORMAL, bottom_before.opacity);
                    bm_row_inplace(&mut dst, &src, base_op);
                    pixels_to_bytes(&top.pixels, &mut src);
                    let top_op = blend_mode_op(top.blend, top.opacity);
                    bm_row_inplace(&mut dst, &src, top_op);
                    self.layers[index - 1].opacity = 255;
                }
            } else {
                pixels_to_bytes(&top.pixels, &mut src);
                let top_op = blend_mode_op(img_pdn::PDNB_NORMAL, top.opacity);
                bm_row_inplace(&mut dst, &src, top_op);
                let bottom = &mut self.layers[index - 1];
                bottom.visible = true;
                bottom.opacity = 255;
                bottom.blend = top.blend;
            }
            bytes_to_pixels(&dst, &mut self.layers[index - 1].pixels);
        }
        Ok((top, bottom_before))
    }

    pub fn layer_thumbnail(&self, layer: usize, max_side: u32) -> Option<(u32, u32, Vec<Pixel>)> {
        let pixels = self.layer_pixels(layer)?;
        Some(sample_thumbnail(pixels, self.width, self.height, max_side))
    }

    pub fn composite_thumbnail(&self, max_side: u32) -> io::Result<(u32, u32, Vec<Pixel>)> {
        let pixels = self.composite()?;
        Ok(sample_thumbnail(&pixels, self.width, self.height, max_side))
    }

    fn validate_layer_data(&self, data: &LayerData) -> io::Result<()> {
        let expected = (u64::from(self.width) * u64::from(self.height)) as usize;
        if data.pixels.len() != expected
            || data.name.trim().is_empty()
            || data.name.contains('\0')
            || data.blend > img_pdn::PDNB_XOR
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "dados de camada incompatíveis",
            ));
        }
        Ok(())
    }

    pub fn remove_top_layer(&mut self) {
        self.take_top_layer_pixels();
    }

    pub fn take_top_layer_pixels(&mut self) -> Option<Vec<Pixel>> {
        if self.layers.len() > 1 {
            self.layers.pop().map(|layer| layer.pixels)
        } else {
            None
        }
    }

    pub fn pixel(&self, layer: usize, x: u32, y: u32) -> Option<Pixel> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let index = (u64::from(y) * u64::from(self.width) + u64::from(x)) as usize;
        Some(self.layers.get(layer)?.pixels[index])
    }

    pub fn set_layer_properties(
        &mut self,
        layer: usize,
        visible: bool,
        opacity: u8,
    ) -> io::Result<()> {
        let layer = self.layer_mut(layer)?;
        layer.visible = visible;
        layer.opacity = opacity;
        Ok(())
    }

    pub fn fill_layer(&mut self, layer: usize, pixel: Pixel) -> io::Result<()> {
        self.layer_mut(layer)?.pixels.fill(pixel);
        Ok(())
    }

    pub fn set_pixel(&mut self, layer: usize, x: u32, y: u32, pixel: Pixel) -> io::Result<()> {
        if x >= self.width || y >= self.height {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "pixel fora da tela",
            ));
        }
        let index = (u64::from(y) * u64::from(self.width) + u64::from(x)) as usize;
        self.layer_mut(layer)?.pixels[index] = pixel;
        Ok(())
    }

    pub fn composite(&self) -> io::Result<Vec<Pixel>> {
        self.composite_rect((0, 0), (self.width - 1, self.height - 1))
    }

    pub fn composite_rect(&self, min: (u32, u32), max: (u32, u32)) -> io::Result<Vec<Pixel>> {
        if min.0 > max.0 || min.1 > max.1 || max.0 >= self.width || max.1 >= self.height {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "região de composição inválida",
            ));
        }
        let rect_w = (max.0 - min.0 + 1) as usize;
        let rect_h = (max.1 - min.1 + 1) as usize;
        let count = rect_w
            .checked_mul(rect_h)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "região muito grande"))?;
        let byte_len = count
            .checked_mul(4)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "região muito grande"))?;
        let mut dst = Vec::new();
        dst.try_reserve_exact(byte_len)
            .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
        dst.resize(byte_len, 0);
        let mut src_row = Vec::new();
        src_row
            .try_reserve_exact(rect_w * 4)
            .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
        src_row.resize(rect_w * 4, 0);

        let full_w = self.width as usize;
        let start_x = min.0 as usize;
        let start_y = min.1 as usize;

        for layer in self
            .layers
            .iter()
            .filter(|layer| layer.visible && layer.opacity != 0)
        {
            let mode_op = blend_mode_op(layer.blend, layer.opacity);
            for row_idx in 0..rect_h {
                let src_start = (start_y + row_idx) * full_w + start_x;
                pixels_to_bytes(&layer.pixels[src_start..src_start + rect_w], &mut src_row);
                let dst_start = row_idx * rect_w * 4;
                bm_row_inplace(&mut dst[dst_start..dst_start + rect_w * 4], &src_row, mode_op);
            }
        }

        let mut result = Vec::new();
        result
            .try_reserve_exact(count)
            .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
        for &[r, g, b, a] in dst.as_chunks::<4>().0 {
            result.push(Pixel([r, g, b, a]));
        }
        Ok(result)
    }

    pub fn write_pam(&self, mut output: impl Write) -> io::Result<()> {
        let pixels = self.composite()?;
        writeln!(
            output,
            "P7\nWIDTH {}\nHEIGHT {}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR",
            self.width, self.height,
        )?;
        for Pixel(rgba) in pixels {
            output.write_all(&rgba)?;
        }
        Ok(())
    }

    fn layer_mut(&mut self, layer: usize) -> io::Result<&mut Layer> {
        self.layers
            .get_mut(layer)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "camada inexistente"))
    }

    fn allocate_pixels(&self) -> io::Result<Vec<Pixel>> {
        let count = usize::try_from(u64::from(self.width) * u64::from(self.height))
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(count)
            .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
        pixels.resize(count, Pixel::default());
        Ok(pixels)
    }
}

fn blend_mode_op(blend: u8, opacity: u8) -> u32 {
    let mode = if blend <= img_pdn::PDNB_XOR {
        u32::from(blend)
    } else {
        BM_NORMAL
    };
    mode | (u32::from(opacity) << BM_OPAC_SHIFT)
}

fn pixels_to_bytes(pixels: &[Pixel], bytes: &mut [u8]) {
    for (dst, &Pixel(rgba)) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(pixels) {
        *dst = rgba;
    }
}

fn bytes_to_pixels(bytes: &[u8], pixels: &mut [Pixel]) {
    for (dst, &rgba) in pixels.iter_mut().zip(bytes.as_chunks::<4>().0) {
        *dst = Pixel(rgba);
    }
}

fn sample_thumbnail(pixels: &[Pixel], width: u32, height: u32, max_side: u32) -> (u32, u32, Vec<Pixel>) {
    let max_side = max_side.max(1);
    let (thumb_w, thumb_h) = if width >= height {
        let thumb_w = width.min(max_side).max(1);
        let thumb_h = ((u64::from(height) * u64::from(thumb_w)).div_ceil(u64::from(width)) as u32)
            .clamp(1, max_side);
        (thumb_w, thumb_h)
    } else {
        let thumb_h = height.min(max_side).max(1);
        let thumb_w = ((u64::from(width) * u64::from(thumb_h)).div_ceil(u64::from(height)) as u32)
            .clamp(1, max_side);
        (thumb_w, thumb_h)
    };
    let mut out = Vec::with_capacity((thumb_w * thumb_h) as usize);
    for ty in 0..thumb_h {
        let sy = ((u64::from(ty) * 2 + 1) * u64::from(height) / (2 * u64::from(thumb_h)))
            .min(u64::from(height - 1)) as usize;
        for tx in 0..thumb_w {
            let sx = ((u64::from(tx) * 2 + 1) * u64::from(width) / (2 * u64::from(thumb_w)))
                .min(u64::from(width - 1)) as usize;
            out.push(pixels[sy * width as usize + sx]);
        }
    }
    (thumb_w, thumb_h, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composites_layers_in_order_and_honors_visibility() -> io::Result<()> {
        let mut doc = Document::new(1, 1)?;
        let red = Pixel([255, 0, 0, 255]);
        doc.fill_layer(0, red)?;
        let overlay = doc.add_layer("Azul")?;
        assert_eq!(doc.layer_name(overlay), Some("Azul"));
        doc.fill_layer(overlay, Pixel([0, 0, 255, 255]))?;
        doc.set_layer_properties(overlay, true, 128)?;
        assert_eq!(doc.composite()?, [Pixel([127, 0, 128, 255])]);
        doc.set_layer_properties(overlay, false, 255)?;
        assert_eq!(doc.composite()?, [red]);
        Ok(())
    }

    #[test]
    fn combines_pixel_alpha_and_layer_opacity() -> io::Result<()> {
        let mut doc = Document::new(1, 1)?;
        doc.fill_layer(0, Pixel([0, 255, 0, 128]))?;
        doc.set_layer_properties(0, true, 128)?;
        assert_eq!(doc.composite()?, [Pixel([0, 255, 0, 64])]);
        Ok(())
    }

    #[test]
    fn exports_rgba_in_row_order() -> io::Result<()> {
        let mut doc = Document::new(2, 2)?;
        doc.set_pixel(0, 0, 1, Pixel([4, 5, 6, 255]))?;
        let mut output = Vec::new();
        doc.write_pam(&mut output)?;
        assert_eq!(
            output,
            b"P7\nWIDTH 2\nHEIGHT 2\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n\0\0\0\0\0\0\0\0\x04\x05\x06\xff\0\0\0\0"
        );
        Ok(())
    }

    #[test]
    fn rejects_empty_canvas_and_invalid_coordinates() -> io::Result<()> {
        assert!(Document::new(0, 1).is_err());
        let mut doc = Document::new(2, 2)?;
        assert!(doc.set_pixel(0, 2, 0, Pixel::default()).is_err());
        assert!(doc.set_pixel(0, 0, 2, Pixel::default()).is_err());
        assert!(doc.fill_layer(1, Pixel::default()).is_err());
        Ok(())
    }

    #[test]
    fn layer_operations_and_partial_composite_preserve_pixels() -> io::Result<()> {
        let mut doc = Document::new(3, 3)?;
        doc.fill_layer(0, Pixel([200, 100, 50, 255]))?;
        let top = doc.add_layer("Topo")?;
        doc.set_pixel(top, 1, 1, Pixel([10, 20, 30, 128]))?;
        doc.set_layer_blend_mode(top, img_pdn::PDNB_MULTIPLY)?;
        assert_eq!(doc.layer_blend_id(top), Some(img_pdn::PDNB_MULTIPLY));

        let dup_idx = doc.duplicate_layer(top)?;
        assert_eq!(doc.layer_name(dup_idx), Some("Topo (cópia)"));
        doc.set_layer_name(dup_idx, "Duplicada")?;
        assert_eq!(doc.layer_name(dup_idx), Some("Duplicada"));
        assert!(doc.set_layer_name(dup_idx, "   ").is_err());

        doc.move_layer(dup_idx, 0)?;
        assert_eq!(doc.layer_name(0), Some("Duplicada"));
        assert_eq!(doc.remove_layer(0)?.name, "Duplicada");

        let full = doc.composite()?;
        let partial = doc.composite_rect((1, 1), (2, 2))?;
        assert_eq!(partial[0], full[4]);
        assert_eq!(partial[3], full[8]);

        let (removed_top, bottom_before) = doc.merge_layer_down(1)?;
        assert_eq!(doc.layer_count(), 1);
        assert_eq!(doc.composite()?, full);
        doc.replace_layer_data(0, bottom_before)?;
        doc.insert_layer_data(1, removed_top)?;
        assert_eq!(doc.layer_count(), 2);
        assert_eq!(doc.composite()?, full);
        Ok(())
    }
}
