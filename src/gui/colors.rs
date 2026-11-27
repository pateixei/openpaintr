use super::i18n::text as tr;
use std::{
    f32::consts::TAU,
    io,
    path::{Path, PathBuf},
};

use eframe::egui::{
    self, Color32, CornerRadius, Mesh, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Vec2,
    ecolor::Hsva, pos2, vec2,
};
use openpaintr::Pixel;

use super::{
    mouse_shortcuts::{MouseAction, MouseBindings, Scope, Trigger},
    theme::Theme,
    tool_icons::{self, ActionIcon},
    widgets::{paint_checker, pixel_to_color32},
};

const PALETTE_FILE: &str = "paleta.txt";

const DEFAULT_SWATCHES: [[u8; 4]; 32] = [
    [0, 0, 0, 255],
    [64, 64, 64, 255],
    [128, 128, 128, 255],
    [192, 192, 192, 255],
    [255, 255, 255, 255],
    [255, 0, 0, 255],
    [255, 106, 0, 255],
    [255, 216, 0, 255],
    [182, 255, 0, 255],
    [76, 255, 0, 255],
    [0, 255, 144, 255],
    [0, 255, 255, 255],
    [0, 148, 255, 255],
    [0, 38, 255, 255],
    [103, 75, 195, 255],
    [178, 0, 255, 255],
    [255, 0, 220, 255],
    [255, 0, 110, 255],
    [127, 0, 0, 255],
    [127, 51, 0, 255],
    [127, 106, 0, 255],
    [91, 127, 0, 255],
    [38, 127, 0, 255],
    [0, 127, 70, 255],
    [0, 127, 127, 255],
    [0, 74, 127, 255],
    [0, 19, 127, 255],
    [87, 0, 127, 255],
    [127, 0, 110, 255],
    [127, 0, 55, 255],
    [255, 178, 127, 255],
    [255, 255, 255, 0],
];

const PASTEL_SWATCHES: [[u8; 4]; 16] = [
    [248, 200, 220, 255],
    [255, 218, 185, 255],
    [255, 245, 186, 255],
    [204, 245, 204, 255],
    [186, 236, 247, 255],
    [203, 213, 248, 255],
    [221, 217, 248, 255],
    [242, 206, 247, 255],
    [224, 122, 152, 255],
    [238, 158, 110, 255],
    [228, 198, 95, 255],
    [116, 196, 134, 255],
    [102, 182, 214, 255],
    [122, 146, 222, 255],
    [154, 134, 242, 255],
    [194, 124, 212, 255],
];

const GRAYSCALE_SWATCHES: [[u8; 4]; 16] = [
    [0, 0, 0, 255],
    [17, 17, 17, 255],
    [34, 34, 34, 255],
    [51, 51, 51, 255],
    [68, 68, 68, 255],
    [85, 85, 85, 255],
    [102, 102, 102, 255],
    [119, 119, 119, 255],
    [136, 136, 136, 255],
    [153, 153, 153, 255],
    [170, 170, 170, 255],
    [187, 187, 187, 255],
    [204, 204, 204, 255],
    [221, 221, 221, 255],
    [238, 238, 238, 255],
    [255, 255, 255, 255],
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PalettePreset {
    Default,
    Pastel,
    Grayscale,
}

impl PalettePreset {
    pub const ALL: [Self; 3] = [Self::Default, Self::Pastel, Self::Grayscale];

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => tr("preset-default"),
            Self::Pastel => tr("pastel"),
            Self::Grayscale => tr("grayscale"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct EditableColor {
    pixel: Pixel,
    hsva: Hsva,
}

impl EditableColor {
    fn new(pixel: Pixel) -> Self {
        let [r, g, b, alpha] = pixel.0;
        let mut hsva = Hsva::from_srgb([r, g, b]);
        hsva.a = f32::from(alpha) / 255.0;
        Self { pixel, hsva }
    }

    fn set_alpha(&mut self, alpha: u8) {
        self.pixel.0[3] = alpha;
        self.hsva.a = f32::from(alpha) / 255.0;
    }

    fn update_rgb(&mut self) {
        self.pixel.0[..3].copy_from_slice(&self.hsva.to_srgb());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RibbonSwatch {
    Custom,
    Black,
    White,
}

impl RibbonSwatch {
    const ALL: [Self; 3] = [Self::Custom, Self::Black, Self::White];

    fn label(self) -> &'static str {
        tr(match self {
            Self::Custom => "color-ribbon-custom",
            Self::Black => "color-ribbon-black",
            Self::White => "color-ribbon-white",
        })
    }
}

#[derive(Clone, Debug)]
pub struct ColorPalette {
    primary_swatches: [EditableColor; 3],
    selected_swatch: RibbonSwatch,
    secondary: EditableColor,
    editing_secondary: bool,
    pub expanded: bool,
    pub swatches: Vec<[u8; 4]>,
    hex_draft: String,
    hex_last: [u8; 4],
    palette_path: Option<PathBuf>,
}

/// Largest palette file that is read.
const PALETTE_LIMIT: u64 = 64 * 1024;

fn read_palette_text(path: &Path) -> io::Result<String> {
    let bytes = super::storage::read_capped(path, PALETTE_LIMIT)?;
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

impl ColorPalette {
    pub fn new(primary: [u8; 3], secondary: [u8; 3]) -> Self {
        let palette_path = super::storage::exe_adjacent(PALETTE_FILE).ok();
        Self::with_palette_file(primary, secondary, palette_path)
    }

    fn with_palette_file(
        primary: [u8; 3],
        secondary: [u8; 3],
        mut palette_path: Option<PathBuf>,
    ) -> Self {
        let loaded = match palette_path.as_deref().map(read_palette_text) {
            Some(Ok(text)) => Some(Self::parse_palette_txt(&text)).filter(|s| !s.is_empty()),
            // Never overwrite a file that exists but could not be read.
            Some(Err(error)) if error.kind() != io::ErrorKind::NotFound => {
                palette_path = None;
                None
            }
            _ => None,
        };
        let p_rgba = [primary[0], primary[1], primary[2], 255];
        Self {
            primary_swatches: [
                EditableColor::new(Pixel(p_rgba)),
                EditableColor::new(Pixel([0, 0, 0, 255])),
                EditableColor::new(Pixel([255, 255, 255, 255])),
            ],
            selected_swatch: RibbonSwatch::Custom,
            secondary: EditableColor::new(Pixel([secondary[0], secondary[1], secondary[2], 255])),
            editing_secondary: false,
            expanded: false,
            swatches: loaded.unwrap_or_else(|| DEFAULT_SWATCHES.to_vec()),
            hex_draft: format_hex(p_rgba),
            hex_last: p_rgba,
            palette_path,
        }
    }

    pub fn swap(&mut self) {
        let primary = self.primary_swatches[self.selected_swatch as usize];
        self.primary_swatches[RibbonSwatch::Custom as usize] = self.secondary;
        self.selected_swatch = RibbonSwatch::Custom;
        self.secondary = primary;
    }

    pub fn reset(&mut self) {
        self.set_primary(Pixel([0, 0, 0, 255]));
        self.set_secondary(Pixel([255, 255, 255, 255]));
    }

    pub fn primary(&self) -> Pixel {
        self.primary_swatches[self.selected_swatch as usize].pixel
    }

    pub fn secondary(&self) -> Pixel {
        self.secondary.pixel
    }

    pub fn color(&self, secondary: bool) -> Pixel {
        if secondary {
            self.secondary()
        } else {
            self.primary()
        }
    }

    pub fn pair(&self, swapped: bool) -> (Pixel, Pixel) {
        if swapped {
            (self.secondary(), self.primary())
        } else {
            (self.primary(), self.secondary())
        }
    }

    pub fn set_primary(&mut self, px: Pixel) {
        self.primary_swatches[RibbonSwatch::Custom as usize] = EditableColor::new(px);
        self.selected_swatch = RibbonSwatch::Custom;
    }

    pub fn set_secondary(&mut self, px: Pixel) {
        self.secondary = EditableColor::new(px);
    }

    fn set_primary_hue(&mut self, hue: f32) {
        self.selected_swatch = RibbonSwatch::Custom;
        self.editing_secondary = false;
        let color = &mut self.primary_swatches[RibbonSwatch::Custom as usize];
        color.hsva.h = hue.clamp(0.0, 1.0);
        if color.hsva.s < 0.02 {
            color.hsva.s = 1.0;
        }
        if color.hsva.v < 0.02 {
            color.hsva.v = 1.0;
        }
        color.update_rgb();
    }

    fn active_color(&self) -> EditableColor {
        if self.editing_secondary {
            self.secondary
        } else {
            self.primary_swatches[self.selected_swatch as usize]
        }
    }

    fn set_active_rgba(&mut self, rgba: [u8; 4]) {
        if self.active_color().pixel.0[..3] == rgba[..3] {
            self.set_active_alpha(rgba[3]);
        } else {
            *self.edit_active_color() = EditableColor::new(Pixel(rgba));
        }
    }

    fn set_active_alpha(&mut self, alpha: u8) {
        let color = if self.editing_secondary {
            &mut self.secondary
        } else {
            &mut self.primary_swatches[self.selected_swatch as usize]
        };
        color.set_alpha(alpha);
    }

    fn edit_active_color(&mut self) -> &mut EditableColor {
        if self.editing_secondary {
            return &mut self.secondary;
        }
        let color = self.primary_swatches[self.selected_swatch as usize];
        self.selected_swatch = RibbonSwatch::Custom;
        self.primary_swatches[RibbonSwatch::Custom as usize] = color;
        &mut self.primary_swatches[RibbonSwatch::Custom as usize]
    }

    pub fn apply_preset(&mut self, preset: PalettePreset) {
        self.swatches = match preset {
            PalettePreset::Default => DEFAULT_SWATCHES.to_vec(),
            PalettePreset::Pastel => PASTEL_SWATCHES.to_vec(),
            PalettePreset::Grayscale => GRAYSCALE_SWATCHES.to_vec(),
        };
        self.autosave();
    }

    pub fn add_current_swatch(&mut self) {
        let current = self.active_color().pixel.0;
        if !self.swatches.contains(&current) {
            self.swatches.push(current);
            self.autosave();
        }
    }

    pub fn parse_palette_txt(content: &str) -> Vec<[u8; 4]> {
        content
            .lines()
            .filter_map(|raw| {
                let line = raw.trim();
                if line.is_empty()
                    || line.starts_with(';')
                    || (line.starts_with('#') && line.len() != 7 && line.len() != 9)
                {
                    return None;
                }
                parse_hex(line)
            })
            .collect()
    }

    pub fn serialize_palette_txt(&self) -> String {
        let mut out = format!(
            "; {} (AARRGGBB)\n",
            super::i18n::logical_text("palette-file-comment")
        );
        for [r, g, b, a] in &self.swatches {
            out.push_str(&format!("{a:02X}{r:02X}{g:02X}{b:02X}\n"));
        }
        out
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        super::storage::ensure_parent_dir(path)?;
        super::storage::atomic_write_bytes(path, self.serialize_palette_txt().as_bytes(), true)
    }

    pub fn load_from(&mut self, path: &Path) -> io::Result<()> {
        let text = read_palette_text(path)?;
        let parsed = Self::parse_palette_txt(&text);
        if parsed.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                tr("palette-undefined"),
            ));
        }
        self.swatches = parsed;
        self.autosave();
        Ok(())
    }

    fn autosave(&self) {
        if let Some(path) = &self.palette_path {
            let _ = self.save_to(path);
        }
    }
}

pub enum PaletteAction {
    None,
    SavePalette,
    LoadPalette,
}

pub struct RibbonOutput {
    pub chevron_rect: Rect,
    pub rect: Rect,
    pub toggle_clicked: bool,
}

pub const RIBBON_CONTENT_HEIGHT: f32 = 24.0;

pub fn ribbon(
    ui: &mut egui::Ui,
    palette: &mut ColorPalette,
    open: bool,
    mouse: &MouseBindings,
    theme: Theme,
    checker: &egui::TextureHandle,
) -> RibbonOutput {
    ui.set_height(RIBBON_CONTENT_HEIGHT);
    let row = ui.horizontal(|ui| {
        ui.set_height(RIBBON_CONTENT_HEIGHT);
        ui.spacing_mut().item_spacing.x = 6.0;

        let swatches = RibbonSwatch::ALL.map(|swatch| {
            let (slot, response) =
                ui.allocate_exact_size(vec2(18.0, RIBBON_CONTENT_HEIGHT), Sense::click());
            let action = mouse.click(&response, Scope::ColorRibbonSwatch, false);
            if action == Some(MouseAction::RibbonSwatchSecondary) {
                palette.secondary = palette.primary_swatches[swatch as usize];
            } else if action == Some(MouseAction::RibbonSwatchPrimary)
                || (response.clicked() && !response.clicked_by(egui::PointerButton::Primary))
            {
                palette.selected_swatch = swatch;
                palette.editing_secondary = false;
            }
            (
                swatch,
                Rect::from_center_size(slot.center(), vec2(18.0, 18.0)),
                response,
            )
        });

        let primary = palette.primary();
        let opacity = (u16::from(primary.0[3]) * 100 + 127) / 255;
        if let Some(alpha) = alpha_bar(
            ui,
            theme,
            vec2(108.0, RIBBON_CONTENT_HEIGHT),
            primary,
            checker,
            &super::i18n::format("color-ribbon-opacity", &[&opacity]),
        ) {
            palette.editing_secondary = false;
            palette.set_active_alpha(alpha);
        }

        // Middle: fill remaining space before the 26px right Chevron button
        let chevron_w = 26.0_f32;
        let gap = ui.spacing().item_spacing.x;
        let bar_w = (ui.available_width() - chevron_w - gap).max(32.0);
        let (bar_slot, bar_resp) =
            ui.allocate_exact_size(vec2(bar_w, RIBBON_CONTENT_HEIGHT), Sense::click_and_drag());
        let bar_rect = Rect::from_center_size(bar_slot.center(), vec2(bar_w, 16.0));
        let radius = 8.0_f32.min(bar_rect.width() * 0.25);

        if (bar_resp.hovered() || bar_resp.dragged()) && !bar_resp.is_pointer_button_down_on() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if (bar_resp.clicked() || bar_resp.dragged() || bar_resp.is_pointer_button_down_on())
            && let Some(ptr) = bar_resp.interact_pointer_pos()
        {
            let span = (bar_rect.width() - 2.0 * radius).max(1.0);
            let hue = ((ptr.x - (bar_rect.left() + radius)) / span).clamp(0.0, 1.0);
            palette.set_primary_hue(hue);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        let left_cap_center = pos2(bar_rect.left() + radius, bar_rect.center().y);
        let right_cap_center = pos2(bar_rect.right() - radius, bar_rect.center().y);
        let red: Color32 = Hsva::new(0.0, 1.0, 1.0, 1.0).into();
        let p = ui.painter();
        p.circle_filled(left_cap_center, radius, red);
        p.circle_filled(right_cap_center, radius, red);

        let strip_rect = Rect::from_min_max(
            pos2(left_cap_center.x, bar_rect.top()),
            pos2(right_cap_center.x, bar_rect.bottom()),
        );
        paint_horizontal_gradient_mesh(p, strip_rect, 48, |t| Hsva::new(t, 1.0, 1.0, 1.0).into());
        p.rect_stroke(
            bar_rect,
            CornerRadius::same(radius.round() as u8),
            Stroke::new(1.0_f32, theme.border_color()),
            StrokeKind::Outside,
        );

        // The spectrum edits the custom swatch.
        let custom = palette.primary_swatches[RibbonSwatch::Custom as usize];
        let hue = custom.hsva.h;
        let marker_x = egui::lerp(strip_rect.left()..=strip_rect.right(), hue);
        let marker_pos = pos2(marker_x, bar_rect.center().y);
        let marker_fill: Color32 = Hsva::new(hue, 1.0, 1.0, 1.0).into();
        p.circle_stroke(
            marker_pos,
            6.5,
            Stroke::new(1.0_f32, Color32::from_black_alpha(110)),
        );
        p.circle_filled(marker_pos, 6.0, marker_fill);
        p.circle_stroke(marker_pos, 6.0, Stroke::new(2.0_f32, Color32::WHITE));

        for (swatch, rect, response) in swatches {
            let pixel = palette.primary_swatches[swatch as usize].pixel;
            let selected = swatch == palette.selected_swatch;
            let border = if selected || response.has_focus() {
                Stroke::new(1.5_f32, theme.accent())
            } else {
                Stroke::new(1.0_f32, theme.border_color())
            };
            paint_color_swatch(p, Some(checker), rect, pixel, 4.0, border);
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::SelectableLabel,
                    ui.is_enabled(),
                    selected,
                    swatch.label(),
                )
            });
            response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_ui(|ui| {
                    ui.label(swatch.label());
                    ui.monospace(format_hex(pixel.0));
                });
        }

        // Right: 26x24 Chevron toggle button
        let chevron_resp = tool_icons::icon_button(
            ui,
            theme,
            if open {
                ActionIcon::ChevronUp
            } else {
                ActionIcon::ChevronDown
            },
            true,
            open,
            vec2(chevron_w, RIBBON_CONTENT_HEIGHT),
            if open {
                tr("close-colors")
            } else {
                tr("open-colors")
            },
        );
        (chevron_resp.rect, chevron_resp.clicked())
    });
    RibbonOutput {
        chevron_rect: row.inner.0,
        rect: row.response.rect,
        toggle_clicked: row.inner.1,
    }
}

pub fn picker(
    ui: &mut egui::Ui,
    palette: &mut ColorPalette,
    mouse: &MouseBindings,
    theme: Theme,
    checker: &egui::TextureHandle,
) -> PaletteAction {
    let mut action = PaletteAction::None;

    // 1. Overlapping Primary / Secondary color squares + swap + sRGB + reset + Mais/Menos
    ui.horizontal(|ui| {
        let (box_rect, _) = ui.allocate_exact_size(vec2(38.0, 36.0), Sense::hover());
        let prim_rect = Rect::from_min_size(box_rect.min + vec2(1.0, 1.0), vec2(22.0, 22.0));
        let sec_rect = Rect::from_min_size(box_rect.min + vec2(14.0, 13.0), vec2(22.0, 22.0));
        let prim_resp = ui.interact(prim_rect, ui.id().with("palette_prim"), Sense::click());
        let sec_resp = ui.interact(sec_rect, ui.id().with("palette_sec"), Sense::click());
        if prim_resp.clicked() {
            palette.editing_secondary = false;
        } else if sec_resp.clicked() {
            palette.editing_secondary = true;
        }
        let draw_swatch = |rect: Rect, px: Pixel, selected: bool| {
            let border = Stroke::new(
                if selected { 1.8_f32 } else { 1.0_f32 },
                if selected {
                    theme.accent()
                } else {
                    theme.border_color()
                },
            );
            paint_color_swatch(ui.painter(), Some(checker), rect, px, 4.0, border);
        };
        if palette.editing_secondary {
            draw_swatch(prim_rect, palette.primary(), false);
            draw_swatch(sec_rect, palette.secondary(), true);
        } else {
            draw_swatch(sec_rect, palette.secondary(), false);
            draw_swatch(prim_rect, palette.primary(), true);
        }
        prim_resp.on_hover_text(tr("primary-color"));
        sec_resp.on_hover_text(tr("secondary-color"));

        if tool_icons::icon_button(
            ui,
            theme,
            ActionIcon::Swap,
            true,
            false,
            vec2(24.0, 24.0),
            tr("swap-color-hint"),
        )
        .clicked()
        {
            palette.swap();
        }
        {
            let [r, g, b, alpha] = palette.active_color().pixel.0;
            let mut rgb = [r, g, b];
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                palette.set_active_rgba([rgb[0], rgb[1], rgb[2], alpha]);
            }
        }
        if ui
            .small_button("P/B")
            .on_hover_text(tr("restore-black-white"))
            .clicked()
        {
            palette.reset();
        }
        if ui
            .small_button(if palette.expanded {
                tr("less")
            } else {
                tr("more")
            })
            .on_hover_text(tr("toggle-numeric-color"))
            .clicked()
        {
            palette.expanded = !palette.expanded;
        }
    });

    // 2. Centered HSV Color Wheel
    let radius = 64.0_f32;
    let wheel_size = radius * 2.0 + 4.0;
    let avail_w = ui.available_width().max(wheel_size);
    let (row_rect, _) = ui.allocate_exact_size(vec2(avail_w, wheel_size), Sense::hover());
    let wheel_rect = Rect::from_center_size(row_rect.center(), Vec2::splat(wheel_size));
    let response = ui.interact(
        wheel_rect,
        ui.id().with("color_wheel"),
        Sense::click_and_drag(),
    );
    let center = wheel_rect.center();

    let modifiers = ui.ctx().input(|i| i.modifiers);
    if let Some(pointer) = response.interact_pointer_pos()
        && let Some(btn) = MouseBindings::active_button(ui.ctx())
    {
        let delta = pointer - center;
        let dist = delta.length();
        if dist <= radius + 6.0 {
            let h = (delta.y.atan2(delta.x) / TAU).rem_euclid(1.0);
            let sat = (dist / radius).clamp(0.0, 1.0);
            let action = mouse.at(ui.ctx(), Scope::ColorWheel, Trigger::Button(btn), modifiers);
            let target_secondary = match action {
                Some(MouseAction::ColorSecondary) => true,
                Some(MouseAction::ColorPrimary) => false,
                _ => palette.editing_secondary,
            };
            palette.editing_secondary = target_secondary;
            let color = palette.edit_active_color();
            let hsva = &mut color.hsva;
            match action {
                Some(MouseAction::ColorHue) => {
                    hsva.h = h;
                }
                Some(MouseAction::ColorSaturation) => {
                    hsva.s = sat;
                }
                Some(MouseAction::ColorSnap) => {
                    hsva.h = snap_hue(h);
                    hsva.s = sat;
                }
                Some(MouseAction::ColorSnapHue) => {
                    hsva.h = snap_hue(h);
                }
                _ => {
                    hsva.h = h;
                    hsva.s = sat;
                }
            }
            color.update_rgb();
        }
    }

    let active_hsva = palette.active_color().hsva;
    let steps = 48;
    let mut wheel = Mesh::default();
    wheel.colored_vertex(center, Hsva::new(0.0, 0.0, active_hsva.v, 1.0).into());
    for i in 0..=steps {
        let hue = i as f32 / steps as f32;
        let position = center + Vec2::angled(hue * TAU) * radius;
        wheel.colored_vertex(position, Hsva::new(hue, 1.0, active_hsva.v, 1.0).into());
    }
    for i in 1..=steps {
        wheel.add_triangle(0, i as u32, (i + 1) as u32);
    }
    ui.painter().add(wheel);
    ui.painter()
        .circle_stroke(center, radius, Stroke::new(1.0_f32, theme.border_color()));
    let marker = center + Vec2::angled(active_hsva.h * TAU) * (active_hsva.s * radius);
    ui.painter().circle_filled(marker, 4.5, Color32::WHITE);
    ui.painter()
        .circle_stroke(marker, 4.5, Stroke::new(1.5_f32, theme.text_primary()));

    // 3. Value / Brightness Gradient Bar + Opacity / Alpha Gradient Bar
    let bar_width = ui.available_width().max(80.0);
    let Hsva { h, s, v, .. } = active_hsva;
    if let Some(next_v) = gradient_bar(
        ui,
        theme,
        vec2(bar_width, 13.0),
        v,
        None,
        |t| Hsva::new(h, s, t, 1.0).into(),
        tr("brightness"),
    ) {
        let color = palette.edit_active_color();
        color.hsva.v = next_v;
        color.update_rgb();
    }

    if let Some(alpha) = alpha_bar(
        ui,
        theme,
        vec2(bar_width, 13.0),
        palette.active_color().pixel,
        checker,
        tr("opacity"),
    ) {
        palette.set_active_alpha(alpha);
    }

    // 4. Expanded numeric RGB / HSV / Hex / Alpha editor
    if palette.expanded {
        let current = palette.active_color().pixel.0;
        if current != palette.hex_last {
            palette.hex_draft = format_hex(current);
            palette.hex_last = current;
        }
        let mut rgb = [current[0], current[1], current[2]];
        let mut alpha = current[3];
        let mut rgb_changed = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            ui.label(RichText::new("RGB").size(11.0).color(theme.text_muted()));
            for ch in &mut rgb {
                rgb_changed |= ui
                    .add(egui::DragValue::new(ch).speed(1).range(0..=255))
                    .changed();
            }
        });
        if rgb_changed {
            palette.set_active_rgba([rgb[0], rgb[1], rgb[2], alpha]);
        }

        let hsva = palette.active_color().hsva;
        let mut h_deg = (hsva.h * 360.0).round() as i32;
        let mut s_pct = (hsva.s * 100.0).round() as i32;
        let mut v_pct = (hsva.v * 100.0).round() as i32;
        let mut hsv_changed = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            ui.label(RichText::new("HSV").size(11.0).color(theme.text_muted()));
            hsv_changed |= ui
                .add(egui::DragValue::new(&mut h_deg).range(0..=360).suffix("°"))
                .changed();
            hsv_changed |= ui
                .add(egui::DragValue::new(&mut s_pct).range(0..=100).suffix("%"))
                .changed();
            hsv_changed |= ui
                .add(egui::DragValue::new(&mut v_pct).range(0..=100).suffix("%"))
                .changed();
        });
        if hsv_changed {
            let color = palette.edit_active_color();
            color.hsva.h = (h_deg.rem_euclid(360) as f32) / 360.0;
            color.hsva.s = (s_pct.clamp(0, 100) as f32) / 100.0;
            color.hsva.v = (v_pct.clamp(0, 100) as f32) / 100.0;
            color.update_rgb();
        }

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(RichText::new("Hex").size(11.0).color(theme.text_muted()));
            let resp =
                ui.add(egui::TextEdit::singleline(&mut palette.hex_draft).desired_width(68.0));
            if resp.changed()
                && let Some(parsed) = parse_hex(&palette.hex_draft)
            {
                palette.set_active_rgba(parsed);
                palette.hex_last = parsed;
            }
            ui.label(
                RichText::new(tr("alpha"))
                    .size(11.0)
                    .color(theme.text_muted()),
            );
            if ui
                .add(egui::DragValue::new(&mut alpha).speed(1).range(0..=255))
                .changed()
            {
                palette.set_active_alpha(alpha);
            }
        });
    }

    // 5. Palette swatches grid & management toolbar
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        if ui
            .small_button("+")
            .on_hover_text(tr("add-palette-color"))
            .clicked()
        {
            palette.add_current_swatch();
        }
        ui.menu_button(tr("palette"), |ui| {
            for preset in PalettePreset::ALL {
                if ui.button(preset.label()).clicked() {
                    palette.apply_preset(preset);
                    ui.close();
                }
            }
            ui.separator();
            if ui.button(tr("load-file")).clicked() {
                action = PaletteAction::LoadPalette;
                ui.close();
            }
            if ui.button(tr("save-file")).clicked() {
                action = PaletteAction::SavePalette;
                ui.close();
            }
        });
    });

    let active_rgba = palette.active_color().pixel.0;
    let swatch_size = 17.0_f32;
    let gap = 3.0_f32;
    let cols = ((ui.available_width() + gap) / (swatch_size + gap))
        .floor()
        .max(4.0) as usize;
    let mut remove_idx: Option<usize> = None;
    let mut picked: Option<[u8; 4]> = None;

    for (row_idx, chunk) in palette.swatches.chunks(cols).enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (col_idx, &rgba) in chunk.iter().enumerate() {
                let idx = row_idx * cols + col_idx;
                let (rect, resp) =
                    ui.allocate_exact_size(vec2(swatch_size, swatch_size), Sense::click());
                let selected = active_rgba == rgba;
                let border = Stroke::new(
                    if selected { 1.8_f32 } else { 1.0_f32 },
                    if selected {
                        theme.accent()
                    } else {
                        theme.border_color()
                    },
                );
                paint_color_swatch(ui.painter(), Some(checker), rect, Pixel(rgba), 4.0, border);
                if resp.clicked() {
                    picked = Some(rgba);
                } else if resp.secondary_clicked() && palette.swatches.len() > 1 {
                    remove_idx = Some(idx);
                }
                resp.on_hover_text(tr("palette-swatch-help"));
            }
        });
    }
    if let Some(rgba) = picked {
        palette.set_active_rgba(rgba);
    } else if let Some(idx) = remove_idx {
        palette.swatches.remove(idx);
        palette.autosave();
    }

    action
}

pub fn format_hex([r, g, b, a]: [u8; 4]) -> String {
    if a == 255 {
        format!("#{r:02X}{g:02X}{b:02X}")
    } else {
        format!("#{a:02X}{r:02X}{g:02X}{b:02X}")
    }
}

pub fn parse_hex(raw: &str) -> Option<[u8; 4]> {
    let trimmed = raw.trim();
    let hex = trimmed.strip_prefix('#').unwrap_or(trimmed);
    if hex.len() == 6 {
        let v = u32::from_str_radix(hex, 16).ok()?;
        Some([
            ((v >> 16) & 0xff) as u8,
            ((v >> 8) & 0xff) as u8,
            (v & 0xff) as u8,
            255,
        ])
    } else if hex.len() == 8 {
        let v = u32::from_str_radix(hex, 16).ok()?;
        Some([
            ((v >> 16) & 0xff) as u8,
            ((v >> 8) & 0xff) as u8,
            (v & 0xff) as u8,
            ((v >> 24) & 0xff) as u8,
        ])
    } else {
        None
    }
}

fn alpha_bar(
    ui: &mut egui::Ui,
    theme: Theme,
    size: Vec2,
    pixel: Pixel,
    checker: &egui::TextureHandle,
    tooltip: &str,
) -> Option<u8> {
    let [r, g, b, a] = pixel.0;
    let quantize = |value: f32| (value * 255.0).round() as u8;
    gradient_bar(
        ui,
        theme,
        size,
        f32::from(a) / 255.0,
        Some(checker),
        |t| Color32::from_rgba_unmultiplied(r, g, b, quantize(t)),
        tooltip,
    )
    .map(quantize)
}

fn gradient_bar(
    ui: &mut egui::Ui,
    theme: Theme,
    size: Vec2,
    value: f32,
    checker: Option<&egui::TextureHandle>,
    color_at: impl Fn(f32) -> Color32,
    tooltip: &str,
) -> Option<f32> {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let inner = Rect::from_center_size(rect.center(), vec2(size.x - 8.0, size.y.min(20.0) - 4.0));
    let updated = resp
        .interact_pointer_pos()
        .map(|pos| ((pos.x - inner.left()) / inner.width().max(1.0)).clamp(0.0, 1.0));
    let value = updated.unwrap_or(value);
    let p = ui.painter();
    if let Some(tex) = checker {
        let tiles = vec2(
            (inner.width() / 8.0).max(1.0),
            (inner.height() / 8.0).max(1.0),
        );
        paint_checker(p, tex, inner, tiles);
    }
    paint_horizontal_gradient_mesh(p, inner, 24, color_at);
    p.rect_stroke(
        inner,
        CornerRadius::same(4),
        Stroke::new(1.0_f32, theme.border_color()),
        StrokeKind::Outside,
    );

    let handle_x = egui::lerp(inner.left()..=inner.right(), value.clamp(0.0, 1.0));
    let handle_pos = Pos2::new(handle_x, inner.center().y);
    p.circle_filled(handle_pos, 5.5, Color32::WHITE);
    p.circle_stroke(handle_pos, 5.5, Stroke::new(1.8_f32, theme.accent()));

    resp.widget_info(|| {
        egui::WidgetInfo::slider(ui.is_enabled(), f64::from(value) * 100.0, tooltip)
    });
    resp.on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
        .on_hover_text(tooltip);
    updated
}

pub(super) fn paint_color_swatch(
    painter: &egui::Painter,
    checker: Option<&egui::TextureHandle>,
    rect: Rect,
    pixel: Pixel,
    corner_radius: f32,
    border: Stroke,
) {
    if pixel.0[3] < 255
        && let Some(checker) = checker
    {
        paint_checker(painter, checker, rect, vec2(2.0, 2.0));
    }
    painter.rect_filled(rect, corner_radius, pixel_to_color32(pixel));
    painter.rect_stroke(rect, corner_radius, border, StrokeKind::Outside);
}

fn paint_horizontal_gradient_mesh(
    painter: &egui::Painter,
    rect: Rect,
    steps: usize,
    color_at: impl Fn(f32) -> Color32,
) {
    let mut mesh = Mesh::default();
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let x = egui::lerp(rect.left()..=rect.right(), t);
        let color = color_at(t);
        mesh.colored_vertex(pos2(x, rect.top()), color);
        mesh.colored_vertex(pos2(x, rect.bottom()), color);
    }
    for i in 0..steps {
        let base = (i * 2) as u32;
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base + 1, base + 3, base + 2);
    }
    painter.add(mesh);
}

fn snap_hue(h: f32) -> f32 {
    let step = 1.0 / 24.0;
    ((h / step).round() * step).rem_euclid(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unreadable_palette_falls_back_and_is_left_untouched() -> io::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("palette.txt");
        let oversized = "FF112233\n".repeat(PALETTE_LIMIT as usize / 9 + 1);
        std::fs::write(&path, &oversized)?;

        let mut palette = ColorPalette::with_palette_file([0; 3], [0; 3], Some(path.clone()));
        assert_eq!(palette.swatches, DEFAULT_SWATCHES);
        palette.autosave();
        assert_eq!(std::fs::read_to_string(&path)?, oversized);
        let error = palette.load_from(&path).expect_err("oversized");
        assert_eq!(error.kind(), io::ErrorKind::FileTooLarge);

        // A missing file is still created on the first change.
        let missing = temp.path().join("new.txt");
        let palette = ColorPalette::with_palette_file([0; 3], [0; 3], Some(missing.clone()));
        palette.autosave();
        assert!(missing.is_file());
        Ok(())
    }

    #[test]
    fn hex_round_trip_and_paint_net_palette_txt_work() {
        assert_eq!(parse_hex("#674BC3"), Some([103, 75, 195, 255]));
        assert_eq!(parse_hex("80112233"), Some([0x11, 0x22, 0x33, 0x80]));
        assert_eq!(format_hex([103, 75, 195, 255]), "#674BC3");

        let mut palette = ColorPalette::new([0, 0, 0], [255, 255, 255]);
        let txt = palette.serialize_palette_txt();
        let parsed = ColorPalette::parse_palette_txt(&txt);
        assert_eq!(parsed, palette.swatches);
        palette.apply_preset(PalettePreset::Grayscale);
        assert_eq!(palette.swatches.len(), 16);
    }
}
