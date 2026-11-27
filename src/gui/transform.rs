use super::i18n::text as tr;
use eframe::egui::{self, CursorIcon, PointerButton, Vec2, vec2};

use super::{
    clipboard::Image,
    geometry::{Affine, Resampling},
    mouse_shortcuts::{MouseAction, MouseBindings, Scope},
    session::{
        SessionOutcome, handle_response, paint_affine_box, paint_crosshair, resize_cursor,
        set_grab_cursor,
    },
    view::CanvasViewport,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformTarget {
    Mask,
    Pixels,
    Layer,
}

impl TransformTarget {
    pub fn short_label(self) -> &'static str {
        match self {
            Self::Mask => tr("move-selection"),
            Self::Pixels => tr("move-pixels"),
            Self::Layer => tr("transform-layer"),
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

    pub fn with_floating(affine: Affine, bounds: (Vec2, Vec2), floating: Image) -> Self {
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
        viewport: CanvasViewport,
        mouse: &MouseBindings,
        ui: &egui::Ui,
    ) -> SessionOutcome {
        let (min, max) = self.bounds;
        let corners = [min, vec2(max.x, min.y), max, vec2(min.x, max.y)];
        let local_edges = [
            (corners[0] + corners[1]) * 0.5,
            (corners[1] + corners[2]) * 0.5,
            (corners[2] + corners[3]) * 0.5,
            (corners[3] + corners[0]) * 0.5,
        ];
        let points = corners.map(|point| viewport.project(self.affine, point));
        let edges = local_edges.map(|point| viewport.project(self.affine, point));
        let box_center = points[0].lerp(points[2], 0.5);
        let top_dir = edges[0] - box_center;
        let rotation = if top_dir.length_sq() > 1e-4 {
            edges[0] + top_dir.normalized() * 22.0
        } else {
            edges[0] + vec2(0.0, -22.0)
        };
        let (cx, cy) = self.affine.world_center();
        let pivot_screen = viewport.to_screen(vec2(cx as f32, cy as f32));

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
        changed |= self.interact_handle(bg_handle, &bg_response, viewport, mouse);

        // 2. Corner and edge-midpoint scale handles (registered after bg so handles take priority).
        // Each group lists its id tag and the cursor angles for even/odd handles.
        let scale_handles = [
            (
                points,
                "transform_corner",
                Handle::ScaleCorner as fn(usize) -> Handle,
                [45.0, 135.0],
            ),
            (edges, "transform_edge", Handle::ScaleEdge, [90.0, 0.0]),
        ];
        for (positions, tag, make_handle, base_degs) in scale_handles {
            for (i, pos) in positions.into_iter().enumerate() {
                let response = handle_response(ui, pos, 14.0, (tag, i));
                if response.hovered() || response.dragged() {
                    let base_deg = base_degs[i % 2];
                    ui.ctx()
                        .set_cursor_icon(resize_cursor(self.affine.degrees as f32 + base_deg));
                }
                changed |= self.interact_handle(make_handle(i), &response, viewport, mouse);
            }
        }

        // 3. Dedicated top rotation handle.
        let rot_response = handle_response(ui, rotation, 15.0, "transform_rotate");
        set_grab_cursor(ui.ctx(), &rot_response);
        changed |= self.interact_handle(Handle::Rotate, &rot_response, viewport, mouse);

        // 4. Pivot crosshair (only interactive when the transformed box is large enough not to block moving).
        let show_pivot =
            points[0].distance(points[1]) >= 28.0 && points[1].distance(points[2]) >= 28.0;
        if show_pivot || matches!(self.gesture, Some((Handle::Pivot, ..))) {
            let pivot_response = handle_response(ui, pivot_screen, 14.0, "transform_pivot");
            if pivot_response.hovered() || pivot_response.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
            }
            changed |= self.interact_handle(Handle::Pivot, &pivot_response, viewport, mouse);
        }

        // 5. Paint visual overlay.
        let painter = ui.painter();
        paint_affine_box(painter, points, &edges, edges[0], rotation);
        paint_crosshair(painter, pivot_screen, 5.5, 180, 5.5);

        SessionOutcome::from_interaction(bg_response.clicked(), self.gesture.is_none(), changed)
    }

    fn interact_handle(
        &mut self,
        handle: Handle,
        response: &egui::Response,
        viewport: CanvasViewport,
        mouse: &MouseBindings,
    ) -> bool {
        let scope = match handle {
            Handle::Move => Scope::TransformMove,
            Handle::ScaleCorner(_) | Handle::ScaleEdge(_) => Scope::TransformScale,
            Handle::Rotate | Handle::BgRotate => Scope::TransformRotate,
            Handle::Pivot => Scope::TransformMove,
        };
        let resolve_action = || {
            mouse
                .active_in(&response.ctx, scope)
                .map(|(_, action)| action)
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
                viewport.drag_origin(response, position),
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
        let current = viewport.to_world(position);
        let delta = current - origin;
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
                let snap = matches!(live_action, MouseAction::RotateSnap)
                    || response.ctx.input(|input| input.modifiers.shift);
                self.affine = start.rotate_from_drag(
                    (f64::from(origin.x), f64::from(origin.y)),
                    (f64::from(current.x), f64::from(current.y)),
                    snap,
                );
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
        self.affine = start.scale_from_anchor(
            (f64::from(origin.x), f64::from(origin.y)),
            (f64::from(current.x), f64::from(current.y)),
            (f64::from(opp_local.x), f64::from(opp_local.y)),
            scale_x,
            scale_y,
            centered,
            proportional,
            0.01,
        );
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
        let after: Vec<_> = corners
            .iter()
            .map(|&c| transform.affine.forward(c))
            .collect();
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
