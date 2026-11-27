#![forbid(unsafe_code)]

use std::io::{self, Write};

use gfx::blendmode::{BM_NORMAL, BM_OPAC_SHIFT, bm_row_inplace};

mod diagnostic;
mod pdn_io;
mod png_io;

use diagnostic::{Message, msg};
pub use diagnostic::Diagnostic;
pub use png_io::{DecodedPng, PngCompression, PngDepth, decode_png_rgba};

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

pub const BLEND_MODE_KEYS: [&str; 14] = [
    "blend.normal",
    "blend.multiply",
    "blend.additive",
    "blend.color-burn",
    "blend.color-dodge",
    "blend.reflect",
    "blend.glow",
    "blend.overlay",
    "blend.difference",
    "blend.negation",
    "blend.lighten",
    "blend.darken",
    "blend.screen",
    "blend.xor",
];

#[inline]
pub fn mul_u8(a: u8, b: u8) -> u8 {
    ((u32::from(a) * u32::from(b) + 127) / 255) as u8
}

pub fn is_valid_layer_name(name: &str) -> bool {
    !name.trim().is_empty() && !name.contains('\0')
}

pub fn try_vec_with_capacity<T>(len: usize) -> io::Result<Vec<T>> {
    let mut vec = Vec::new();
    vec.try_reserve_exact(len)
        .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
    Ok(vec)
}

pub fn try_vec_filled<T: Clone>(len: usize, value: T) -> io::Result<Vec<T>> {
    let mut vec = try_vec_with_capacity(len)?;
    vec.resize(len, value);
    Ok(vec)
}

fn invalid_input(message: Message) -> io::Error {
    Diagnostic::io(io::ErrorKind::InvalidInput, message)
}

pub(crate) fn check_layer_pixel_limit(
    width: u32,
    height: u32,
    layers: usize,
    kind: io::ErrorKind,
    message: Message,
) -> io::Result<u64> {
    let pixel_count = u64::from(width) * u64::from(height);
    let divisor = (layers as u64).max(1);
    if pixel_count > pdn_io::MAX_PIXELS / divisor {
        return Err(Diagnostic::io(kind, message));
    }
    Ok(pixel_count)
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Pixel(pub [u8; 4]);

impl Pixel {
    #[inline]
    pub fn scale_alpha(self, factor: u8) -> Self {
        match factor {
            0 => Self::default(),
            255 => self,
            _ => {
                let alpha = mul_u8(self.0[3], factor);
                if alpha == 0 {
                    Self::default()
                } else {
                    Self([self.0[0], self.0[1], self.0[2], alpha])
                }
            }
        }
    }

    #[inline]
    pub fn premul_channels(self) -> [u8; 4] {
        let [r, g, b, a] = self.0;
        match a {
            0 => [0, 0, 0, 0],
            255 => [r, g, b, 255],
            _ => [mul_u8(r, a), mul_u8(g, a), mul_u8(b, a), a],
        }
    }

    #[inline]
    pub fn from_bgra([b, g, r, a]: [u8; 4]) -> Self {
        Self([r, g, b, a])
    }

    #[inline]
    pub fn to_bgra(self) -> [u8; 4] {
        let [r, g, b, a] = self.0;
        [b, g, r, a]
    }

    #[inline]
    pub fn to_argb_u32(self) -> u32 {
        let [r, g, b, a] = self.0;
        (u32::from(a) << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
    }

    pub fn to_rgba_bytes(pixels: &[Self]) -> io::Result<Vec<u8>> {
        let byte_len = pixels
            .len()
            .checked_mul(4)
            .ok_or_else(|| io::Error::new(io::ErrorKind::OutOfMemory, "pixel buffer too large"))?;
        let mut bytes = try_vec_with_capacity(byte_len)?;
        for &Self(rgba) in pixels {
            bytes.extend_from_slice(&rgba);
        }
        Ok(bytes)
    }

    pub fn from_rgba_bytes(bytes: &[u8]) -> io::Result<Vec<Self>> {
        let mut pixels = try_vec_with_capacity(bytes.len() / 4)?;
        for &rgba in bytes.as_chunks::<4>().0 {
            pixels.push(Self(rgba));
        }
        Ok(pixels)
    }

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

impl LayerData {
    fn with_pixels(&self, pixels: Vec<Pixel>) -> Self {
        Self {
            name: self.name.clone(),
            visible: self.visible,
            opacity: self.opacity,
            blend: self.blend,
            pixels,
            pdn_background: self.pdn_background,
            pdn_metadata: self.pdn_metadata.clone(),
        }
    }
}

struct Layer(LayerData);

impl std::ops::Deref for Layer {
    type Target = LayerData;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for Layer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Layer {
    fn to_data(&self) -> io::Result<LayerData> {
        let mut pixels = try_vec_with_capacity(self.0.pixels.len())?;
        pixels.extend_from_slice(&self.0.pixels);
        Ok(self.0.with_pixels(pixels))
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
            return Err(invalid_input(msg::EMPTY_CANVAS));
        }
        let mut document = Self {
            width,
            height,
            layers: Vec::new(),
            pdn_metadata: Default::default(),
        };
        document.add_layer("Background")?;
        Ok(document)
    }

    pub fn add_layer(&mut self, name: impl Into<String>) -> io::Result<usize> {
        let index = self.layers.len();
        self.insert_blank_layer(index, name)?;
        Ok(index)
    }

    pub fn insert_blank_layer(&mut self, index: usize, name: impl Into<String>) -> io::Result<()> {
        self.check_insert_position(index)?;
        let pixels = self.allocate_pixels()?;
        self.layers.insert(
            index,
            Layer(LayerData {
                name: name.into(),
                visible: true,
                opacity: 255,
                blend: img_pdn::PDNB_NORMAL,
                pixels,
                pdn_background: 0,
                pdn_metadata: Vec::new(),
            }),
        );
        Ok(())
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
            return Err(invalid_input(msg::INVALID_LAYER_SIZE));
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
            return Err(invalid_input(msg::EMPTY_CANVAS));
        }
        check_layer_pixel_limit(
            width,
            height,
            self.layers.len(),
            io::ErrorKind::InvalidInput,
            msg::TOTAL_LAYER_LIMIT,
        )?;
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
            mapped.layers.push(Layer(layer.with_pixels(pixels)));
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
        document.layers.push(Layer(LayerData {
            name: "Merged image".to_owned(),
            visible: true,
            opacity: 255,
            blend: img_pdn::PDNB_NORMAL,
            pixels,
            pdn_background: 0,
            pdn_metadata: Vec::new(),
        }));
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
        if !is_valid_layer_name(&name) {
            return Err(invalid_input(msg::INVALID_LAYER_NAME));
        }
        self.layer_mut(layer)?.name = name;
        Ok(())
    }

    pub fn set_layer_blend_mode(&mut self, layer: usize, blend: u8) -> io::Result<()> {
        if blend > img_pdn::PDNB_XOR {
            return Err(invalid_input(msg::INVALID_BLEND_MODE));
        }
        self.layer_mut(layer)?.blend = blend;
        Ok(())
    }

    pub fn clone_layer_data(&self, layer: usize) -> io::Result<LayerData> {
        self.layer(layer)?.to_data()
    }

    pub fn replace_layer_data(&mut self, index: usize, data: LayerData) -> io::Result<LayerData> {
        self.validate_layer_data(&data)?;
        let slot = self.layer_mut(index)?;
        let previous = slot.to_data()?;
        *slot = Layer(data);
        Ok(previous)
    }

    pub fn insert_layer_data(&mut self, index: usize, data: LayerData) -> io::Result<()> {
        self.check_insert_position(index)?;
        self.validate_layer_data(&data)?;
        self.layers.insert(index, Layer(data));
        Ok(())
    }

    /// Checks that a new layer can be inserted at `index` without exceeding
    /// the total pixel limit.
    fn check_insert_position(&self, index: usize) -> io::Result<()> {
        if index > self.layers.len() {
            return Err(invalid_input(msg::INVALID_LAYER_POSITION));
        }
        check_layer_pixel_limit(
            self.width,
            self.height,
            self.layers.len() + 1,
            io::ErrorKind::InvalidInput,
            msg::TOTAL_LAYER_LIMIT,
        )?;
        Ok(())
    }

    pub fn remove_layer(&mut self, index: usize) -> io::Result<LayerData> {
        if self.layers.len() <= 1 {
            return Err(invalid_input(msg::LAST_LAYER));
        }
        self.layer(index)?;
        Ok(self.layers.remove(index).0)
    }

    pub fn duplicate_layer(&mut self, index: usize) -> io::Result<usize> {
        self.duplicate_layer_with_name(index, |name| format!("{name} (copy)"))
    }

    pub fn duplicate_layer_with_name(
        &mut self,
        index: usize,
        name: impl FnOnce(&str) -> String,
    ) -> io::Result<usize> {
        let mut copy = self.clone_layer_data(index)?;
        copy.name = name(&copy.name);
        copy.pdn_background = 0;
        let target = index + 1;
        self.insert_layer_data(target, copy)?;
        Ok(target)
    }

    pub fn move_layer(&mut self, from: usize, to: usize) -> io::Result<()> {
        if from >= self.layers.len() || to >= self.layers.len() {
            return Err(invalid_input(msg::INVALID_LAYER_INDEX));
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
            return Err(invalid_input(msg::NO_LAYER_BELOW));
        }
        let bottom_before = self.clone_layer_data(index - 1)?;
        let needs_blend = self.layers[index].visible && self.layers[index].opacity > 0;
        let mut buffers = if needs_blend {
            let byte_len = self.layers[index - 1]
                .pixels
                .len()
                .checked_mul(4)
                .ok_or_else(|| io::Error::new(io::ErrorKind::OutOfMemory, "layer too large"))?;
            Some((try_vec_filled(byte_len, 0u8)?, try_vec_filled(byte_len, 0u8)?))
        } else {
            None
        };
        let top = self.remove_layer(index)?;
        if let Some((ref mut dst, ref mut src)) = buffers {
            let blend_onto =
                |dst: &mut [u8], src: &mut [u8], pixels: &[Pixel], blend: u8, opacity: u8| {
                    pixels_to_bytes(pixels, src);
                    bm_row_inplace(dst, src, blend_mode_op(blend, opacity));
                };
            if bottom_before.visible {
                if bottom_before.opacity == 255 {
                    pixels_to_bytes(&bottom_before.pixels, dst);
                    blend_onto(dst, src, &top.pixels, top.blend, top.opacity);
                } else {
                    blend_onto(
                        dst,
                        src,
                        &bottom_before.pixels,
                        img_pdn::PDNB_NORMAL,
                        bottom_before.opacity,
                    );
                    blend_onto(dst, src, &top.pixels, top.blend, top.opacity);
                    self.layers[index - 1].opacity = 255;
                }
            } else {
                blend_onto(dst, src, &top.pixels, img_pdn::PDNB_NORMAL, top.opacity);
                let bottom = &mut self.layers[index - 1];
                bottom.visible = true;
                bottom.opacity = 255;
                bottom.blend = top.blend;
            }
            bytes_to_pixels(dst, &mut self.layers[index - 1].pixels);
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
            || !is_valid_layer_name(&data.name)
            || data.blend > img_pdn::PDNB_XOR
        {
            return Err(invalid_input(msg::INCOMPATIBLE_LAYER_DATA));
        }
        Ok(())
    }

    pub fn remove_top_layer(&mut self) {
        self.take_top_layer_pixels();
    }

    pub fn take_top_layer_pixels(&mut self) -> Option<Vec<Pixel>> {
        if self.layers.len() > 1 {
            self.layers.pop().map(|layer| layer.0.pixels)
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
            return Err(invalid_input(msg::PIXEL_OUTSIDE));
        }
        let index = (u64::from(y) * u64::from(self.width) + u64::from(x)) as usize;
        self.layer_mut(layer)?.pixels[index] = pixel;
        Ok(())
    }

    pub fn composite(&self) -> io::Result<Vec<Pixel>> {
        self.composite_rect((0, 0), (self.width - 1, self.height - 1))
    }

    pub fn composite_pixel(&self, x: u32, y: u32) -> io::Result<Pixel> {
        Ok(self.composite_rect((x, y), (x, y))?[0])
    }

    pub fn composite_rect(&self, min: (u32, u32), max: (u32, u32)) -> io::Result<Vec<Pixel>> {
        if min.0 > max.0 || min.1 > max.1 || max.0 >= self.width || max.1 >= self.height {
            return Err(invalid_input(msg::INVALID_COMPOSITE_REGION));
        }
        let rect_w = (max.0 - min.0 + 1) as usize;
        let rect_h = (max.1 - min.1 + 1) as usize;
        let count = rect_w.checked_mul(rect_h).ok_or_else(|| {
            invalid_input(msg::REGION_TOO_LARGE)
        })?;
        let byte_len = count.checked_mul(4).ok_or_else(|| {
            invalid_input(msg::REGION_TOO_LARGE)
        })?;
        let mut dst = try_vec_filled(byte_len, 0u8)?;
        let mut src_row = try_vec_filled(rect_w * 4, 0u8)?;

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

        Pixel::from_rgba_bytes(&dst)
    }

    pub fn write_pam(&self, mut output: impl Write) -> io::Result<()> {
        let pixels = self.composite()?;
        writeln!(
            output,
            "P7\nWIDTH {}\nHEIGHT {}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR",
            self.width, self.height,
        )?;
        let bytes = Pixel::to_rgba_bytes(&pixels)?;
        output.write_all(&bytes)
    }

    fn layer(&self, layer: usize) -> io::Result<&Layer> {
        self.layers.get(layer).ok_or_else(|| invalid_input(msg::LAYER_MISSING))
    }

    fn layer_mut(&mut self, layer: usize) -> io::Result<&mut Layer> {
        self.layers.get_mut(layer).ok_or_else(|| invalid_input(msg::LAYER_MISSING))
    }

    fn allocate_pixels(&self) -> io::Result<Vec<Pixel>> {
        let count = usize::try_from(u64::from(self.width) * u64::from(self.height))
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        try_vec_filled(count, Pixel::default())
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
        assert_eq!(doc.layer_name(dup_idx), Some("Topo (copy)"));
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
