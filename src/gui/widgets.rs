use eframe::egui::{
    self, Color32, ColorImage, CornerRadius, Painter, Rect, Stroke, StrokeKind, TextureHandle,
    TextureId, Vec2, pos2,
};
use openpaintr::{BLEND_MODES, Pixel};

use super::i18n::{self, text as tr};

pub fn pixel_to_color32(Pixel([r, g, b, a]): Pixel) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

pub fn pixels_to_color_image(width: usize, height: usize, pixels: &[Pixel]) -> ColorImage {
    let colors = pixels.iter().copied().map(pixel_to_color32).collect();
    ColorImage::new([width, height], colors)
}

pub fn enum_combo_box<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    combo: egui::ComboBox,
    value: &mut T,
    options: &[T],
    label: impl Fn(T) -> &'static str,
) -> bool {
    let previous = *value;
    combo.selected_text(label(*value)).show_ui(ui, |ui| {
        for &option in options {
            ui.selectable_value(value, option, label(option));
        }
    });
    *value != previous
}

pub fn enum_combo<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    width: Option<f32>,
    value: &mut T,
    options: &[T],
    label: impl Fn(T) -> &'static str,
) -> bool {
    let mut combo = egui::ComboBox::from_id_salt(id);
    if let Some(w) = width {
        combo = combo.width(w);
    }
    enum_combo_box(ui, combo, value, options, label)
}

pub fn blend_mode_combo(ui: &mut egui::Ui, combo: egui::ComboBox, blend: &mut u8) -> bool {
    let previous = *blend;
    combo
        .selected_text(i18n::blend_name(*blend))
        .show_ui(ui, |ui| {
            for mode in 0..BLEND_MODES.len() as u8 {
                ui.selectable_value(blend, mode, i18n::blend_name(mode));
            }
        });
    *blend != previous
}

pub fn opacity_slider(opacity: &mut u8) -> egui::Slider<'_> {
    egui::Slider::new(opacity, 0..=255)
        .custom_formatter(|value, _| format!("{:.0}%", value * 100.0 / 255.0))
        .custom_parser(|text| {
            text.trim_end_matches('%')
                .parse::<f64>()
                .ok()
                .map(|value| value * 255.0 / 100.0)
        })
}

pub fn max_texture_side(ctx: &egui::Context) -> usize {
    ctx.input(|input| input.max_texture_side)
}

pub fn max_canvas_side(ctx: &egui::Context) -> u32 {
    max_texture_side(ctx).clamp(1, 8192) as u32
}

pub fn dimension_inputs(
    ui: &mut egui::Ui,
    width: &mut u32,
    height: &mut u32,
    limit: u32,
    labeled: bool,
) -> (bool, bool) {
    if labeled {
        ui.label(tr("width"));
    }
    let w_changed = ui
        .add(egui::DragValue::new(width).range(1..=limit).suffix(" px"))
        .changed();
    if labeled {
        ui.label(tr("height"));
    } else {
        ui.label("×");
    }
    let h_changed = ui
        .add(egui::DragValue::new(height).range(1..=limit).suffix(" px"))
        .changed();
    (w_changed, h_changed)
}

pub fn paint_checker(painter: &Painter, checker: &TextureHandle, rect: Rect, tiles: Vec2) {
    painter.image(
        checker.id(),
        rect,
        Rect::from_min_max(pos2(0.0, 0.0), pos2(tiles.x, tiles.y)),
        Color32::WHITE,
    );
}

pub fn paint_thumbnail(
    painter: &Painter,
    checker: &TextureHandle,
    texture: TextureId,
    rect: Rect,
    corner_radius: impl Into<CornerRadius>,
    border: Option<Stroke>,
    checker_tile_px: f32,
) {
    paint_checker(painter, checker, rect, rect.size() / checker_tile_px);
    painter.image(
        texture,
        rect,
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    if let Some(stroke) = border {
        painter.rect_stroke(rect, corner_radius, stroke, StrokeKind::Outside);
    }
}
