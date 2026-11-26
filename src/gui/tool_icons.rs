use eframe::egui::{
    self, Color32, CornerRadius, Painter, Rect, Shape as PaintShape, Stroke, StrokeKind, Vec2,
    pos2, vec2,
};

use super::{Tool, icon_atlas::Icon, selection::Shape, shortcuts::Command, theme::Theme};

#[derive(Clone, Copy)]
pub enum ActionIcon {
    NewDoc,
    OpenFolder,
    SaveDisk,
    ExportPng,
    Undo,
    Redo,
    EyeOpen,
    EyeClosed,
    Plus,
    NewLayer,
    Duplicate,
    Trash,
    MergeDown,
    ArrowUp,
    ArrowDown,
    Gear,
    ChevronUp,
    ChevronDown,
    ChevronRight,
    Swap,
    ZoomOut,
    ZoomIn,
    Move,
}

pub fn paint_logo(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(4), Color32::from_rgb(107, 88, 214));
    let inner = rect.shrink(2.0);
    p.rect_filled(
        Rect::from_min_max(inner.min, pos2(inner.center().x, inner.max.y)),
        CornerRadius {
            nw: 2,
            ne: 0,
            sw: 2,
            se: 0,
        },
        Color32::from_rgb(80, 178, 244),
    );
    p.rect_filled(
        Rect::from_min_max(pos2(inner.center().x, inner.center().y), inner.max),
        CornerRadius {
            nw: 0,
            ne: 0,
            sw: 0,
            se: 2,
        },
        Color32::from_rgb(245, 118, 158),
    );
    p.line_segment(
        [
            pos2(inner.left() + 3.0, inner.bottom() - 3.0),
            pos2(inner.right() - 3.0, inner.top() + 3.0),
        ],
        Stroke::new(2.2_f32, Color32::WHITE),
    );
}

pub fn button(ui: &mut egui::Ui, theme: Theme, tool: Tool, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(32.0, 32.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            tool.label(),
        )
    });
    let fill = if selected {
        theme.pill_active_fill()
    } else if response.hovered() {
        theme.hover_fill()
    } else {
        Color32::TRANSPARENT
    };
    let stroke = if selected {
        Stroke::new(1.0_f32, theme.accent().linear_multiply(0.45))
    } else {
        Stroke::NONE
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(6),
        fill,
        stroke,
        StrokeKind::Inside,
    );
    paint_tool_symbol(
        ui.painter(),
        Rect::from_center_size(rect.center(), vec2(24.0, 24.0)),
        theme == Theme::Dark,
        ui.is_enabled(),
        tool,
    );
    tool_tooltip(response, ui, tool.label())
}

fn tool_tooltip(response: egui::Response, ui: &egui::Ui, label: &str) -> egui::Response {
    if response.hovered() {
        let mut tooltip = egui::Tooltip::for_enabled(&response).gap(5.0);
        tooltip.popup = tooltip.popup.frame(
            egui::Frame::new()
                .fill(ui.visuals().window_fill().gamma_multiply(0.70))
                .corner_radius(5)
                .inner_margin(egui::Margin::symmetric(7, 4)),
        );
        tooltip.show(|ui| {
            ui.label(label);
        });
    }
    response
}

pub fn move_button(ui: &mut egui::Ui, theme: Theme, selected: bool) -> egui::Response {
    let response = icon_button(
        ui,
        theme,
        ActionIcon::Move,
        true,
        selected,
        vec2(32.0, 32.0),
        "",
    );
    tool_tooltip(response, ui, "Mover pixels selecionados")
}

pub fn paint_small_tool(ui: &mut egui::Ui, tool: Tool) {
    let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::hover());
    paint_tool_symbol(
        ui.painter(),
        rect,
        ui.visuals().dark_mode,
        ui.is_enabled(),
        tool,
    );
}

pub fn paint_shape_icon(ui: &mut egui::Ui, kind: super::ShapeKind) {
    use super::ShapeKind;
    let icon = match kind {
        ShapeKind::RoundedRect => Icon::ShapeRoundRect,
        ShapeKind::Ellipse | ShapeKind::CalloutOval => Icon::ShapeEllipse,
        ShapeKind::Pentagon | ShapeKind::Hexagon | ShapeKind::Triangle | ShapeKind::RightTriangle => {
            Icon::ShapePolygon
        }
        ShapeKind::Star4 | ShapeKind::Star5 | ShapeKind::Star6 => Icon::ShapeStar,
        _ => Icon::ShapeRect,
    };
    let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::hover());
    icon.paint(ui.painter(), rect, ui.visuals().dark_mode, ui.is_enabled());
}

pub fn icon_button(
    ui: &mut egui::Ui,
    theme: Theme,
    icon: ActionIcon,
    enabled: bool,
    active: bool,
    size: Vec2,
    tooltip: &str,
) -> egui::Response {
    let enabled = enabled && ui.is_enabled();
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let fill = if enabled && active {
        theme.pill_active_fill()
    } else if enabled && response.hovered() {
        theme.hover_fill()
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(5), fill);
    if let Some(icon) = icon.atlas() {
        let size = rect.width().min(rect.height()).clamp(10.0, 24.0) - 3.0;
        icon.paint(
            ui.painter(),
            Rect::from_center_size(rect.center(), vec2(size, size)),
            theme == Theme::Dark,
            enabled,
        );
    } else {
        let color = if !enabled {
            theme.text_muted().gamma_multiply(0.45)
        } else if active {
            theme.pill_active_fg()
        } else {
            theme.text_primary()
        };
        paint_action_icon(ui.painter(), rect, color, icon);
    }
    if tooltip.is_empty() {
        response
    } else {
        response.on_hover_text(tooltip)
    }
}

fn tool_icon(tool: Tool) -> Option<Icon> {
    Some(match tool {
        Tool::Brush => Icon::Brush,
        Tool::Bucket => Icon::Bucket,
        Tool::Eraser => Icon::Eraser,
        Tool::Eyedropper => Icon::Eyedropper,
        Tool::CloneStamp => Icon::Clone,
        Tool::LineCurve => Icon::Line,
        Tool::DrawShape => Icon::ShapeRect,
        Tool::Text => Icon::Text,
        Tool::Pan => Icon::Hand,
        Tool::Zoom => Icon::Zoom,
        Tool::Select(Shape::Rectangle) => Icon::Rectangle,
        Tool::Select(Shape::Ellipse) => Icon::Ellipse,
        Tool::Lasso => Icon::Lasso,
        Tool::Wand => Icon::Wand,
        Tool::Pencil | Tool::Recolor | Tool::Gradient => return None,
    })
}

fn paint_tool_symbol(painter: &Painter, rect: Rect, dark: bool, enabled: bool, tool: Tool) {
    if let Some(icon) = tool_icon(tool) {
        icon.paint(painter, rect, dark, enabled);
        return;
    }
    let ink = if dark {
        Color32::from_rgb(230, 232, 240)
    } else {
        Color32::from_rgb(44, 46, 58)
    };
    let ink = if enabled {
        ink
    } else {
        ink.gamma_multiply(0.45)
    };
    let s = rect.width().min(rect.height());
    let c = rect.center();
    let stroke = Stroke::new((s * 0.08).max(1.2), ink);
    match tool {
        Tool::Pencil => {
            let p0 = c + vec2(-s * 0.28, s * 0.28);
            let p1 = c + vec2(s * 0.24, -s * 0.24);
            painter.line_segment([p0, p1], Stroke::new((s * 0.14).max(1.8), ink));
            painter.circle_filled(p0 - vec2(s * 0.04, -s * 0.04), (s * 0.06).max(1.0), ink);
        }
        Tool::Recolor => {
            let accent = if enabled {
                Color32::from_rgb(122, 92, 224)
            } else {
                ink
            };
            painter.circle_filled(c + vec2(-s * 0.14, s * 0.08), s * 0.18, ink);
            painter.circle_filled(c + vec2(s * 0.14, -s * 0.08), s * 0.18, accent);
        }
        Tool::Gradient => {
            let r = Rect::from_center_size(c, vec2(s * 0.72, s * 0.62));
            let mut mesh = egui::Mesh::default();
            let i0 = mesh.vertices.len() as u32;
            let c_left = if dark {
                Color32::from_rgb(232, 234, 242)
            } else {
                Color32::from_rgb(38, 40, 52)
            };
            let c_right = Color32::from_rgb(138, 108, 226);
            mesh.colored_vertex(r.left_top(), c_left);
            mesh.colored_vertex(r.right_top(), c_right);
            mesh.colored_vertex(r.right_bottom(), c_right);
            mesh.colored_vertex(r.left_bottom(), c_left);
            mesh.add_triangle(i0, i0 + 1, i0 + 2);
            mesh.add_triangle(i0, i0 + 2, i0 + 3);
            painter.add(PaintShape::mesh(mesh));
            painter.rect_stroke(r, 2.0, stroke, StrokeKind::Inside);
        }
        _ => {}
    }
}

impl ActionIcon {
    fn atlas(self) -> Option<Icon> {
        Some(match self {
            Self::NewDoc => Icon::New,
            Self::OpenFolder => Icon::Open,
            Self::SaveDisk => Icon::Save,
            Self::ExportPng => Icon::Export,
            Self::Undo => Icon::Undo,
            Self::Redo => Icon::Redo,
            Self::NewLayer => Icon::NewLayer,
            Self::Duplicate => Icon::Duplicate,
            Self::Trash => Icon::DeleteLayer,
            Self::MergeDown => Icon::MergeDown,
            Self::ArrowUp => Icon::LayerUp,
            Self::ArrowDown => Icon::LayerDown,
            Self::Gear => Icon::Settings,
            Self::Swap => Icon::Swap,
            Self::ZoomOut => Icon::ZoomOut,
            Self::ZoomIn => Icon::ZoomIn,
            Self::Move => Icon::Move,
            _ => return None,
        })
    }
}

pub fn command_image(ui: &egui::Ui, command: Command) -> Option<egui::Image<'static>> {
    let icon = match command {
        Command::New => Icon::New,
        Command::Open => Icon::Open,
        Command::Save => Icon::Save,
        Command::SaveAs => Icon::SaveAs,
        Command::Export => Icon::Export,
        Command::Undo => Icon::Undo,
        Command::Redo => Icon::Redo,
        Command::Cut => Icon::Cut,
        Command::Copy => Icon::Copy,
        Command::Paste => Icon::Paste,
        Command::Erase => Icon::Delete,
        Command::MovePixels => Icon::Move,
        Command::MoveMask | Command::RotateZoom | Command::FreeTransform => Icon::Transform,
        Command::Crop => Icon::Crop,
        Command::ZoomIn => Icon::ZoomIn,
        Command::ZoomOut => Icon::ZoomOut,
        Command::Fit => Icon::Fit,
        Command::ActualSize => Icon::ActualSize,
        Command::Grid => Icon::Grid,
        Command::Rulers => Icon::Rulers,
        Command::Pan => Icon::Hand,
        Command::Zoom => Icon::Zoom,
        Command::Eyedropper => Icon::Eyedropper,
        Command::CloneStamp => Icon::Clone,
        Command::Text => Icon::Text,
        Command::LineCurve => Icon::Curve,
        Command::Shape => Icon::ShapeStar,
        Command::AddLayer => Icon::NewLayer,
        Command::DeleteLayer => Icon::DeleteLayer,
        Command::DuplicateLayer => Icon::Duplicate,
        Command::MergeLayerDown => Icon::MergeDown,
        Command::MoveLayerUp => Icon::LayerUp,
        Command::MoveLayerDown => Icon::LayerDown,
        Command::LayerProperties | Command::Settings => Icon::Settings,
        Command::SwapColors => Icon::Swap,
        Command::Exit => Icon::Exit,
        _ => return None,
    };
    icon.image(ui, 16.0)
}

fn paint_action_icon(painter: &Painter, rect: Rect, color: Color32, icon: ActionIcon) {
    let c = rect.center();
    let stroke = Stroke::new(1.5_f32, color);
    let thin = Stroke::new(1.3_f32, color);
    match icon {
        ActionIcon::EyeOpen => {
            painter.add(PaintShape::line(
                vec![
                    c + vec2(-7.0, 0.0),
                    c + vec2(-3.5, -3.8),
                    c + vec2(0.0, -4.5),
                    c + vec2(3.5, -3.8),
                    c + vec2(7.0, 0.0),
                    c + vec2(3.5, 3.8),
                    c + vec2(0.0, 4.5),
                    c + vec2(-3.5, 3.8),
                    c + vec2(-7.0, 0.0),
                ],
                thin,
            ));
            painter.circle_filled(c, 2.2, color);
        }
        ActionIcon::EyeClosed => {
            painter.add(PaintShape::line(
                vec![
                    c + vec2(-6.5, 0.0),
                    c + vec2(-3.0, 3.2),
                    c + vec2(0.0, 3.8),
                    c + vec2(3.0, 3.2),
                    c + vec2(6.5, 0.0),
                ],
                thin,
            ));
            painter.line_segment([c + vec2(-5.0, 4.5), c + vec2(5.0, -4.5)], thin);
        }
        ActionIcon::Plus => {
            painter.line_segment([c + vec2(-4.5, 0.0), c + vec2(4.5, 0.0)], stroke);
            painter.line_segment([c + vec2(0.0, -4.5), c + vec2(0.0, 4.5)], stroke);
        }
        ActionIcon::ChevronUp => {
            painter.line_segment([c + vec2(-4.0, 2.0), c + vec2(0.0, -2.2)], thin);
            painter.line_segment([c + vec2(0.0, -2.2), c + vec2(4.0, 2.0)], thin);
        }
        ActionIcon::ChevronDown => {
            painter.line_segment([c + vec2(-4.0, -2.0), c + vec2(0.0, 2.2)], thin);
            painter.line_segment([c + vec2(0.0, 2.2), c + vec2(4.0, -2.0)], thin);
        }
        ActionIcon::ChevronRight => {
            painter.line_segment([c + vec2(-2.0, -4.0), c + vec2(2.2, 0.0)], thin);
            painter.line_segment([c + vec2(2.2, 0.0), c + vec2(-2.0, 4.0)], thin);
        }
        _ => unreachable!("ícone com textura no atlas"),
    }
}
