use openpaintr::Pixel;

use super::{selection::Point, stroke_raster};

pub const NEIGHBORS: [(i64, i64); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

pub fn offset((x, y): Point, (dx, dy): (i64, i64), size: Point) -> Option<Point> {
    let x = u32::try_from(i64::from(x) + dx).ok()?;
    let y = u32::try_from(i64::from(y) + dy).ok()?;
    (x < size.0 && y < size.1).then_some((x, y))
}

pub struct Edge {
    foreground: Pixel,
    samples: u32,
}

impl Edge {
    pub fn over(&self, background: Pixel) -> Pixel {
        stroke_raster::apply(self.foreground, background, (1_u16 << self.samples) - 1)
    }

    pub fn deeper(&self, observed: Pixel, background: Pixel) -> Option<Self> {
        let next = Self::fit(observed, background, self.foreground)?;
        // A second SSAA fringe pixel can occur on a diagonal. It must have
        // more foreground coverage, so this never tracks a falling edge.
        (next.samples > self.samples).then_some(next)
    }

    fn fit(observed: Pixel, background: Pixel, foreground: Pixel) -> Option<Self> {
        if background.0[3] != 255
            || observed.0[3] != 255
            || foreground.0[3] != 255
            || observed == background
            || observed == foreground
        {
            return None;
        }
        let channel = (0..3)
            .max_by_key(|&i| background.0[i].abs_diff(foreground.0[i]))
            .unwrap();
        let base = f32::from(background.0[channel]);
        let contrast = f32::from(foreground.0[channel]) - base;
        if contrast.abs() < 32.0 {
            return None;
        }
        let samples = (16.0 * (f32::from(observed.0[channel]) - base) / contrast).round() as u32;
        if !(1..16).contains(&samples) {
            return None;
        }
        let edge = Self {
            foreground,
            samples,
        };
        let reconstructed = edge.over(background);
        reconstructed
            .0
            .iter()
            .zip(observed.0)
            .all(|(&a, b)| a.abs_diff(b) <= 1)
            .then_some(edge)
    }
}

/// Recover an opaque SSAA transition only when a nearby solid foreground
/// reproduces the original edge sample. The flood itself never enters it.
pub fn detect(
    edge: Point,
    direction: (i64, i64),
    size: Point,
    background: Pixel,
    sample: impl Fn(Point) -> Pixel,
) -> Option<Edge> {
    if background.0[3] != 255 {
        return None;
    }
    let observed = sample(edge);
    let mut best = None;
    for step in 1..=2 {
        let ahead = (step * direction.0, step * direction.1);
        let Some(interior) = offset(edge, ahead, size) else {
            continue;
        };
        let foreground = sample(interior);
        if let Some(edge) = Edge::fit(observed, background, foreground)
            && NEIGHBORS.iter().any(|&delta| {
                offset(interior, delta, size).is_some_and(|near| sample(near) == foreground)
            })
        {
            let contrast = |pixel: Pixel| {
                pixel
                    .0
                    .iter()
                    .zip(background.0)
                    .map(|(&front, back)| front.abs_diff(back))
                    .max()
                    .unwrap()
            };
            if best
                .as_ref()
                .is_none_or(|other: &Edge| contrast(foreground) > contrast(other.foreground))
            {
                best = Some(edge);
            }
        }
    }
    best
}
