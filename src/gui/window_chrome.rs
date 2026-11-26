use eframe::egui::{
    self, CornerRadius, CursorIcon, LayerId, Order, PointerButton, Rect, ResizeDirection, Sense,
    Stroke, StrokeKind, ViewportCommand, pos2, vec2,
};

use super::{
    i18n::text as tr,
    theme::Theme,
    tool_icons::{ActionIcon, icon_button},
};

pub const WINDOW_CORNER_RADIUS: u8 = 4;

pub fn corner_radius(ctx: &egui::Context, fullscreen: bool) -> u8 {
    if fullscreen || ctx.input(|input| input.viewport().maximized.unwrap_or(false)) {
        0
    } else {
        WINDOW_CORNER_RADIUS
    }
}

pub fn window_border(ctx: &egui::Context, theme: Theme, fullscreen: bool) {
    let radius = corner_radius(ctx, fullscreen);
    if radius == 0 {
        return;
    }
    let painter = ctx.layer_painter(LayerId::new(Order::Foreground, egui::Id::new("window_border")));
    painter.rect_stroke(
        ctx.content_rect(),
        CornerRadius::same(radius),
        Stroke::new(1.0_f32, theme.border_color()),
        StrokeKind::Inside,
    );
}

fn caption_button(
    ui: &mut egui::Ui,
    theme: Theme,
    icon: ActionIcon,
    label: &str,
    selected: Option<bool>,
) -> bool {
    let size = if selected.is_some() {
        vec2(26.0, 26.0)
    } else {
        vec2(32.0, 26.0)
    };
    let response = icon_button(
        ui,
        theme,
        icon,
        true,
        selected.unwrap_or(false),
        size,
        label,
    );
    response.widget_info(|| match selected {
        Some(selected) => {
            egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label)
        }
        None => egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label),
    });
    response.clicked()
}

pub fn controls(ui: &mut egui::Ui, theme: &mut Theme, fullscreen: bool) -> bool {
    let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
    let close = caption_button(
        ui,
        *theme,
        ActionIcon::CloseWindow,
        tr("window-close"),
        None,
    );
    let (size_icon, size_label) = if fullscreen || maximized {
        (ActionIcon::RestoreWindow, "window-restore")
    } else {
        (ActionIcon::MaximizeWindow, "window-maximize")
    };
    if caption_button(ui, *theme, size_icon, tr(size_label), None) {
        if fullscreen {
            ui.ctx()
                .send_viewport_cmd(ViewportCommand::Fullscreen(false));
        } else {
            ui.ctx()
                .send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        }
    }
    if caption_button(
        ui,
        *theme,
        ActionIcon::MinimizeWindow,
        tr("window-minimize"),
        None,
    ) {
        ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
    }

    ui.add_space(4.0);
    let previous = *theme;
    for (mode, icon, label) in [
        (Theme::Dark, ActionIcon::Moon, "dark"),
        (Theme::Light, ActionIcon::Sun, "light"),
    ] {
        if caption_button(ui, previous, icon, tr(label), Some(previous == mode)) {
            *theme = mode;
        }
    }
    if previous != *theme {
        theme.apply(ui.ctx());
        ui.ctx().request_repaint();
    }
    close
}

pub fn drag_space(ui: &mut egui::Ui, fullscreen: bool) {
    let width = ui.available_width();
    if fullscreen || width <= 0.0 {
        return;
    }
    let (_, response) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click_and_drag());
    if response.double_clicked_by(PointerButton::Primary) {
        let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    } else if response.drag_started_by(PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
}

pub fn resize_edges(ctx: &egui::Context, fullscreen: bool) {
    if fullscreen || ctx.input(|input| input.viewport().maximized.unwrap_or(false)) {
        return;
    }

    // Reserve a narrow hit region along each edge, with wider targets for corners.
    let viewport = ctx.content_rect();
    let x = viewport.x_range();
    let y = viewport.y_range();
    let (edge, corner) = (4.0, 12.0);
    let left = pos2(x.min + corner, y.min + corner);
    let right = pos2(x.max - corner, y.max - corner);
    for (direction, cursor, min, max) in [
        (
            ResizeDirection::North,
            CursorIcon::ResizeNorth,
            pos2(left.x, y.min),
            pos2(right.x, y.min + edge),
        ),
        (
            ResizeDirection::South,
            CursorIcon::ResizeSouth,
            pos2(left.x, y.max - edge),
            pos2(right.x, y.max),
        ),
        (
            ResizeDirection::West,
            CursorIcon::ResizeWest,
            pos2(x.min, left.y),
            pos2(x.min + edge, right.y),
        ),
        (
            ResizeDirection::East,
            CursorIcon::ResizeEast,
            pos2(x.max - edge, left.y),
            pos2(x.max, right.y),
        ),
        (
            ResizeDirection::NorthWest,
            CursorIcon::ResizeNorthWest,
            viewport.min,
            left,
        ),
        (
            ResizeDirection::NorthEast,
            CursorIcon::ResizeNorthEast,
            pos2(right.x, y.min),
            pos2(x.max, left.y),
        ),
        (
            ResizeDirection::SouthWest,
            CursorIcon::ResizeSouthWest,
            pos2(x.min, right.y),
            pos2(left.x, y.max),
        ),
        (
            ResizeDirection::SouthEast,
            CursorIcon::ResizeSouthEast,
            right,
            viewport.max,
        ),
    ] {
        let rect = Rect::from_min_max(min, max);
        let response = egui::Area::new(egui::Id::new(("window_resize", direction as u8)))
            .order(Order::Foreground)
            .fixed_pos(rect.min)
            .default_size(rect.size())
            .sense(Sense::drag())
            .fade_in(false)
            .show(ctx, |ui| ui.set_min_size(rect.size()))
            .response
            .on_hover_and_drag_cursor(cursor);
        if response.drag_started_by(PointerButton::Primary) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
        }
    }
}
