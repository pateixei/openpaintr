use std::io::{self, BufRead, Seek, Write};

use png::{BitDepth, ColorType, Compression, Decoder, Encoder, Transformations, Unit};

use crate::{Document, Pixel, pdn_io::PdnMetadata};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PngDepth {
    #[default]
    Rgba32,
    Rgb24White,
    Rgb24Black,
}

impl PngDepth {
    pub const ALL: [Self; 3] = [Self::Rgba32, Self::Rgb24White, Self::Rgb24Black];

    pub fn label(self) -> &'static str {
        match self {
            Self::Rgba32 => "RGBA 32 bits (com transparência)",
            Self::Rgb24White => "RGB 24 bits (fundo branco)",
            Self::Rgb24Black => "RGB 24 bits (fundo preto)",
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

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Nenhuma",
            Self::Fast => "Rápida",
            Self::Balanced => "Equilibrada",
            Self::High => "Alta",
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

impl Document {
    pub fn read_png(input: impl BufRead + Seek) -> io::Result<Self> {
        let mut decoder = Decoder::new(input);
        decoder.set_transformations(Transformations::normalize_to_color8());
        let mut reader = decoder.read_info()?;
        if reader.info().animation_control.is_some() {
            return Err(crate::Diagnostic::io(
                io::ErrorKind::Unsupported,
                "error.animated-png",
                "PNG animado ainda não é suportado",
            ));
        }

        let size = reader.output_buffer_size().ok_or_else(|| {
            crate::Diagnostic::io(
                io::ErrorKind::InvalidData,
                "error.invalid-png-size",
                "dimensões de PNG inválidas",
            )
        })?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(size).map_err(io::Error::other)?;
        bytes.resize(size, 0);
        let frame = reader.next_frame(&mut bytes)?;
        reader.finish()?;

        let mut document = Self::new(frame.width, frame.height)?;
        if let Some(density) = reader
            .info()
            .pixel_dims
            .filter(|density| density.unit == Unit::Meter)
        {
            document.pdn_metadata = PdnMetadata::with_dpi(
                f64::from(density.xppu) * 0.0254,
                f64::from(density.yppu) * 0.0254,
            );
        }
        let channels = frame.color_type.samples();
        for (pixel, sample) in document.layers[0]
            .pixels
            .iter_mut()
            .zip(bytes[..frame.buffer_size()].chunks_exact(channels))
        {
            *pixel = Pixel(match sample {
                [gray] => [*gray, *gray, *gray, 255],
                [gray, alpha] => [*gray, *gray, *gray, *alpha],
                [red, green, blue] => [*red, *green, *blue, 255],
                [red, green, blue, alpha] => [*red, *green, *blue, *alpha],
                _ => unreachable!("tipo de cor PNG inválido"),
            });
        }
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
        let mut row = Vec::new();
        row.try_reserve_exact(stride).map_err(io::Error::other)?;
        let bg = match depth {
            PngDepth::Rgb24White => Some(Pixel([255, 255, 255, 255])),
            PngDepth::Rgb24Black => Some(Pixel([0, 0, 0, 255])),
            PngDepth::Rgba32 => None,
        };
        for pixels in pixels.chunks_exact(self.width as usize) {
            row.clear();
            match bg {
                None => row.extend(pixels.iter().flat_map(|pixel| pixel.0)),
                Some(background) => {
                    for &pixel in pixels {
                        let Pixel([r, g, b, _]) = if pixel.0[3] == 255 {
                            pixel
                        } else {
                            pixel.over(background, 255)
                        };
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
