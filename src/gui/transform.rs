use eframe::egui::{self, Color32, CursorIcon, PointerButton, Rect, Stroke, Vec2, vec2};

use super::{
    clipboard::Image,
    geometry::{Affine, Resampling},
    mouse_shortcuts::{BUTTONS, MouseAction, MouseBindings, Scope, Trigger},
    paint_dashed_line,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformTarget {
    Mask,
    Pixels,
    Layer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformOutcome {
    None,
    Changed,
    Commit,
}

impl TransformTarget {
    pub fn short_label(self) -> &'static str {
        match self {
            Self::Mask => "Mover seleção",
            Self::Pixels => "Mover pixels",
            Self::Layer => "Transformar camada",
        }
    }
}

pub struct Transform {
    pub target: TransformTarget,
    pub affine: Affine,
    pub method: Resampling,
    pub floating: Option<Image>,
    pub clone_source: bool,
    bounds: (Vec2, Vec2),
    gesture: Option<(Handle, Affine, Vec2, MouseAction)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Handle {
    Move,
    ScaleCorner(usize),
    ScaleEdge(usize),
    Rotate,
    BgRotate,
    Pivot,
}

impl Transform {
    pub fn new(target: TransformTarget, affine: Affine, bounds: (Vec2, Vec2)) -> Self {
        Self {
            target,
            affine,
            bounds,
            method: Resampling::Nearest,
            floating: None,
            clone_source: false,
            gesture: None,
        }
    }

    pub fn with_floating(
        affine: Affine,
        bounds: (Vec2, Vec2),
        floating: Image,
    ) -> Self {
        Self {
            target: TransformTarget::Pixels,
            affine,
            bounds,
            method: Resampling::Bilinear,
            floating: Some(floating),
            clone_source: false,
            gesture: None,
        }
    }

    pub fn nudge(&mut self, dx: f64, dy: f64) {
        self.affine.shift.0 += dx;
        self.affine.shift.1 += dy;
    }

    pub fn move_pivot(&mut self, world: Vec2) {
        let world_pt = (f64::from(world.x), f64::from(world.y));
        let new_center = self.affine.inverse(world_pt);
        self.affine.center = new_center;
        self.affine.shift = (world_pt.0 - new_center.0, world_pt.1 - new_center.1);
    }

    pub fn handles(
        &mut self,
        rect: Rect,
        zoom: f32,
        mouse: &MouseBindings,
        ui: &egui::Ui,
    ) -> TransformOutcome {
        let (min, max) = self.bounds;
        let corners = [min, vec2(max.x, min.y), max, vec2(min.x, max.y)];
        let local_edges = [
            (corners[0] + corners[1]) * 0.5,
            (corners[1] + corners[2]) * 0.5,
            (corners[2] + corners[3]) * 0.5,
            (corners[3] + corners[0]) * 0.5,
        ];
        let project = |affine: Affine, point: Vec2| {
            let (x, y) = affine.forward((f64::from(point.x), f64::from(point.y)));
            rect.min + zoom * vec2(x as f32, y as f32)
        };
        let points = corners.map(|point| project(self.affine, point));
        let edges = local_edges.map(|point| project(self.affine, point));
        let box_center = points[0].lerp(points[2], 0.5);
        let top_dir = edges[0] - box_center;
        let rotation = if top_dir.length_sq() > 1e-4 {
            edges[0] + top_dir.normalized() * 22.0
        } else {
            edges[0] + vec2(0.0, -22.0)
        };
        let pivot_screen = rect.min
            + zoom
                * vec2(
                    (self.affine.center.0 + self.affine.shift.0) as f32,
                    (self.affine.center.1 + self.affine.shift.1) as f32,
                );

        let mut changed = false;

        // 1. Canvas-wide direct interaction (background):
        // Left click (no drag) = Commit, Left drag = Move, Right drag = Rotate.
        let bg_response = ui.interact(
            ui.clip_rect(),
            ui.id().with("transform_bg"),
            egui::Sense::click_and_drag(),
        );
        let secondary_active = matches!(self.gesture, Some((Handle::BgRotate, ..)))
            || bg_response.drag_started_by(PointerButton::Secondary);
        let bg_handle = if secondary_active {
            Handle::BgRotate
        } else {
            Handle::Move
        };
        if bg_response.dragged() {
            ui.ctx().set_cursor_icon(if secondary_active {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Move
            });
        } else if bg_response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Move);
        }
        changed |= self.interact_handle(bg_handle, &bg_response, rect, zoom, mouse);

        // 2. Corner and edge-midpoint scale handles (registered after bg so handles take priority).
        for (i, pos) in points.into_iter().enumerate() {
            let handle = Handle::ScaleCorner(i);
            let response = ui.interact(
                Rect::from_center_size(pos, Vec2::splat(14.0)),
                ui.id().with(("transform_corner", i)),
                egui::Sense::drag(),
            );
            if response.hovered() || response.dragged() {
                let base_deg = match i {
                    0 | 2 => 45.0,
                    _ => 135.0,
                };
                ui.ctx()
                    .set_cursor_icon(resize_cursor(self.affine.degrees as f32 + base_deg));
            }
            changed |= self.interact_handle(handle, &response, rect, zoom, mouse);
        }
        for (i, pos) in edges.into_iter().enumerate() {
            let handle = Handle::ScaleEdge(i);
            let response = ui.interact(
                Rect::from_center_size(pos, Vec2::splat(14.0)),
                ui.id().with(("transform_edge", i)),
                egui::Sense::drag(),
            );
            if response.hovered() || response.dragged() {
                let base_deg = match i {
                    0 | 2 => 90.0,
                    _ => 0.0,
                };
                ui.ctx()
                    .set_cursor_icon(resize_cursor(self.affine.degrees as f32 + base_deg));
            }
            changed |= self.interact_handle(handle, &response, rect, zoom, mouse);
        }

        // 3. Dedicated top rotation handle.
        let rot_response = ui.interact(
            Rect::from_center_size(rotation, Vec2::splat(15.0)),
            ui.id().with("transform_rotate"),
            egui::Sense::drag(),
        );
        if rot_response.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if rot_response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }
        changed |= self.interact_handle(Handle::Rotate, &rot_response, rect, zoom, mouse);

        // 4. Pivot crosshair (only interactive when the transformed box is large enough not to block moving).
        let show_pivot = points[0].distance(points[1]) >= 28.0 && points[1].distance(points[2]) >= 28.0;
        if show_pivot || matches!(self.gesture, Some((Handle::Pivot, ..))) {
            let pivot_response = ui.interact(
                Rect::from_center_size(pivot_screen, Vec2::splat(14.0)),
                ui.id().with("transform_pivot"),
                egui::Sense::drag(),
            );
            if pivot_response.hovered() || pivot_response.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
            }
            changed |= self.interact_handle(Handle::Pivot, &pivot_response, rect, zoom, mouse);
        }

        // 5. Paint visual overlay.
        let painter = ui.painter();
        let outline = [points[0], points[1], points[2], points[3], points[0]];
        paint_dashed_line(painter, &outline);
        painter.line_segment([edges[0], rotation], (1.0_f32, Color32::WHITE));
        painter.circle(rotation, 5.0, Color32::WHITE, (1.0_f32, Color32::BLACK));
        for point in points.into_iter().chain(edges) {
            painter.rect(
                Rect::from_center_size(point, Vec2::splat(8.0)),
                1,
                Color32::WHITE,
                (1.0_f32, Color32::BLACK),
                egui::StrokeKind::Inside,
            );
        }
        painter.circle(
            pivot_screen,
            5.5,
            Color32::from_white_alpha(180),
            Stroke::new(1.2_f32, Color32::BLACK),
        );
        painter.line_segment(
            [pivot_screen - vec2(5.5, 0.0), pivot_screen + vec2(5.5, 0.0)],
            Stroke::new(1.0_f32, Color32::BLACK),
        );
        painter.line_segment(
            [pivot_screen - vec2(0.0, 5.5), pivot_screen + vec2(0.0, 5.5)],
            Stroke::new(1.0_f32, Color32::BLACK),
        );

        if bg_response.clicked() && self.gesture.is_none() && !changed {
            TransformOutcome::Commit
        } else if changed {
            TransformOutcome::Changed
        } else {
            TransformOutcome::None
        }
    }

    fn interact_handle(
        &mut self,
        handle: Handle,
        response: &egui::Response,
        rect: Rect,
        zoom: f32,
        mouse: &MouseBindings,
    ) -> bool {
        let convert = |pointer| (pointer - rect.min) / zoom;
        let scope = match handle {
            Handle::Move => Scope::TransformMove,
            Handle::ScaleCorner(_) | Handle::ScaleEdge(_) => Scope::TransformScale,
            Handle::Rotate | Handle::BgRotate => Scope::TransformRotate,
            Handle::Pivot => Scope::TransformMove,
        };
        let resolve_action = || {
            response.ctx.input(|input| {
                for btn in BUTTONS {
                    if (input.pointer.button_down(btn) || input.pointer.button_pressed(btn))
                        && let Some(action) = mouse.resolve(
                            scope,
                            Trigger::Button(btn),
                            input.modifiers,
                            |key| input.key_down(key),
                        )
                    {
                        return Some(action);
                    }
                }
                None
            })
        };

        if response.drag_started()
            && let Some(position) = response.interact_pointer_pos()
        {
            let fallback = match handle {
                Handle::Move | Handle::Pivot => MouseAction::Move,
                Handle::ScaleCorner(_) | Handle::ScaleEdge(_) => MouseAction::Scale,
                Handle::Rotate | Handle::BgRotate => MouseAction::Rotate,
            };
            let action = resolve_action().unwrap_or(fallback);
            if handle != Handle::Pivot
                && matches!(action, MouseAction::MoveCopy | MouseAction::RotateCopy)
                && self.floating.is_none()
            {
                self.clone_source = true;
            }
            self.gesture = Some((
                handle,
                self.affine,
                convert(position - response.total_drag_delta().unwrap_or_default()),
                action,
            ));
        }
        let Some((active, start, origin, initial_action)) = self.gesture else {
            return false;
        };
        if active != handle {
            return false;
        }
        let live_action = resolve_action().unwrap_or(initial_action);
        if response.drag_stopped() {
            self.gesture = None;
        }
        let Some(position) = response.interact_pointer_pos() else {
            return false;
        };
        let current = convert(position);
        let delta = current - origin;
        let center = vec2(
            (start.center.0 + start.shift.0) as f32,
            (start.center.1 + start.shift.1) as f32,
        );
        match handle {
            Handle::Move => {
                self.affine.shift = (
                    start.shift.0 + f64::from(delta.x),
                    start.shift.1 + f64::from(delta.y),
                );
            }
            Handle::Pivot => {
                self.affine = start;
                self.move_pivot(current);
            }
            Handle::Rotate | Handle::BgRotate => {
                let mut deg = f64::from(
                    ((current - center).angle() - (origin - center).angle()).to_degrees(),
                );
                if matches!(live_action, MouseAction::RotateSnap)
                    || response.ctx.input(|input| input.modifiers.shift)
                {
                    deg = (deg / 15.0).round() * 15.0;
                }
                self.affine.degrees = start.degrees + deg;
            }
            Handle::ScaleCorner(corner_idx) => {
                let (min, max) = self.bounds;
                let corners = [min, vec2(max.x, min.y), max, vec2(min.x, max.y)];
                let opp_corner = corners[(corner_idx + 2) % 4];
                self.apply_scale(
                    start,
                    origin,
                    current,
                    opp_corner,
                    true,
                    true,
                    live_action,
                    response.ctx.input(|input| input.modifiers),
                );
            }
            Handle::ScaleEdge(edge_idx) => {
                let (min, max) = self.bounds;
                let corners = [min, vec2(max.x, min.y), max, vec2(min.x, max.y)];
                let edges = [
                    (corners[0] + corners[1]) * 0.5,
                    (corners[1] + corners[2]) * 0.5,
                    (corners[2] + corners[3]) * 0.5,
                    (corners[3] + corners[0]) * 0.5,
                ];
                let opp_edge = edges[(edge_idx + 2) % 4];
                let (scale_x, scale_y) = match edge_idx {
                    0 | 2 => (false, true),
                    _ => (true, false),
                };
                self.apply_scale(
                    start,
                    origin,
                    current,
                    opp_edge,
                    scale_x,
                    scale_y,
                    live_action,
                    response.ctx.input(|input| input.modifiers),
                );
            }
        }
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_scale(
        &mut self,
        start: Affine,
        origin: Vec2,
        current: Vec2,
        opp_local: Vec2,
        scale_x: bool,
        scale_y: bool,
        live_action: MouseAction,
        modifiers: egui::Modifiers,
    ) {
        let centered = matches!(
            live_action,
            MouseAction::ScaleCentered | MouseAction::ScaleBoth
        ) || modifiers.alt;
        let proportional = scale_x
            && scale_y
            && (matches!(
                live_action,
                MouseAction::ScaleProportional | MouseAction::ScaleBoth
            ) || modifiers.shift);

        let opp_world = start.forward((f64::from(opp_local.x), f64::from(opp_local.y)));
        let center = vec2(
            (start.center.0 + start.shift.0) as f32,
            (start.center.1 + start.shift.1) as f32,
        );
        let pivot = if centered {
            center
        } else {
            vec2(opp_world.0 as f32, opp_world.1 as f32)
        };

        let rotate = egui::emath::Rot2::from_angle(-(start.degrees as f32).to_radians());
        let from = rotate * (origin - pivot);
        let to = rotate * (current - pivot);
        let ratio = |from: f32, to: f32| {
            if from.abs() < 0.01 {
                1.0
            } else {
                f64::from(to / from)
            }
        };
        let mut rx = if scale_x { ratio(from.x, to.x) } else { 1.0 };
        let mut ry = if scale_y { ratio(from.y, to.y) } else { 1.0 };
        if proportional {
            if (rx - 1.0).abs() > (ry - 1.0).abs() {
                ry = rx;
            } else {
                rx = ry;
            }
        }
        let new_scale = (
            (start.scale.0 * rx).clamp(0.01, 32.0),
            (start.scale.1 * ry).clamp(0.01, 32.0),
        );
        self.affine.scale = new_scale;
        if centered {
            self.affine.shift = start.shift;
        } else {
            let mut temp = start;
            temp.scale = new_scale;
            let temp_opp = temp.forward((f64::from(opp_local.x), f64::from(opp_local.y)));
            self.affine.shift = (
                start.shift.0 + (opp_world.0 - temp_opp.0),
                start.shift.1 + (opp_world.1 - temp_opp.1),
            );
        }
    }
}

fn resize_cursor(angle_deg: f32) -> CursorIcon {
    match ((angle_deg / 45.0).round() as i32).rem_euclid(4) {
        0 => CursorIcon::ResizeHorizontal,
        1 => CursorIcon::ResizeNwSe,
        2 => CursorIcon::ResizeVertical,
        _ => CursorIcon::ResizeNeSw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_pivot_preserves_projected_corners() {
        let mut affine = Affine::centered((100, 80));
        affine.shift = (12.0, -7.0);
        affine.scale = (1.5, 0.75);
        affine.degrees = 33.0;
        let bounds = (vec2(10.0, 20.0), vec2(70.0, 60.0));
        let corners = [
            (f64::from(bounds.0.x), f64::from(bounds.0.y)),
            (f64::from(bounds.1.x), f64::from(bounds.0.y)),
            (f64::from(bounds.1.x), f64::from(bounds.1.y)),
            (f64::from(bounds.0.x), f64::from(bounds.1.y)),
        ];
        let before: Vec<_> = corners.iter().map(|&c| affine.forward(c)).collect();

        let mut transform = Transform::new(TransformTarget::Pixels, affine, bounds);
        transform.move_pivot(vec2(35.0, 48.0));
        let after: Vec<_> = corners.iter().map(|&c| transform.affine.forward(c)).collect();
        for (a, b) in before.into_iter().zip(after) {
            assert!((a.0 - b.0).abs() < 1e-6);
            assert!((a.1 - b.1).abs() < 1e-6);
        }
    }

    #[test]
    fn nudge_updates_shift() {
        let mut transform = Transform::new(
            TransformTarget::Layer,
            Affine::centered((16, 16)),
            (vec2(0.0, 0.0), vec2(16.0, 16.0)),
        );
        transform.nudge(1.0, 0.0);
        transform.nudge(0.0, 10.0);
        assert_eq!(transform.affine.shift, (1.0, 10.0));
    }
}
