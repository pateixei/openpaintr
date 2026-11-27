use super::i18n::text as tr;
use eframe::egui::{self, Color32, Stroke, Vec2, vec2};
use gfx::gradient::{
    GF_ALPHA_MODE, GF_DITHER, GR_NONE, GR_REFLECT, GR_WRAP, GT_CONICAL, GT_DIAMOND, GT_LINEAR,
    GT_LINEAR_REFLECTED, GT_RADIAL, GT_SPIRAL_CCW, GT_SPIRAL_CW, Grad, grad_alpha_span, grad_span,
};
use openpaintr::Pixel;

use super::{
    paint_dashed_line,
    selection::{Point, Selection},
    session::{HandleDrag, SessionOutcome, handle_response, set_grab_cursor, track_handle_drag},
    tolerance::{self, StrokeBlend},
    view::CanvasViewport,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GradientShape {
    #[default]
    Linear,
    LinearReflected,
    Diamond,
    Radial,
    Conical,
    SpiralCw,
    SpiralCcw,
}

impl GradientShape {
    pub const ALL: [Self; 7] = [
        Self::Linear,
        Self::LinearReflected,
        Self::Diamond,
        Self::Radial,
        Self::Conical,
        Self::SpiralCw,
        Self::SpiralCcw,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => tr("linear"),
            Self::LinearReflected => tr("reflected-linear"),
            Self::Diamond => tr("diamond"),
            Self::Radial => tr("radial"),
            Self::Conical => tr("conical"),
            Self::SpiralCw => tr("spiral-clockwise"),
            Self::SpiralCcw => tr("spiral-counterclockwise"),
        }
    }

    fn code(self) -> u8 {
        match self {
            Self::Linear => GT_LINEAR,
            Self::LinearReflected => GT_LINEAR_REFLECTED,
            Self::Diamond => GT_DIAMOND,
            Self::Radial => GT_RADIAL,
            Self::Conical => GT_CONICAL,
            Self::SpiralCw => GT_SPIRAL_CW,
            Self::SpiralCcw => GT_SPIRAL_CCW,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GradientRepeat {
    #[default]
    None,
    Wrap,
    Reflect,
}

impl GradientRepeat {
    pub const ALL: [Self; 3] = [Self::None, Self::Wrap, Self::Reflect];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => tr("no-repeat"),
            Self::Wrap => tr("repeat"),
            Self::Reflect => tr("reflect"),
        }
    }

    fn code(self) -> u8 {
        match self {
            Self::None => GR_NONE,
            Self::Wrap => GR_WRAP,
            Self::Reflect => GR_REFLECT,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GradientChannelMode {
    #[default]
    Colors,
    Transparency,
}

impl GradientChannelMode {
    pub const ALL: [Self; 2] = [Self::Colors, Self::Transparency];

    pub fn label(self) -> &'static str {
        match self {
            Self::Colors => tr("colors"),
            Self::Transparency => tr("transparency"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GradientParams {
    pub shape: GradientShape,
    pub repeat: GradientRepeat,
    pub channel_mode: GradientChannelMode,
    pub dither: bool,
    pub blend: StrokeBlend,
}

impl Default for GradientParams {
    fn default() -> Self {
        Self {
            shape: GradientShape::Linear,
            repeat: GradientRepeat::None,
            channel_mode: GradientChannelMode::Colors,
            dither: false,
            blend: StrokeBlend::Normal,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GradientHandle {
    Start,
    End,
    Mid,
}

#[derive(Clone)]
pub struct GradientSession {
    pub start: Vec2,
    pub end: Vec2,
    pub primary: Pixel,
    pub secondary: Pixel,
    pub swapped: bool,
    pub params: GradientParams,
    dragging: Option<HandleDrag<GradientHandle, (Vec2, Vec2)>>,
}

impl GradientSession {
    pub fn new(
        start: Vec2,
        end: Vec2,
        primary: Pixel,
        secondary: Pixel,
        swapped: bool,
        params: GradientParams,
    ) -> Self {
        Self {
            start,
            end,
            primary,
            secondary,
            swapped,
            params,
            dragging: None,
        }
    }

    pub fn nudge(&mut self, dx: f32, dy: f32) {
        let delta = vec2(dx, dy);
        self.start += delta;
        self.end += delta;
    }

    pub fn render_preview(
        &self,
        base_layer: &[Pixel],
        size: Point,
        selection: Option<&Selection>,
    ) -> Vec<Pixel> {
        let w = size.0 as usize;
        let h = size.1 as usize;
        let mut out = base_layer.to_vec();
        let (col_a, col_b) = if self.swapped {
            (self.secondary, self.primary)
        } else {
            (self.primary, self.secondary)
        };
        let mut flags = if self.params.dither { GF_DITHER } else { 0 };
        if self.params.channel_mode == GradientChannelMode::Transparency {
            flags |= GF_ALPHA_MODE;
        }
        // Evaluate at 4x4 SSAA coordinates so rotated/conical/spiral ramps stay anti-aliased.
        let grad = Grad {
            typ: self.params.shape.code(),
            repeat: self.params.repeat.code(),
            flags,
            col_a: col_a.to_argb_u32(),
            col_b: col_b.to_argb_u32(),
            sx: f64::from(self.start.x) * 4.0,
            sy: f64::from(self.start.y) * 4.0,
            ex: f64::from(self.end.x) * 4.0,
            ey: f64::from(self.end.y) * 4.0,
        };
        let row_bytes = w * 4 * 4;
        let mut sub_row = vec![0u8; row_bytes];
        let mut alpha_row = vec![0u8; w * 4];
        for y in 0..h {
            let mut acc_rgba = vec![[0u32; 4]; w];
            for sy in 0..4 {
                let gy = (y as i32) * 4 + sy;
                match self.params.channel_mode {
                    GradientChannelMode::Colors => {
                        grad_span(&mut sub_row, 0, gy, &grad);
                        for (x, acc) in acc_rgba.iter_mut().enumerate() {
                            for sx in 0..4 {
                                let off = (x * 4 + sx) * 4;
                                // gfx uses BGRA in little-endian memory (FMT_ARGB)
                                let Pixel([r, g, b, a]) = Pixel::from_bgra(
                                    sub_row[off..off + 4].try_into().expect("4 bytes"),
                                );
                                let a32 = u32::from(a);
                                acc[0] += u32::from(r) * a32;
                                acc[1] += u32::from(g) * a32;
                                acc[2] += u32::from(b) * a32;
                                acc[3] += a32;
                            }
                        }
                    }
                    GradientChannelMode::Transparency => {
                        grad_alpha_span(&mut alpha_row, 0, gy, &grad);
                        for (x, acc) in acc_rgba.iter_mut().enumerate() {
                            for sx in 0..4 {
                                acc[3] += u32::from(alpha_row[x * 4 + sx]);
                            }
                        }
                    }
                }
            }
            for x in 0..w {
                let cov = selection.map_or(255, |sel| sel.coverage_u8((x as u32, y as u32)));
                if cov == 0 {
                    continue;
                }
                let base = base_layer[y * w + x];
                match self.params.channel_mode {
                    GradientChannelMode::Colors => {
                        let sum_a = acc_rgba[x][3];
                        let fg = if sum_a == 0 {
                            Pixel::default()
                        } else {
                            let avg_a = super::stroke_raster::avg16_u8(sum_a);
                            Pixel([
                                ((acc_rgba[x][0] + sum_a / 2) / sum_a).min(255) as u8,
                                ((acc_rgba[x][1] + sum_a / 2) / sum_a).min(255) as u8,
                                ((acc_rgba[x][2] + sum_a / 2) / sum_a).min(255) as u8,
                                avg_a,
                            ])
                        };
                        out[y * w + x] = tolerance::blend_covered(fg, base, cov, self.params.blend);
                    }
                    GradientChannelMode::Transparency => {
                        let ramp = super::stroke_raster::avg16_u8(acc_rgba[x][3]);
                        let fg = base.scale_alpha(ramp);
                        out[y * w + x] =
                            tolerance::blend_covered(fg, base, cov, StrokeBlend::Overwrite);
                    }
                }
            }
        }
        out
    }

    pub fn handles(&mut self, viewport: CanvasViewport, ui: &egui::Ui) -> SessionOutcome {
        let start_pos = viewport.to_screen(self.start);
        let end_pos = viewport.to_screen(self.end);
        let mid_pos = start_pos.lerp(end_pos, 0.5);

        let bg_response = ui.interact(
            ui.clip_rect(),
            ui.id().with("gradient_bg"),
            egui::Sense::click_and_drag(),
        );

        let mut changed = false;
        let handles = [
            (GradientHandle::Mid, mid_pos, "gradient_mid", 14.0),
            (GradientHandle::Start, start_pos, "gradient_start", 16.0),
            (GradientHandle::End, end_pos, "gradient_end", 16.0),
        ];

        for (handle, pos, id, size) in handles {
            let response = handle_response(ui, pos, size, id);
            set_grab_cursor(ui.ctx(), &response);
            let Some((drag, current)) = track_handle_drag(
                &mut self.dragging,
                handle,
                &response,
                viewport,
                false,
                || (self.start, self.end),
            ) else {
                continue;
            };
            let constrain = ui.ctx().input(|i| i.modifiers.shift);
            match handle {
                GradientHandle::Start => self.start = snap_if(self.end, current, constrain),
                GradientHandle::End => self.end = snap_if(self.start, current, constrain),
                GradientHandle::Mid => {
                    let (orig_start, orig_end) = drag.start;
                    let delta = current - drag.origin;
                    self.start = orig_start + delta;
                    self.end = orig_end + delta;
                }
            }
            changed = true;
        }

        let painter = ui.painter();
        let accent = ui.visuals().selection.stroke.color;
        paint_dashed_line(painter, &[start_pos, end_pos]);
        for (pos, fill) in [
            (mid_pos, Color32::from_gray(220)),
            (start_pos, Color32::WHITE),
            (end_pos, accent),
        ] {
            painter.circle(pos, 5.5, fill, Stroke::new(1.2_f32, Color32::BLACK));
        }

        SessionOutcome::from_interaction(bg_response.clicked(), self.dragging.is_none(), changed)
    }
}

pub fn snap_if(anchor: Vec2, end: Vec2, constrain: bool) -> Vec2 {
    if !constrain {
        return end;
    }
    let delta = end - anchor;
    let len = delta.length();
    if len < 1e-4 {
        return end;
    }
    let angle = delta.y.atan2(delta.x);
    let step = std::f32::consts::FRAC_PI_4 / 3.0; // 15 deg
    let snapped = (angle / step).round() * step;
    anchor + Vec2::angled(snapped) * len
}
