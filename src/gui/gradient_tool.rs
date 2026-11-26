use super::i18n::text as tr;
use eframe::egui::{self, Color32, CursorIcon, Rect, Stroke, Vec2, vec2};
use gfx::gradient::{
    GF_ALPHA_MODE, GF_DITHER, GR_NONE, GR_REFLECT, GR_WRAP, GT_CONICAL, GT_DIAMOND, GT_LINEAR,
    GT_LINEAR_REFLECTED, GT_RADIAL, GT_SPIRAL_CCW, GT_SPIRAL_CW, Grad, grad_alpha_span, grad_span,
};
use openpaintr::Pixel;

use super::{
    paint_dashed_line,
    selection::{Point, Selection},
    tolerance::{self, StrokeBlend},
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
pub enum SessionOutcome {
    None,
    Changed,
    Commit,
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
    pub shape: GradientShape,
    pub repeat: GradientRepeat,
    pub channel_mode: GradientChannelMode,
    pub dither: bool,
    pub blend: StrokeBlend,
    dragging: Option<(GradientHandle, Vec2, Vec2, Vec2)>,
}

fn to_argb(Pixel([r, g, b, a]): Pixel) -> u32 {
    (u32::from(a) << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

impl GradientSession {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        start: Vec2,
        end: Vec2,
        primary: Pixel,
        secondary: Pixel,
        swapped: bool,
        shape: GradientShape,
        repeat: GradientRepeat,
        channel_mode: GradientChannelMode,
        dither: bool,
        blend: StrokeBlend,
    ) -> Self {
        Self {
            start,
            end,
            primary,
            secondary,
            swapped,
            shape,
            repeat,
            channel_mode,
            dither,
            blend,
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
        let mut flags = if self.dither { GF_DITHER } else { 0 };
        if self.channel_mode == GradientChannelMode::Transparency {
            flags |= GF_ALPHA_MODE;
        }
        // Evaluate at 4x4 SSAA coordinates so rotated/conical/spiral ramps stay anti-aliased.
        let grad = Grad {
            typ: self.shape.code(),
            repeat: self.repeat.code(),
            flags,
            col_a: to_argb(col_a),
            col_b: to_argb(col_b),
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
                match self.channel_mode {
                    GradientChannelMode::Colors => {
                        grad_span(&mut sub_row, 0, gy, &grad);
                        for (x, acc) in acc_rgba.iter_mut().enumerate() {
                            for sx in 0..4 {
                                let off = (x * 4 + sx) * 4;
                                // gfx uses BGRA in little-endian memory (FMT_ARGB)
                                let b = sub_row[off];
                                let g = sub_row[off + 1];
                                let r = sub_row[off + 2];
                                let a = sub_row[off + 3];
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
                match self.channel_mode {
                    GradientChannelMode::Colors => {
                        let sum_a = acc_rgba[x][3];
                        let fg = if sum_a == 0 {
                            Pixel::default()
                        } else {
                            let avg_a = ((sum_a + 8) / 16).min(255) as u8;
                            Pixel([
                                ((acc_rgba[x][0] + sum_a / 2) / sum_a).min(255) as u8,
                                ((acc_rgba[x][1] + sum_a / 2) / sum_a).min(255) as u8,
                                ((acc_rgba[x][2] + sum_a / 2) / sum_a).min(255) as u8,
                                avg_a,
                            ])
                        };
                        out[y * w + x] = tolerance::blend_covered(fg, base, cov, self.blend);
                    }
                    GradientChannelMode::Transparency => {
                        let ramp = (acc_rgba[x][3] + 8) / 16;
                        let modulated_a = ((u32::from(base.0[3]) * ramp + 127) / 255) as u8;
                        let fg = if modulated_a == 0 {
                            Pixel::default()
                        } else {
                            Pixel([base.0[0], base.0[1], base.0[2], modulated_a])
                        };
                        out[y * w + x] =
                            tolerance::blend_covered(fg, base, cov, StrokeBlend::Overwrite);
                    }
                }
            }
        }
        out
    }

    pub fn handles(&mut self, rect: Rect, zoom: f32, ui: &egui::Ui) -> SessionOutcome {
        let to_screen = |pt: Vec2| rect.min + pt * zoom;
        let to_world = |pos: egui::Pos2| (pos - rect.min) / zoom;
        let start_pos = to_screen(self.start);
        let end_pos = to_screen(self.end);
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

        for (handle, pos, id, radius) in handles {
            let response = ui.interact(
                Rect::from_center_size(pos, Vec2::splat(radius)),
                ui.id().with(id),
                egui::Sense::drag(),
            );
            if response.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            } else if response.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::Grab);
            }
            if response.drag_started()
                && let Some(pointer) = response.interact_pointer_pos()
            {
                let origin = to_world(pointer - response.total_drag_delta().unwrap_or_default());
                self.dragging = Some((handle, self.start, self.end, origin));
            }
            if let Some((active, orig_start, orig_end, origin)) = self.dragging
                && active == handle
            {
                if response.drag_stopped() {
                    self.dragging = None;
                }
                if let Some(pointer) = response.interact_pointer_pos() {
                    let current = to_world(pointer);
                    let constrain = ui.ctx().input(|i| i.modifiers.shift);
                    match handle {
                        GradientHandle::Start => {
                            self.start = snap_if(self.end, current, constrain);
                            changed = true;
                        }
                        GradientHandle::End => {
                            self.end = snap_if(self.start, current, constrain);
                            changed = true;
                        }
                        GradientHandle::Mid => {
                            let delta = current - origin;
                            self.start = orig_start + delta;
                            self.end = orig_end + delta;
                            changed = true;
                        }
                    }
                }
            }
        }

        let painter = ui.painter();
        paint_dashed_line(painter, &[start_pos, end_pos]);
        for (pos, accent) in [
            (mid_pos, Color32::from_gray(220)),
            (start_pos, Color32::WHITE),
            (end_pos, Color32::from_rgb(103, 75, 195)),
        ] {
            painter.circle(pos, 5.5, accent, Stroke::new(1.2_f32, Color32::BLACK));
        }

        if bg_response.clicked() && self.dragging.is_none() && !changed {
            SessionOutcome::Commit
        } else if changed {
            SessionOutcome::Changed
        } else {
            SessionOutcome::None
        }
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
