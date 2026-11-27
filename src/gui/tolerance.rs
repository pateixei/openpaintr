use super::i18n::text as tr;
use openpaintr::{Pixel, mul_u8};

use super::{
    selection::{Point, Selection},
    stroke_raster,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlphaMatchMode {
    #[default]
    Premultiplied,
    Straight,
}

impl AlphaMatchMode {
    pub const ALL: [Self; 2] = [Self::Premultiplied, Self::Straight];

    pub fn label(self) -> &'static str {
        match self {
            Self::Premultiplied => tr("premultiplied-alpha"),
            Self::Straight => tr("straight-alpha"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StrokeBlend {
    #[default]
    Normal,
    Overwrite,
}

impl StrokeBlend {
    pub const ALL: [Self; 2] = [Self::Normal, Self::Overwrite];

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => tr("composite-stroke"),
            Self::Overwrite => tr("overwrite"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FillPattern {
    #[default]
    Solid,
    Horizontal,
    Vertical,
    DiagonalUp,
    DiagonalDown,
    Checker,
    Dots,
    Grid,
}

impl FillPattern {
    pub const ALL: [Self; 8] = [
        Self::Solid,
        Self::Horizontal,
        Self::Vertical,
        Self::DiagonalUp,
        Self::DiagonalDown,
        Self::Checker,
        Self::Dots,
        Self::Grid,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Solid => tr("solid-color"),
            Self::Horizontal => tr("horizontal-lines"),
            Self::Vertical => tr("vertical-lines"),
            Self::DiagonalUp => tr("diagonal-up"),
            Self::DiagonalDown => tr("diagonal-down"),
            Self::Checker => tr("checkerboard"),
            Self::Dots => tr("dots"),
            Self::Grid => tr("grid"),
        }
    }

    pub fn sample(self, x: u32, y: u32, primary: Pixel, secondary: Pixel) -> Pixel {
        let on = match self {
            Self::Solid => true,
            Self::Horizontal => y % 4 < 2,
            Self::Vertical => x % 4 < 2,
            Self::DiagonalUp => (x + y) % 4 < 2,
            Self::DiagonalDown => (x + 4 - (y % 4)) % 4 < 2,
            Self::Checker => ((x / 4) + (y / 4)).is_multiple_of(2),
            Self::Dots => (x % 4 == 1) && (y % 4 == 1),
            Self::Grid => x.is_multiple_of(4) || y.is_multiple_of(4),
        };
        if on { primary } else { secondary }
    }
}

pub fn color_matches(
    reference: Pixel,
    candidate: Pixel,
    tolerance_pct: u8,
    alpha_mode: AlphaMatchMode,
) -> bool {
    if tolerance_pct >= 100 {
        return true;
    }
    if tolerance_pct == 0 {
        return match alpha_mode {
            AlphaMatchMode::Straight => reference == candidate,
            AlphaMatchMode::Premultiplied => {
                (reference.0[3] == 0 && candidate.0[3] == 0)
                    || reference.premul_channels() == candidate.premul_channels()
            }
        };
    }
    let tol = (u32::from(tolerance_pct) * 255 + 50) / 100;
    let thr = tol * tol * 4;
    let a = match alpha_mode {
        AlphaMatchMode::Straight => reference.0,
        AlphaMatchMode::Premultiplied => reference.premul_channels(),
    };
    let b = match alpha_mode {
        AlphaMatchMode::Straight => candidate.0,
        AlphaMatchMode::Premultiplied => candidate.premul_channels(),
    };
    let sum: u32 = a
        .into_iter()
        .zip(b)
        .map(|(x, y)| {
            let d = i32::from(x) - i32::from(y);
            (d * d) as u32
        })
        .sum();
    sum <= thr
}

pub fn recolor_pixel(
    px: Pixel,
    target: Pixel,
    replacement: Pixel,
    tolerance_pct: u8,
    alpha_mode: AlphaMatchMode,
) -> Option<Pixel> {
    if px.0[3] == 0 || !color_matches(target, px, tolerance_pct, alpha_mode) {
        return None;
    }
    let alpha = mul_u8(px.0[3], replacement.0[3]);
    if alpha == 0 {
        return Some(Pixel::default());
    }
    Some(Pixel([
        (i32::from(px.0[0]) + i32::from(replacement.0[0]) - i32::from(target.0[0])).clamp(0, 255)
            as u8,
        (i32::from(px.0[1]) + i32::from(replacement.0[1]) - i32::from(target.0[1])).clamp(0, 255)
            as u8,
        (i32::from(px.0[2]) + i32::from(replacement.0[2]) - i32::from(target.0[2])).clamp(0, 255)
            as u8,
        alpha,
    ]))
}

pub(super) fn blend_coverage_in_place(
    pixels: &mut [Pixel],
    size: Point,
    selection: Option<&Selection>,
    covs: impl IntoIterator<Item = (Point, u8)>,
    blend: StrokeBlend,
    mut color_at: impl FnMut(Point) -> Pixel,
) {
    let w = size.0 as usize;
    for (pt @ (x, y), cov) in covs {
        let sel_cov = selection.map_or(255, |sel| sel.coverage_u8(pt));
        let eff = mul_u8(cov, sel_cov);
        if eff == 0 {
            continue;
        }
        let idx = y as usize * w + x as usize;
        pixels[idx] = blend_covered(color_at(pt), pixels[idx], eff, blend);
    }
}

pub fn blend_covered(fg: Pixel, base: Pixel, alpha_u8: u8, blend: StrokeBlend) -> Pixel {
    if alpha_u8 == 0 {
        return base;
    }
    match blend {
        StrokeBlend::Normal => stroke_raster::apply_u8(fg, base, alpha_u8),
        StrokeBlend::Overwrite => {
            if alpha_u8 == 255 {
                return if fg.0[3] == 0 { Pixel::default() } else { fg };
            }
            let w1 = u32::from(alpha_u8);
            let w0 = 255 - w1;
            let a = (u32::from(base.0[3]) * w0 + u32::from(fg.0[3]) * w1 + 127) / 255;
            if a == 0 {
                return Pixel::default();
            }
            let denom = a * 255;
            let mut out = [0, 0, 0, a as u8];
            for (i, channel) in out.iter_mut().enumerate().take(3) {
                let num = u32::from(base.0[i]) * u32::from(base.0[3]) * w0
                    + u32::from(fg.0[i]) * u32::from(fg.0[3]) * w1;
                *channel = ((num + denom / 2) / denom).min(255) as u8;
            }
            Pixel(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_tolerance_honors_alpha_match_modes() {
        let trans_red = Pixel([255, 0, 0, 0]);
        let trans_blue = Pixel([0, 0, 255, 0]);
        assert!(color_matches(
            trans_red,
            trans_blue,
            0,
            AlphaMatchMode::Premultiplied
        ));
        assert!(!color_matches(
            trans_red,
            trans_blue,
            0,
            AlphaMatchMode::Straight
        ));
        assert!(color_matches(
            trans_red,
            trans_blue,
            100,
            AlphaMatchMode::Straight
        ));
    }

    #[test]
    fn recolor_preserves_shading_and_overwrite_replaces_alpha() {
        let target = Pixel([100, 120, 140, 255]);
        let shaded = Pixel([120, 140, 160, 255]);
        let replacement = Pixel([40, 200, 100, 255]);
        let out = recolor_pixel(
            shaded,
            target,
            replacement,
            25,
            AlphaMatchMode::Premultiplied,
        )
        .unwrap();
        assert_eq!(out, Pixel([60, 220, 120, 255]));

        let base = Pixel([255, 0, 0, 255]);
        let half_green = Pixel([0, 255, 0, 128]);
        assert_eq!(
            blend_covered(half_green, base, 255, StrokeBlend::Overwrite),
            half_green
        );
    }
}
