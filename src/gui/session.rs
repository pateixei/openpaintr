use eframe::egui::{self, Color32, CursorIcon, Painter, Pos2, Rect, Stroke, Vec2, vec2};
use openpaintr::Pixel;

use super::{
    app_settings::AaMode,
    gradient_tool::GradientSession,
    paint_dashed_line,
    selection::{Point, Selection},
    text_tool::{FontCatalog, TextSession},
    transform::{Transform, TransformTarget},
    vector_tools::{LineCurveSession, ShapeSession},
    view::CanvasViewport,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionOutcome {
    None,
    Changed,
    Commit,
}

impl SessionOutcome {
    pub fn from_interaction(bg_clicked: bool, idle: bool, changed: bool) -> Self {
        if bg_clicked && idle && !changed {
            Self::Commit
        } else if changed {
            Self::Changed
        } else {
            Self::None
        }
    }
}

/// State captured when a handle drag begins: the grabbed handle, a snapshot of
/// the session state at that moment, and the world-space drag origin.
#[derive(Clone, Copy, Debug)]
pub struct HandleDrag<H, S> {
    pub handle: H,
    pub start: S,
    pub origin: Vec2,
}

/// Square drag-sensitive hit area centered on a handle.
pub fn handle_response(
    ui: &egui::Ui,
    center: Pos2,
    size: f32,
    id: impl std::hash::Hash,
) -> egui::Response {
    ui.interact(
        Rect::from_center_size(center, Vec2::splat(size)),
        ui.id().with(id),
        egui::Sense::drag(),
    )
}

/// Advances the drag of `handle` for this frame.
///
/// Starts a drag (capturing `snapshot`) when `response` begins one, or only
/// while no other drag is active when `only_when_idle` is set. Clears the drag
/// when it stops. While `handle` is the active handle and the pointer is known,
/// returns the drag state with the pointer's current world position.
pub fn track_handle_drag<H: Copy + PartialEq, S: Copy>(
    drag: &mut Option<HandleDrag<H, S>>,
    handle: H,
    response: &egui::Response,
    viewport: CanvasViewport,
    only_when_idle: bool,
    snapshot: impl FnOnce() -> S,
) -> Option<(HandleDrag<H, S>, Vec2)> {
    if response.drag_started()
        && (!only_when_idle || drag.is_none())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        *drag = Some(HandleDrag {
            handle,
            start: snapshot(),
            origin: viewport.drag_origin(response, pointer),
        });
    }
    let active = drag.filter(|active| active.handle == handle)?;
    if response.drag_stopped() {
        *drag = None;
    }
    let pointer = response.interact_pointer_pos()?;
    Some((active, viewport.to_world(pointer)))
}

pub fn set_grab_cursor(ctx: &egui::Context, response: &egui::Response) {
    if response.dragged() {
        ctx.set_cursor_icon(CursorIcon::Grabbing);
    } else if response.hovered() {
        ctx.set_cursor_icon(CursorIcon::Grab);
    }
}

pub fn resize_cursor(angle_deg: f32) -> CursorIcon {
    match ((angle_deg / 45.0).round() as i32).rem_euclid(4) {
        0 => CursorIcon::ResizeHorizontal,
        1 => CursorIcon::ResizeNwSe,
        2 => CursorIcon::ResizeVertical,
        _ => CursorIcon::ResizeNeSw,
    }
}

pub fn rectangle_outline(rect: Rect) -> [Pos2; 5] {
    [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ]
}

pub fn paint_crosshair(
    painter: &Painter,
    center: Pos2,
    radius: f32,
    fill_alpha: u8,
    arm: f32,
) {
    painter.circle(
        center,
        radius,
        Color32::from_white_alpha(fill_alpha),
        Stroke::new(1.2_f32, Color32::BLACK),
    );
    painter.line_segment(
        [center - vec2(arm, 0.0), center + vec2(arm, 0.0)],
        Stroke::new(1.0_f32, Color32::BLACK),
    );
    painter.line_segment(
        [center - vec2(0.0, arm), center + vec2(0.0, arm)],
        Stroke::new(1.0_f32, Color32::BLACK),
    );
}

pub fn paint_affine_box(
    painter: &Painter,
    corners: [Pos2; 4],
    extra_handles: &[Pos2],
    top_mid: Pos2,
    rotation: Pos2,
) {
    let outline = [corners[0], corners[1], corners[2], corners[3], corners[0]];
    paint_dashed_line(painter, &outline);
    painter.line_segment([top_mid, rotation], Stroke::new(1.0_f32, Color32::WHITE));
    painter.circle(
        rotation,
        5.0,
        Color32::WHITE,
        Stroke::new(1.0_f32, Color32::BLACK),
    );
    for point in corners.into_iter().chain(extra_handles.iter().copied()) {
        painter.rect(
            Rect::from_center_size(point, Vec2::splat(8.0)),
            1,
            Color32::WHITE,
            Stroke::new(1.0_f32, Color32::BLACK),
            egui::StrokeKind::Inside,
        );
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CanvasResizeDrag {
    BottomRight,
    RightEdge,
    BottomEdge,
}

impl CanvasResizeDrag {
    pub fn cursor(self) -> CursorIcon {
        match self {
            Self::RightEdge => CursorIcon::ResizeHorizontal,
            Self::BottomEdge => CursorIcon::ResizeVertical,
            Self::BottomRight => CursorIcon::ResizeNwSe,
        }
    }
}

#[derive(Clone)]
pub struct CanvasResizeSession {
    pub original: Point,
    pub preview_size: Point,
    pub dragging: Option<CanvasResizeDrag>,
    pub prev_ctrl: bool,
}

pub enum ActiveSession {
    Transform(Transform),
    Gradient(GradientSession),
    LineCurve(LineCurveSession),
    Shape(ShapeSession),
    Text(TextSession),
    CanvasResize(CanvasResizeSession),
}

impl ActiveSession {
    pub fn is_transform(&self) -> bool {
        matches!(self, Self::Transform(_))
    }

    pub fn as_transform(&self) -> Option<&Transform> {
        match self {
            Self::Transform(transform) => Some(transform),
            _ => None,
        }
    }

    pub fn as_transform_mut(&mut self) -> Option<&mut Transform> {
        match self {
            Self::Transform(transform) => Some(transform),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&TextSession> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_text_mut(&mut self) -> Option<&mut TextSession> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_canvas_resize(&self) -> Option<&CanvasResizeSession> {
        match self {
            Self::CanvasResize(resize) => Some(resize),
            _ => None,
        }
    }

    pub fn as_canvas_resize_mut(&mut self) -> Option<&mut CanvasResizeSession> {
        match self {
            Self::CanvasResize(resize) => Some(resize),
            _ => None,
        }
    }

    pub fn needs_layer_preview(&self) -> bool {
        match self {
            Self::Transform(t) => {
                t.floating.is_some()
                    || (t.target != TransformTarget::Mask
                        && (!t.affine.is_identity() || t.clone_source))
            }
            Self::Gradient(_) | Self::LineCurve(_) | Self::Shape(_) | Self::Text(_) => true,
            Self::CanvasResize(_) => false,
        }
    }

    pub fn commit_label(&self) -> &'static str {
        match self {
            Self::Gradient(_) => "gradient",
            Self::LineCurve(_) => "line-curve",
            Self::Shape(_) => "shape",
            Self::Text(_) => "text",
            Self::Transform(_) | Self::CanvasResize(_) => "",
        }
    }

    pub fn render_layer_preview(
        &mut self,
        fonts: &mut FontCatalog,
        src: &[Pixel],
        size: Point,
        selection: Option<&Selection>,
        aa_mode: AaMode,
    ) -> Option<Vec<Pixel>> {
        match self {
            Self::Gradient(g) => Some(g.render_preview(src, size, selection)),
            Self::LineCurve(lc) => Some(lc.render_preview(src, size, selection, aa_mode)),
            Self::Shape(s) => Some(s.render_preview(src, size, selection, aa_mode)),
            Self::Text(t) => Some(t.render_preview(fonts, src, size, selection)),
            Self::Transform(_) | Self::CanvasResize(_) => None,
        }
    }

    pub fn stroke_size_target(&mut self) -> Option<&mut u32> {
        match self {
            Self::LineCurve(lc) => Some(&mut lc.params.thickness),
            Self::Shape(s) => Some(&mut s.params.thickness),
            _ => None,
        }
    }

    /// Moves the session's content by a canvas-space offset; returns whether
    /// the session supports nudging.
    pub fn nudge(&mut self, dx: f32, dy: f32) -> bool {
        match self {
            Self::Transform(t) => t.nudge(f64::from(dx), f64::from(dy)),
            Self::Gradient(g) => g.nudge(dx, dy),
            Self::LineCurve(lc) => lc.nudge(dx, dy),
            Self::Shape(s) => s.nudge(dx, dy),
            Self::Text(t) => t.nudge(dx, dy),
            Self::CanvasResize(_) => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::{
        gradient_tool::GradientParams, text_tool::TextParams, vector_tools::{LineCurveParams, ShapeParams},
    };
    use eframe::egui::{PointerButton, pos2};

    const VIEW: CanvasViewport = CanvasViewport {
        rect: Rect::from_min_max(pos2(100.0, 100.0), pos2(900.0, 700.0)),
        zoom: 2.0,
    };

    /// Drives a full press → move → release drag through `handles`, which is
    /// called once per frame inside a frameless central panel.
    fn drag(world_from: Vec2, world_to: Vec2, mut handles: impl FnMut(&egui::Ui)) {
        let ctx = egui::Context::default();
        let (from, to) = (VIEW.to_screen(world_from), VIEW.to_screen(world_to));
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let frames = [
            vec![egui::Event::PointerMoved(from)],
            vec![egui::Event::PointerMoved(from), button(from, true)],
            vec![egui::Event::PointerMoved(from.lerp(to, 0.5))],
            vec![egui::Event::PointerMoved(to)],
            vec![egui::Event::PointerMoved(to), button(to, false)],
            Vec::new(),
        ];
        for events in frames {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 800.0))),
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| handles(ui));
            });
        }
    }

    fn assert_near(actual: Vec2, expected: Vec2) {
        assert!((actual - expected).length() < 0.01, "{actual:?} != {expected:?}");
    }

    #[test]
    fn gradient_end_and_mid_handles_follow_the_pointer() {
        let mut g = GradientSession::new(
            vec2(10.0, 10.0),
            vec2(50.0, 10.0),
            Pixel::default(),
            Pixel::default(),
            false,
            GradientParams::default(),
        );
        drag(vec2(50.0, 10.0), vec2(50.0, 40.0), |ui| {
            g.handles(VIEW, ui);
        });
        assert_near(g.start, vec2(10.0, 10.0));
        assert_near(g.end, vec2(50.0, 40.0));

        let mid = (g.start + g.end) * 0.5;
        drag(mid, mid + vec2(5.0, -3.0), |ui| {
            g.handles(VIEW, ui);
        });
        assert_near(g.start, vec2(15.0, 7.0));
        assert_near(g.end, vec2(55.0, 37.0));
    }

    #[test]
    fn line_curve_point_and_background_drags_move_points() {
        let mut lc = LineCurveSession::new(
            vec2(10.0, 10.0),
            vec2(70.0, 10.0),
            Pixel::default(),
            LineCurveParams::default(),
        );
        let p1 = lc.points[1];
        drag(p1, p1 + vec2(0.0, 20.0), |ui| {
            lc.handles(VIEW, ui);
        });
        assert_near(lc.points[1], p1 + vec2(0.0, 20.0));
        assert_near(lc.points[0], vec2(10.0, 10.0));

        let before = lc.points;
        drag(vec2(200.0, 150.0), vec2(204.0, 156.0), |ui| {
            lc.handles(VIEW, ui);
        });
        for (after, before) in lc.points.into_iter().zip(before) {
            assert_near(after, before + vec2(4.0, 6.0));
        }
    }

    #[test]
    fn shape_background_drag_shifts_and_corner_drag_scales() {
        let mut s = ShapeSession::new(
            vec2(20.0, 20.0),
            vec2(60.0, 50.0),
            Pixel::default(),
            Pixel::default(),
            ShapeParams::default(),
        );
        drag(vec2(200.0, 150.0), vec2(207.0, 153.0), |ui| {
            s.handles(VIEW, ui);
        });
        assert_eq!(s.affine.shift, (7.0, 3.0));
        assert_eq!(s.affine.scale, (1.0, 1.0));

        let corner = vec2(67.0, 53.0);
        drag(corner, corner + vec2(40.0, 30.0), |ui| {
            s.handles(VIEW, ui);
        });
        assert!(s.affine.scale.0 > 1.5 && s.affine.scale.1 > 1.5, "{:?}", s.affine);
    }

    #[test]
    fn text_anchor_drag_moves_the_anchor() {
        let mut t = TextSession::new(vec2(30.0, 30.0), Pixel::default(), TextParams::default());
        drag(vec2(30.0, 30.0), vec2(42.0, 21.0), |ui| {
            t.handles(VIEW, ui);
        });
        assert_near(t.anchor, vec2(42.0, 21.0));
    }
}
