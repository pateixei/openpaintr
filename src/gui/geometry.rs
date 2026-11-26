use std::io;

use openpaintr::{Document, Pixel};

use super::selection::Point;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Resampling {
    #[default]
    Nearest,
    Antialiased,
    Bilinear,
}

impl Resampling {
    pub const ALL: [Self; 3] = [Self::Nearest, Self::Antialiased, Self::Bilinear];

    pub fn label(self) -> &'static str {
        match self {
            Self::Nearest => "Vizinho mais próximo",
            Self::Antialiased => "Vizinho mais próximo com antialiasing",
            Self::Bilinear => "Bilinear",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orthogonal {
    FlipX,
    FlipY,
    Clockwise,
    Counterclockwise,
    HalfTurn,
}

impl Orthogonal {
    pub fn label(self) -> &'static str {
        match self {
            Self::FlipX => "Inverter horizontalmente",
            Self::FlipY => "Inverter verticalmente",
            Self::Clockwise => "Girar 90° horário",
            Self::Counterclockwise => "Girar 90° anti-horário",
            Self::HalfTurn => "Girar 180°",
        }
    }

    pub fn size(self, size: Point) -> Point {
        match self {
            Self::Clockwise | Self::Counterclockwise => (size.1, size.0),
            _ => size,
        }
    }

    pub fn source(self, size: Point, (x, y): Point) -> Point {
        match self {
            Self::FlipX => (size.0 - 1 - x, y),
            Self::FlipY => (x, size.1 - 1 - y),
            Self::Clockwise => (y, size.1 - 1 - x),
            Self::Counterclockwise => (size.0 - 1 - y, x),
            Self::HalfTurn => (size.0 - 1 - x, size.1 - 1 - y),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Affine {
    pub center: (f64, f64),
    pub shift: (f64, f64),
    pub scale: (f64, f64),
    pub degrees: f64,
}

impl Affine {
    pub fn centered(size: Point) -> Self {
        Self {
            center: (f64::from(size.0) * 0.5, f64::from(size.1) * 0.5),
            shift: (0.0, 0.0),
            scale: (1.0, 1.0),
            degrees: 0.0,
        }
    }

    pub fn inverse(self, (x, y): (f64, f64)) -> (f64, f64) {
        let (sin, cos) = self.degrees.to_radians().sin_cos();
        let x = x - self.center.0 - self.shift.0;
        let y = y - self.center.1 - self.shift.1;
        (
            (x * cos + y * sin) / self.scale.0 + self.center.0,
            (-x * sin + y * cos) / self.scale.1 + self.center.1,
        )
    }

    pub fn forward(self, (x, y): (f64, f64)) -> (f64, f64) {
        let (sin, cos) = self.degrees.to_radians().sin_cos();
        let x = (x - self.center.0) * self.scale.0;
        let y = (y - self.center.1) * self.scale.1;
        (
            x * cos - y * sin + self.center.0 + self.shift.0,
            x * sin + y * cos + self.center.1 + self.shift.1,
        )
    }

    pub fn is_identity(self) -> bool {
        self.shift == (0.0, 0.0) && self.scale == (1.0, 1.0) && self.degrees == 0.0
    }
}

pub fn pixel(source: &[Pixel], size: Point, x: i64, y: i64) -> Pixel {
    if x < 0 || y < 0 || x >= i64::from(size.0) || y >= i64::from(size.1) {
        return Pixel::default();
    }
    source[y as usize * size.0 as usize + x as usize]
}

pub fn resample(
    at: impl Fn(i64, i64) -> Pixel,
    inverse: impl Fn((f64, f64)) -> (f64, f64),
    point: Point,
    method: Resampling,
) -> Pixel {
    let (x, y) = (f64::from(point.0), f64::from(point.1));
    let nearest = |position| {
        let (sx, sy) = inverse(position);
        at(sx.floor() as i64, sy.floor() as i64)
    };
    match method {
        Resampling::Nearest => nearest((x + 0.5, y + 0.5)),
        Resampling::Antialiased => {
            let mut samples = [(Pixel::default(), 1.0 / 16.0); 16];
            for (index, sample) in samples.iter_mut().enumerate() {
                sample.0 = nearest((
                    x + (index % 4) as f64 * 0.25 + 0.125,
                    y + (index / 4) as f64 * 0.25 + 0.125,
                ));
            }
            weighted(samples)
        }
        Resampling::Bilinear => {
            let (sx, sy) = inverse((x + 0.5, y + 0.5));
            let (sx, sy) = (sx - 0.5, sy - 0.5);
            let (left, top) = (sx.floor() as i64, sy.floor() as i64);
            let (dx, dy) = (sx - left as f64, sy - top as f64);
            weighted([
                (at(left, top), (1.0 - dx) * (1.0 - dy)),
                (at(left + 1, top), dx * (1.0 - dy)),
                (at(left, top + 1), (1.0 - dx) * dy),
                (at(left + 1, top + 1), dx * dy),
            ])
        }
    }
}

fn weighted<const N: usize>(samples: [(Pixel, f64); N]) -> Pixel {
    let mut rgba = [0.0; 4];
    for (Pixel([r, g, b, a]), weight) in samples {
        let opacity = f64::from(a) * weight;
        for (channel, value) in rgba[..3].iter_mut().zip([r, g, b]) {
            *channel += f64::from(value) * opacity;
        }
        rgba[3] += opacity;
    }
    if rgba[3].round() == 0.0 {
        return Pixel::default();
    }
    Pixel([
        (rgba[0] / rgba[3]).round() as u8,
        (rgba[1] / rgba[3]).round() as u8,
        (rgba[2] / rgba[3]).round() as u8,
        rgba[3].round() as u8,
    ])
}

pub fn resize(document: &Document, size: Point, method: Resampling) -> io::Result<Document> {
    let old = document.dimensions();
    let inverse = |(x, y): (f64, f64)| {
        (
            x * f64::from(old.0) / f64::from(size.0),
            y * f64::from(old.1) / f64::from(size.1),
        )
    };
    document.remap(size.0, size.1, |source, x, y| {
        resample(
            |x, y| {
                pixel(
                    source,
                    old,
                    x.clamp(0, i64::from(old.0) - 1),
                    y.clamp(0, i64::from(old.1) - 1),
                )
            },
            inverse,
            (x, y),
            method,
        )
    })
}

pub fn canvas_size(document: &Document, size: Point, offset: (i64, i64)) -> io::Result<Document> {
    let old = document.dimensions();
    document.remap(size.0, size.1, |source, x, y| {
        pixel(
            source,
            old,
            i64::from(x) - offset.0,
            i64::from(y) - offset.1,
        )
    })
}

pub fn orthogonal(document: &Document, operation: Orthogonal) -> io::Result<Document> {
    let old = document.dimensions();
    let size = operation.size(old);
    document.remap(size.0, size.1, |source, x, y| {
        let (x, y) = operation.source(old, (x, y));
        pixel(source, old, i64::from(x), i64::from(y))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resizing_and_rotating_keep_layer_properties_and_alpha() -> io::Result<()> {
        let mut document = Document::new(3, 2)?;
        let layer = document.add_layer("Oculta")?;
        document.set_layer_properties(layer, false, 127)?;
        let red = Pixel([200, 0, 0, 113]);
        document.set_pixel(layer, 0, 1, red)?;
        let rotated = orthogonal(&document, Orthogonal::Clockwise)?;
        assert_eq!(rotated.dimensions(), (2, 3));
        assert_eq!(rotated.layer_name(layer), Some("Oculta"));
        assert_eq!(rotated.layer_properties(layer), Some((false, 127)));
        assert_eq!(rotated.pixel(layer, 0, 0), Some(red));
        assert_eq!(
            orthogonal(&rotated, Orthogonal::Counterclockwise)?.layer_pixels(layer),
            document.layer_pixels(layer)
        );
        let resized = resize(&rotated, (4, 6), Resampling::Nearest)?;
        assert_eq!(resized.pixel(layer, 1, 1), Some(red));
        assert_eq!(resized.pixel(layer, 2, 0), Some(Pixel::default()));
        Ok(())
    }

    #[test]
    fn interpolation_uses_premultiplied_coverage_and_canvas_clips() -> io::Result<()> {
        let red = Pixel([200, 0, 0, 255]);
        let mut document = Document::new(1, 1)?;
        document.set_pixel(0, 0, 0, red)?;
        let padded = canvas_size(&document, (3, 2), (2, 1))?;
        assert_eq!(padded.pixel(0, 0, 0), Some(Pixel::default()));
        assert_eq!(padded.pixel(0, 2, 1), Some(red));
        let clipped = canvas_size(&padded, (1, 1), (-2, -1))?;
        assert_eq!(clipped.pixel(0, 0, 0), Some(red));
        for method in [Resampling::Antialiased, Resampling::Bilinear] {
            let value = resample(
                |x, y| pixel(&[red], (1, 1), x, y),
                |(x, y)| (x - 0.5, y),
                (0, 0),
                method,
            );
            assert_eq!(value, Pixel([200, 0, 0, 128]));
        }
        Ok(())
    }
}
