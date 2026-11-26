use std::io;

use super::{selection::Point, stroke_raster};

/// Each row first finds its nearest covered pixel horizontally. For each column,
/// the lower envelope of the row-distance parabolas gives the nearest site.
pub fn expand(
    (width, height): Point,
    radius: f64,
    coverage: impl Fn(Point) -> u8,
    mut set: impl FnMut(Point, u8),
) -> io::Result<()> {
    let width = width as usize;
    let height = height as usize;
    let area = width
        .checked_mul(height)
        .ok_or(io::ErrorKind::OutOfMemory)?;
    let mut nearest = Vec::new();
    nearest.try_reserve_exact(area).map_err(io::Error::other)?;
    const EMPTY: u32 = u32::MAX;
    nearest.resize(area, EMPTY);
    for (y, row) in nearest.chunks_exact_mut(width).enumerate() {
        let mut site = EMPTY;
        for (x, nearest) in row.iter_mut().enumerate() {
            if coverage((x as u32, y as u32)) > 0 {
                site = x as u32;
            }
            *nearest = site;
        }
        let mut site = EMPTY;
        for (x, nearest) in row.iter_mut().enumerate().rev() {
            let x = x as u32;
            if *nearest == x {
                site = x;
            }
            if site != EMPTY && (*nearest == EMPTY || site - x < x - *nearest) {
                *nearest = site;
            }
        }
    }

    let mut envelope = Vec::<(usize, f64)>::new();
    envelope
        .try_reserve_exact(height)
        .map_err(io::Error::other)?;
    for x in 0..width {
        envelope.clear();
        let horizontal = |y| nearest[y * width + x];
        let cost = |y| (f64::from(horizontal(y)) - x as f64).powi(2);
        for y in 0..height {
            if horizontal(y) == EMPTY {
                continue;
            }
            let mut start = f64::NEG_INFINITY;
            while let Some(&(previous, previous_start)) = envelope.last() {
                start = (y + previous) as f64 / 2.0
                    + (cost(y) - cost(previous)) / (2.0 * (y - previous) as f64);
                if start > previous_start {
                    break;
                }
                envelope.pop();
            }
            envelope.push((y, start));
        }
        if envelope.is_empty() {
            continue;
        }

        let mut index = 0;
        for y in 0..height {
            while envelope
                .get(index + 1)
                .is_some_and(|&(_, start)| start < y as f64)
            {
                index += 1;
            }
            let (source_y, _) = envelope[index];
            let point = (x as u32, y as u32);
            let base = coverage(point);
            let source_x = horizontal(source_y);
            let dx = x as f64 - f64::from(source_x);
            let dy = y as f64 - source_y as f64;
            let mut value = base;
            if base < 255 && dx.abs().max(dy.abs()) < radius + 1.0 {
                // An antialiased source pixel only covers part of its cell.
                // Estimate its footprint with an equal-area square for distance sampling.
                let source_coverage = coverage((source_x, source_y as u32));
                let half_side = 0.5 * (f64::from(source_coverage) / 255.0).sqrt();
                let opacity = stroke_raster::average_samples(|sx, sy| {
                    let edge_x = ((dx + sx).abs() - half_side).max(0.0);
                    let edge_y = ((dy + sy).abs() - half_side).max(0.0);
                    (1.0 - (edge_x * edge_x + edge_y * edge_y).sqrt() / radius).max(0.0)
                });
                value = base.max((opacity * 255.0).round() as u8);
            }
            if value > 0 {
                set(point, value);
            }
        }
    }
    Ok(())
}
