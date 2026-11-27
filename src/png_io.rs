use std::io::{self, BufRead, Seek, Write};

use png::{BitDepth, ColorType, Compression, Decoder, Encoder, Limits, Transformations, Unit};

use crate::{
    Document, Pixel, check_image_dimensions, diagnostic::msg, pdn_io::PdnMetadata, try_vec_filled,
    try_vec_with_capacity,
};

/// Memory the decoder may use for its own buffers and ancillary chunks.
const DECODER_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PngDepth {
    #[default]
    Rgba32,
    Rgb24White,
    Rgb24Black,
}

impl PngDepth {
    pub const ALL: [Self; 3] = [Self::Rgba32, Self::Rgb24White, Self::Rgb24Black];

    pub fn label_key(self) -> &'static str {
        match self {
            Self::Rgba32 => "png-rgba32",
            Self::Rgb24White => "png-rgb-white",
            Self::Rgb24Black => "png-rgb-black",
        }
    }

    pub fn background(self) -> Option<Pixel> {
        match self {
            Self::Rgba32 => None,
            Self::Rgb24White => Some(Pixel([255, 255, 255, 255])),
            Self::Rgb24Black => Some(Pixel([0, 0, 0, 255])),
        }
    }

    pub fn flatten_pixel(self, pixel: Pixel) -> Pixel {
        match self.background() {
            Some(background) if pixel.0[3] < 255 => pixel.over(background, 255),
            _ => pixel,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PngCompression {
    None,
    Fast,
    #[default]
    Balanced,
    High,
}

impl PngCompression {
    pub const ALL: [Self; 4] = [Self::None, Self::Fast, Self::Balanced, Self::High];

    pub fn label_key(self) -> &'static str {
        match self {
            Self::None => "compression-none",
            Self::Fast => "compression-fast",
            Self::Balanced => "compression-balanced",
            Self::High => "compression-high",
        }
    }

    fn into_png(self) -> Compression {
        match self {
            Self::None => Compression::NoCompression,
            Self::Fast => Compression::Fast,
            Self::Balanced => Compression::Balanced,
            Self::High => Compression::High,
        }
    }
}

pub struct DecodedPng {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<Pixel>,
    pub dpi: Option<(f64, f64)>,
}

pub fn decode_png_rgba(input: impl BufRead + Seek) -> io::Result<DecodedPng> {
    let mut decoder = Decoder::new_with_limits(
        input,
        Limits {
            bytes: DECODER_BYTES,
        },
    );
    decoder.set_transformations(Transformations::normalize_to_color8());
    let mut reader = decoder.read_info()?;
    // The header alone decides how much is allocated below.
    check_image_dimensions(reader.info().width, reader.info().height, 1)?;
    if reader.info().animation_control.is_some() {
        return Err(crate::Diagnostic::io(
            io::ErrorKind::Unsupported,
            msg::ANIMATED_PNG,
        ));
    }

    let size = reader
        .output_buffer_size()
        .ok_or_else(|| crate::Diagnostic::io(io::ErrorKind::InvalidData, msg::INVALID_PNG_SIZE))?;
    let mut bytes = try_vec_filled(size, 0u8)?;
    let frame = reader.next_frame(&mut bytes)?;
    reader.finish()?;

    let pixel_count = usize::try_from(u64::from(frame.width) * u64::from(frame.height))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut pixels = try_vec_with_capacity(pixel_count)?;
    let channels = frame.color_type.samples();
    for sample in bytes[..frame.buffer_size()].chunks_exact(channels) {
        pixels.push(Pixel(match sample {
            [gray] => [*gray, *gray, *gray, 255],
            [gray, alpha] => [*gray, *gray, *gray, *alpha],
            [red, green, blue] => [*red, *green, *blue, 255],
            [red, green, blue, alpha] => [*red, *green, *blue, *alpha],
            _ => unreachable!("invalid PNG color type"),
        }));
    }
    let dpi = reader
        .info()
        .pixel_dims
        .filter(|density| density.unit == Unit::Meter)
        .map(|density| {
            (
                f64::from(density.xppu) * 0.0254,
                f64::from(density.yppu) * 0.0254,
            )
        });
    Ok(DecodedPng {
        width: frame.width,
        height: frame.height,
        pixels,
        dpi,
    })
}

impl Document {
    pub fn read_png(input: impl BufRead + Seek) -> io::Result<Self> {
        let mut decoded = decode_png_rgba(input)?;
        let mut document = Self::new(decoded.width, decoded.height)?;
        if let Some((dpi_x, dpi_y)) = decoded.dpi {
            document.pdn_metadata = PdnMetadata::with_dpi(dpi_x, dpi_y);
        }
        document.swap_layer_pixels(0, &mut decoded.pixels)?;
        Ok(document)
    }

    pub fn write_png(&self, output: impl Write) -> io::Result<()> {
        self.write_png_with_options(output, PngDepth::Rgba32, PngCompression::Balanced)
    }

    pub fn write_png_with_options(
        &self,
        output: impl Write,
        depth: PngDepth,
        compression: PngCompression,
    ) -> io::Result<()> {
        let pixels = self.composite()?;
        let mut encoder = Encoder::new(output, self.width, self.height);
        let color_type = match depth {
            PngDepth::Rgba32 => ColorType::Rgba,
            PngDepth::Rgb24White | PngDepth::Rgb24Black => ColorType::Rgb,
        };
        encoder.set_color(color_type);
        encoder.set_depth(BitDepth::Eight);
        encoder.set_compression(compression.into_png());
        let mut writer = encoder.write_header()?;
        let mut stream = writer.stream_writer()?;
        let stride = self.width as usize * if depth == PngDepth::Rgba32 { 4 } else { 3 };
        let mut row = try_vec_with_capacity(stride)?;
        let bg = depth.background();
        for pixels in pixels.chunks_exact(self.width as usize) {
            row.clear();
            match bg {
                None => row.extend(pixels.iter().flat_map(|pixel| pixel.0)),
                Some(_) => {
                    for &pixel in pixels {
                        let Pixel([r, g, b, _]) = depth.flatten_pixel(pixel);
                        row.extend_from_slice(&[r, g, b]);
                    }
                }
            }
            stream.write_all(&row)?;
        }
        stream.finish()?;
        writer.finish()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn reads_physical_density_for_document_rulers() -> io::Result<()> {
        let mut bytes = Vec::new();
        let mut encoder = Encoder::new(&mut bytes, 1, 1);
        encoder.set_color(ColorType::Rgba);
        encoder.set_pixel_dims(Some(png::PixelDimensions {
            xppu: 10000,
            yppu: 5000,
            unit: Unit::Meter,
        }));
        encoder.write_header()?.write_image_data(&[0; 4])?;
        assert_eq!(
            Document::read_png(Cursor::new(bytes))?.resolution_dpi(),
            (254.0, 127.0)
        );
        Ok(())
    }

    #[test]
    fn round_trips_composited_rgba() -> io::Result<()> {
        let mut original = Document::new(2, 2)?;
        original.set_pixel(0, 0, 0, Pixel([30, 40, 50, 128]))?;
        let overlay = original.add_layer("Teste")?;
        original.set_pixel(overlay, 0, 0, Pixel([10, 20, 30, 200]))?;
        original.set_pixel(overlay, 1, 1, Pixel([200, 100, 50, 255]))?;

        let mut bytes = Vec::new();
        original.write_png(&mut bytes)?;
        let decoded = Document::read_png(Cursor::new(bytes))?;
        assert_eq!((decoded.width, decoded.height), (2, 2));
        assert_eq!(decoded.layers.len(), 1);
        assert_eq!(decoded.composite()?, original.composite()?);
        Ok(())
    }

    #[test]
    fn expands_palette_and_preserves_alpha() -> io::Result<()> {
        let mut bytes = Vec::new();
        let mut encoder = Encoder::new(&mut bytes, 2, 1);
        encoder.set_color(ColorType::Indexed);
        encoder.set_depth(BitDepth::One);
        encoder.set_palette(vec![10, 20, 30, 100, 110, 120]);
        encoder.set_trns(vec![40]);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&[0b0100_0000])?;
        writer.finish()?;

        let decoded = Document::read_png(Cursor::new(bytes))?;
        assert_eq!(
            decoded.composite()?,
            [Pixel([10, 20, 30, 40]), Pixel([100, 110, 120, 255])]
        );
        Ok(())
    }
}
