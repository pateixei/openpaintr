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

/// Exact analytical area of the intersection of the unit pixel square `[-0.5, 0.5]^2`
/// with the half-plane `nx * x + ny * y <= sd`.
pub fn half_plane_coverage(nx: f64, ny: f64, sd: f64) -> f64 {
    let len = nx.hypot(ny);
    if len < 1e-12 {
        return if sd >= 0.0 { 1.0 } else { 0.0 };
    }
    let ux = (nx / len).abs();
    let uy = (ny / len).abs();
    let (a, b) = if ux >= uy { (ux, uy) } else { (uy, ux) };
    if b <= 1e-12 {
        return ((sd + 0.5 * a) / a).clamp(0.0, 1.0);
    }
    let h = 0.5 * (a + b);
    let m = 0.5 * (a - b);
    if sd >= h {
        1.0
    } else if sd <= -h {
        0.0
    } else if sd.abs() <= m {
        (0.5 + sd / a).clamp(0.0, 1.0)
    } else if sd > m {
        (1.0 - (h - sd).powi(2) / (2.0 * a * b)).clamp(0.0, 1.0)
    } else {
        ((h + sd).powi(2) / (2.0 * a * b)).clamp(0.0, 1.0)
    }
}

fn circle_quadrant_area(ax: f64, bx: f64, ay: f64, by: f64, r: f64) -> f64 {
    if ax >= bx || ay >= by || r <= 0.0 {
        return 0.0;
    }
    let r2 = r * r;
    let x_max = (r2 - ay * ay).max(0.0).sqrt();
    let u0 = ax;
    let u1 = bx.min(x_max);
    if u0 >= u1 {
        return 0.0;
    }
    let x_top = (r2 - by * by).max(0.0).sqrt();
    let flat_end = u1.min(x_top);
    let mut area = if flat_end > u0 {
        (by - ay) * (flat_end - u0)
    } else {
        0.0
    };
    let arc_start = u0.max(x_top);
    if u1 > arc_start {
        let antideriv = |x: f64| -> f64 {
            let root = (r2 - x * x).max(0.0).sqrt();
            0.5 * (x * root + r2 * (x / r).clamp(-1.0, 1.0).asin())
        };
        area += (antideriv(u1) - antideriv(arc_start)) - ay * (u1 - arc_start);
    }
    area.max(0.0)
}

/// Exact analytical area of the intersection of a disk of radius `r` at the origin
/// with the unit pixel square `[dx - 0.5, dx + 0.5] x [dy - 0.5, dy + 0.5]`.
pub fn circle_pixel_area(dx: f64, dy: f64, r: f64) -> f64 {
    let x0 = dx - 0.5;
    let x1 = dx + 0.5;
    let y0 = dy - 0.5;
    let y1 = dy + 0.5;
    let x_spans = [(x0.max(0.0), x1.max(0.0)), ((-x1).max(0.0), (-x0).max(0.0))];
    let y_spans = [(y0.max(0.0), y1.max(0.0)), ((-y1).max(0.0), (-y0).max(0.0))];
    let mut total = 0.0;
    for &(ax, bx) in &x_spans {
        for &(ay, by) in &y_spans {
            total += circle_quadrant_area(ax, bx, ay, by, r);
        }
    }
    total.clamp(0.0, 1.0)
}

fn soft_radial_coverage(dist: f64, inner_r: f64, radius: f64) -> f64 {
    let feather = radius - inner_r;
    let lo = dist - 0.5;
    let hi = dist + 0.5;
    let solid = (hi.min(inner_r) - lo).max(0.0);
    let rlo = lo.max(inner_r);
    let rhi = hi.min(radius);
    let ramp = if rhi > rlo && feather > 1e-9 {
        (rhi - rlo) * ((radius - 0.5 * (rlo + rhi)) / feather)
    } else {
        0.0
    };
    (solid + ramp).clamp(0.0, 1.0)
}

fn coverage_to_u8(cov: f64) -> u8 {
    (cov * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Rasterize the capsule joining the two samples using analytical / SDF coverage
/// with 256 alpha levels. Canonicalizes endpoint order so retracing is deterministic.
pub fn capsule_analytical(
    start: Point,
    end: Point,
    diameter: u32,
    hardness_pct: u8,
    size: Point,
    mut visit: impl FnMut(Point, u8),
) {
    if size.0 == 0 || size.1 == 0 {
        return;
    }
    let (start, end) = if start <= end { (start, end) } else { (end, start) };
    let transpose = start.0.abs_diff(end.0) < start.1.abs_diff(end.1);
    let axis = |(x, y)| if transpose { (y, x) } else { (x, y) };
    let start = axis(start);
    let end = axis(end);
    let size = axis(size);
    let radius = f64::from(diameter.max(1)) * 0.5;
    let hardness = (f64::from(hardness_pct.min(100)) / 100.0).clamp(0.0, 1.0);
    let inner_r = radius * hardness;
    let offset = if diameter.is_multiple_of(2) {
        -0.5
    } else {
        0.0
    };
    let (ax, ay) = (f64::from(start.0) + offset, f64::from(start.1) + offset);
    let (bx, by) = (f64::from(end.0) + offset, f64::from(end.1) + offset);
    let (dx, dy) = (bx - ax, by - ay);
    let length_sq = dx * dx + dy * dy;
    let length = length_sq.sqrt();
    let radius_sq = radius * radius;
    let outer = radius + 0.72;
    let inner_solid = if hardness_pct >= 100 {
        (radius - 0.72).max(-1.0)
    } else {
        (inner_r - 0.72).max(-1.0)
    };
    let span = (radius + 0.72) * std::f64::consts::SQRT_2;
    let range = |min: f64, max: f64, limit: u32| {
        (min.floor().max(0.0) as u32)..=(max.ceil().max(0.0) as u32).min(limit - 1)
    };

    for x in range(
        ax.min(bx) - radius - 0.72,
        ax.max(bx) + radius + 0.72,
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
            let rx = x_center - ax;
            let ry = y_center - ay;
            let t = if length_sq == 0.0 {
                0.0
            } else {
                ((rx * dx + ry * dy) / length_sq).clamp(0.0, 1.0)
            };
            let cx = ax + t * dx;
            let cy = ay + t * dy;
            let vx = x_center - cx;
            let vy = y_center - cy;
            let dist = vx.hypot(vy);
            if dist > outer {
                continue;
            }
            let alpha = if dist <= inner_solid {
                255
            } else if hardness_pct >= 100 {
                let cov = if radius >= 1.0 {
                    if dist < 1e-9 {
                        1.0
                    } else {
                        half_plane_coverage(vx / dist, vy / dist, radius - dist)
                    }
                } else if length_sq == 0.0 {
                    circle_pixel_area(rx, ry, radius)
                } else {
                    let da = (x_center - ax).hypot(y_center - ay);
                    let db = (x_center - bx).hypot(y_center - by);
                    if da >= radius + 0.71 && db >= radius + 0.71 {
                        let nx = -dy / length;
                        let ny = dx / length;
                        let d_perp = rx * nx + ry * ny;
                        (half_plane_coverage(nx, ny, radius + d_perp)
                            + half_plane_coverage(nx, ny, radius - d_perp)
                            - 1.0)
                            .clamp(0.0, 1.0)
                    } else {
                        let mut hits = 0u32;
                        for sy in 0..16 {
                            let oy = (f64::from(sy) + 0.5) / 16.0 - 0.5;
                            for sx in 0..16 {
                                let ox = (f64::from(sx) + 0.5) / 16.0 - 0.5;
                                let px = x_center + ox - ax;
                                let py = y_center + oy - ay;
                                let pt = ((px * dx + py * dy) / length_sq).clamp(0.0, 1.0);
                                let d2 = (px - pt * dx).powi(2) + (py - pt * dy).powi(2);
                                if d2 <= radius_sq {
                                    hits += 1;
                                }
                            }
                        }
                        f64::from(hits) / 256.0
                    }
                };
                coverage_to_u8(cov)
            } else {
                coverage_to_u8(soft_radial_coverage(dist, inner_r, radius))
            };
            if alpha > 0 {
                visit(axis((x, y)), alpha);
            }
        }
    }
}

pub fn dab_analytical(
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
    let min_x = (center.0 - radius - 0.72).floor().max(0.0) as u32;
    let min_y = (center.1 - radius - 0.72).floor().max(0.0) as u32;
    let max_x = ((center.0 + radius + 0.72).ceil().max(0.0) as u32).min(size.0 - 1);
    let max_y = ((center.1 + radius + 0.72).ceil().max(0.0) as u32).min(size.1 - 1);
    let outer = radius + 0.72;
    let inner_solid = if hardness_pct >= 100 {
        (radius - 0.72).max(-1.0)
    } else {
        (inner_r - 0.72).max(-1.0)
    };
    for y in min_y..=max_y {
        let yc = f64::from(y);
        let vy = yc - center.1;
        for x in min_x..=max_x {
            let xc = f64::from(x);
            let vx = xc - center.0;
            let dist = vx.hypot(vy);
            if dist > outer {
                continue;
            }
            let alpha = if dist <= inner_solid {
                255
            } else if hardness_pct >= 100 {
                let cov = if radius < 1.0 {
                    circle_pixel_area(vx, vy, radius)
                } else if dist < 1e-9 {
                    1.0
                } else {
                    half_plane_coverage(vx / dist, vy / dist, radius - dist)
                };
                coverage_to_u8(cov)
            } else {
                coverage_to_u8(soft_radial_coverage(dist, inner_r, radius))
            };
            if alpha > 0 {
                visit((x, y), alpha);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn segment_dabs_analytical(
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
        dab_analytical((bx, by), diameter, hardness_pct, size, &mut visit);
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
        dab_analytical((ax + ux * t, ay + uy * t), diameter, hardness_pct, size, &mut visit);
        *carry += step;
    }
    *carry -= dist;
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;

    #[test]
    fn diagonal_capsule_uses_ssaa_partial_coverage() {
        let mut coverages = Vec::new();
        capsule((1, 1), (8, 8), 1, (10, 10), |_, mask| {
            coverages.push(mask.count_ones());
        });
        assert!(coverages.iter().any(|&c| (1..16).contains(&c)));
    }

    #[test]
    fn analytical_geometry_helpers_are_exact() {
        assert!((half_plane_coverage(1.0, 0.0, 0.0) - 0.5).abs() < 1e-12);
        assert!((half_plane_coverage(1.0, 0.0, 0.25) - 0.75).abs() < 1e-12);
        assert!((half_plane_coverage(1.0, 1.0, 0.0) - 0.5).abs() < 1e-12);
        // 45-degree strip of width 1 has area sqrt(2) per unit column.
        let mut col_sum = 0.0;
        let inv_sqrt2 = std::f64::consts::FRAC_1_SQRT_2;
        for y in -3..=3 {
            let d = f64::from(y) * inv_sqrt2;
            let cov = (half_plane_coverage(inv_sqrt2, inv_sqrt2, 0.5 + d)
                + half_plane_coverage(inv_sqrt2, inv_sqrt2, 0.5 - d)
                - 1.0)
                .clamp(0.0, 1.0);
            col_sum += cov;
        }
        assert!((col_sum - std::f64::consts::SQRT_2).abs() < 1e-9);

        assert!((circle_pixel_area(0.0, 0.0, 0.5) - std::f64::consts::FRAC_PI_4).abs() < 1e-9);
        assert!((circle_pixel_area(0.0, 0.0, 1.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn analytical_capsule_is_symmetric_and_exceeds_ssaa_levels() {
        let mut forward = BTreeMap::new();
        let mut reverse = BTreeMap::new();
        capsule_analytical((1, 2), (30, 5), 3, 100, (32, 16), |pt, cov| {
            forward.insert(pt, cov);
        });
        capsule_analytical((30, 5), (1, 2), 3, 100, (32, 16), |pt, cov| {
            reverse.insert(pt, cov);
        });
        assert_eq!(forward, reverse);

        let distinct: BTreeSet<u8> = forward.values().copied().collect();
        assert!(
            distinct.len() > 17,
            "expected >17 distinct analytical levels, got {}",
            distinct.len()
        );

        let mut soft = BTreeMap::new();
        capsule_analytical((2, 8), (20, 8), 8, 50, (24, 16), |pt, cov| {
            soft.insert(pt, cov);
        });
        assert!(soft.values().any(|&v| v == 255));
        assert!(soft.values().any(|&v| (1..255).contains(&v)));
    }
}
