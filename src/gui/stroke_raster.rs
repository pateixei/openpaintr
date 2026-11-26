use openpaintr::Pixel;

use super::selection::Point;

const GRID: u16 = 4;
pub const FULL: u16 = u16::MAX;

pub fn sample_offsets() -> impl Iterator<Item = (f64, f64)> {
    let offset = |sample| (f64::from(sample) + 0.5) / f64::from(GRID) - 0.5;
    (0..GRID).flat_map(move |sy| (0..GRID).map(move |sx| (offset(sx), offset(sy))))
}

pub fn samples(mut inside: impl FnMut(f64, f64) -> bool) -> u16 {
    let mut mask = 0;
    for (index, (x, y)) in sample_offsets().enumerate() {
        if inside(x, y) {
            mask |= 1 << index;
        }
    }
    mask
}

pub fn average_samples(mut measure: impl FnMut(f64, f64) -> f64) -> f64 {
    sample_offsets().map(|(x, y)| measure(x, y)).sum::<f64>() / f64::from(GRID * GRID)
}

pub fn apply(color: Pixel, base: Pixel, coverage: u16) -> Pixel {
    let count = coverage.count_ones();
    let total = u32::from(GRID * GRID);
    if count == 0 {
        return base;
    }
    if color == Pixel::default() {
        let alpha = (u32::from(base.0[3]) * (total - count) + total / 2) / total;
        if alpha == 0 {
            Pixel::default()
        } else {
            let [r, g, b, _] = base.0;
            Pixel([r, g, b, alpha as u8])
        }
    } else {
        let opacity = (count * 255 + total / 2) / total;
        color.over(base, opacity as u8)
    }
}

pub fn apply_u8(color: Pixel, base: Pixel, coverage: u8) -> Pixel {
    if coverage == 0 {
        return base;
    }
    if color == Pixel::default() {
        let alpha = (u32::from(base.0[3]) * u32::from(255 - coverage) + 127) / 255;
        if alpha == 0 {
            Pixel::default()
        } else {
            let [r, g, b, _] = base.0;
            Pixel([r, g, b, alpha as u8])
        }
    } else {
        color.over(base, coverage)
    }
}

/// Rasterize the capsule joining the two samples using 4x4 SSAA. Iterate along
/// its major axis so a long diagonal does not scan the entire rectangle surrounding
/// the stroke.
pub fn capsule(
    start: Point,
    end: Point,
    diameter: u32,
    size: Point,
    mut visit: impl FnMut(Point, u16),
) {
    let transpose = start.0.abs_diff(end.0) < start.1.abs_diff(end.1);
    let axis = |(x, y)| if transpose { (y, x) } else { (x, y) };
    let start = axis(start);
    let end = axis(end);
    let size = axis(size);
    let radius = f64::from(diameter) * 0.5;
    let offset = if diameter.is_multiple_of(2) {
        -0.5
    } else {
        0.0
    };
    let (ax, ay) = (f64::from(start.0) + offset, f64::from(start.1) + offset);
    let (bx, by) = (f64::from(end.0) + offset, f64::from(end.1) + offset);
    let (dx, dy) = (bx - ax, by - ay);
    let length_sq = dx * dx + dy * dy;
    let distance_sq = |x: f64, y: f64| {
        let (x, y) = (x - ax, y - ay);
        let t = if length_sq == 0.0 {
            0.0
        } else {
            ((x * dx + y * dy) / length_sq).clamp(0.0, 1.0)
        };
        (x - t * dx).powi(2) + (y - t * dy).powi(2)
    };
    // The furthest sample is less than 0.54 px from the pixel center.
    let outer = (radius + 0.54).powi(2);
    let inner = (radius - 0.54).max(0.0).powi(2);
    let radius_sq = radius * radius;
    let span = (radius + 0.54) * std::f64::consts::SQRT_2;
    let range =
        |min: f64, max: f64, limit: u32| (min.ceil() as u32)..=(max.floor() as u32).min(limit - 1);
    for x in range(
        ax.min(bx) - radius - 0.54,
        ax.max(bx) + radius + 0.54,
        size.0,
    ) {
        let x_center = f64::from(x);
        let axis_y = if dx == 0.0 {
            ay
        } else {
            ay + dy * ((x_center - ax) / dx).clamp(0.0, 1.0)
        };
        for y in range(axis_y - span, axis_y + span, size.1) {
            let y_center = f64::from(y);
            let distance = distance_sq(x_center, y_center);
            if distance > outer {
                continue;
            }
            let coverage = if distance < inner {
                FULL
            } else {
                samples(|sx, sy| {
                    let (dx, dy) = if transpose { (sy, sx) } else { (sx, sy) };
                    distance_sq(x_center + dx, y_center + dy) <= radius_sq
                })
            };
            if coverage != 0 {
                visit(axis((x, y)), coverage);
            }
        }
    }
}

pub fn dab_ssaa(
    center: (f64, f64),
    diameter: u32,
    hardness_pct: u8,
    size: Point,
    mut visit: impl FnMut(Point, u8),
) {
    if size.0 == 0 || size.1 == 0 {
        return;
    }
    let radius = (f64::from(diameter) * 0.5).max(0.5);
    let hardness = (f64::from(hardness_pct.min(100)) / 100.0).clamp(0.0, 1.0);
    let inner_r = radius * hardness;
    let feather = (radius - inner_r).max(1e-6);
    let min_x = (center.0 - radius - 0.6).floor().max(0.0) as u32;
    let min_y = (center.1 - radius - 0.6).floor().max(0.0) as u32;
    let max_x = ((center.0 + radius + 0.6).ceil() as u32).min(size.0 - 1);
    let max_y = ((center.1 + radius + 0.6).ceil() as u32).min(size.1 - 1);
    let outer = radius + 0.54;
    let inner_solid = (inner_r - 0.54).max(-1.0);
    for y in min_y..=max_y {
        let yc = f64::from(y);
        for x in min_x..=max_x {
            let xc = f64::from(x);
            let dist = (xc - center.0).hypot(yc - center.1);
            if dist > outer {
                continue;
            }
            let alpha = if dist <= inner_solid {
                255
            } else {
                let opacity = average_samples(|sx, sy| {
                    let d = (xc + sx - center.0).hypot(yc + sy - center.1);
                    if d >= radius {
                        0.0
                    } else if hardness_pct >= 100 || d <= inner_r {
                        1.0
                    } else {
                        ((radius - d) / feather).clamp(0.0, 1.0)
                    }
                });
                (opacity * 255.0).round() as u8
            };
            if alpha > 0 {
                visit((x, y), alpha);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn segment_dabs(
    start: Point,
    end: Point,
    carry: &mut f64,
    diameter: u32,
    hardness_pct: u8,
    spacing_pct: u8,
    size: Point,
    is_first: bool,
    mut visit: impl FnMut(Point, u8),
) {
    let offset = if diameter.is_multiple_of(2) { -0.5 } else { 0.0 };
    let step = (f64::from(diameter) * f64::from(spacing_pct.max(1)) / 100.0).max(0.25);
    let ax = f64::from(start.0) + offset;
    let ay = f64::from(start.1) + offset;
    let bx = f64::from(end.0) + offset;
    let by = f64::from(end.1) + offset;
    if is_first {
        dab_ssaa((bx, by), diameter, hardness_pct, size, &mut visit);
        *carry = step;
        return;
    }
    let dx = bx - ax;
    let dy = by - ay;
    let dist = dx.hypot(dy);
    if dist < 1e-6 {
        return;
    }
    let ux = dx / dist;
    let uy = dy / dist;
    while *carry <= dist + 1e-6 {
        let t = (*carry).min(dist);
        dab_ssaa((ax + ux * t, ay + uy * t), diameter, hardness_pct, size, &mut visit);
        *carry += step;
    }
    *carry -= dist;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagonal_capsule_uses_ssaa_partial_coverage() {
        let mut coverages = Vec::new();
        capsule((1, 1), (8, 8), 1, (10, 10), |_, mask| {
            coverages.push(mask.count_ones());
        });
        assert!(coverages.iter().any(|&c| (1..16).contains(&c)));
    }
}
