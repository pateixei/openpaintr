use super::i18n::text as tr;
use std::{
    f32::consts::TAU,
    fs, io,
    path::{Path, PathBuf},
};

use eframe::egui::{
    self, Color32, CornerRadius, Mesh, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Vec2,
    ecolor::Hsva, pos2, vec2,
};
use openpaintr::Pixel;

use super::{
    mouse_shortcuts::{BUTTONS, MouseAction, MouseBindings, Scope, Trigger},
    theme::Theme,
    tool_icons::{self, ActionIcon},
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

#[derive(Clone, Debug)]
pub struct ColorPalette {
    pub primary: Pixel,
    pub secondary: Pixel,
    pub primary_hsva: Hsva,
    pub secondary_hsva: Hsva,
    pub editing_secondary: bool,
    pub expanded: bool,
    pub swatches: Vec<[u8; 4]>,
    hex_draft: String,
    hex_last: [u8; 4],
    palette_path: Option<PathBuf>,
}

impl ColorPalette {
    pub fn new(primary: [u8; 3], secondary: [u8; 3]) -> Self {
        let palette_path = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|dir| dir.join(PALETTE_FILE)));
        let loaded = palette_path
            .as_deref()
            .and_then(|p| fs::read_to_string(p).ok())
            .map(|text| Self::parse_palette_txt(&text))
            .filter(|s| !s.is_empty());
        let p_rgba = [primary[0], primary[1], primary[2], 255];
        Self {
            primary: Pixel(p_rgba),
            secondary: Pixel([secondary[0], secondary[1], secondary[2], 255]),
            primary_hsva: Hsva::from_srgb(primary),
            secondary_hsva: Hsva::from_srgb(secondary),
            editing_secondary: false,
            expanded: false,
            swatches: loaded.unwrap_or_else(|| DEFAULT_SWATCHES.to_vec()),
            hex_draft: format_hex(p_rgba),
            hex_last: p_rgba,
            palette_path,
        }
    }

    pub fn swap(&mut self) {
        std::mem::swap(&mut self.primary, &mut self.secondary);
        std::mem::swap(&mut self.primary_hsva, &mut self.secondary_hsva);
    }

    pub fn reset(&mut self) {
        self.primary = Pixel([0, 0, 0, 255]);
        self.primary_hsva = Hsva::from_srgb([0, 0, 0]);
        self.secondary = Pixel([255, 255, 255, 255]);
        self.secondary_hsva = Hsva::from_srgb([255, 255, 255]);
    }

    pub fn set_primary(&mut self, px: Pixel) {
        self.primary = px;
        let mut hsva = Hsva::from_srgb([px.0[0], px.0[1], px.0[2]]);
        hsva.a = f32::from(px.0[3]) / 255.0;
        self.primary_hsva = hsva;
    }

    pub fn set_secondary(&mut self, px: Pixel) {
        self.secondary = px;
        let mut hsva = Hsva::from_srgb([px.0[0], px.0[1], px.0[2]]);
        hsva.a = f32::from(px.0[3]) / 255.0;
        self.secondary_hsva = hsva;
    }

    pub fn set_primary_hue(&mut self, hue: f32) {
        self.editing_secondary = false;
        self.primary_hsva.h = hue.clamp(0.0, 1.0);
        if self.primary_hsva.s < 0.02 {
            self.primary_hsva.s = 1.0;
        }
        if self.primary_hsva.v < 0.02 {
            self.primary_hsva.v = 1.0;
        }
        let srgb = self.primary_hsva.to_srgb();
        self.primary.0[0..3].copy_from_slice(&srgb);
    }

    pub fn active_pixel(&self) -> Pixel {
        if self.editing_secondary {
            self.secondary
        } else {
            self.primary
        }
    }

    pub fn set_active_rgba(&mut self, rgba: [u8; 4]) {
        if self.editing_secondary {
            self.set_secondary(Pixel(rgba));
        } else {
            self.set_primary(Pixel(rgba));
        }
    }

    fn active_mut(&mut self) -> (&mut Pixel, &mut Hsva) {
        if self.editing_secondary {
            (&mut self.secondary, &mut self.secondary_hsva)
        } else {
            (&mut self.primary, &mut self.primary_hsva)
        }
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
        let current = self.active_pixel().0;
        if !self.swatches.contains(&current) {
            self.swatches.push(current);
            self.autosave();
        }
    }

    pub fn parse_palette_txt(content: &str) -> Vec<[u8; 4]> {
        let mut colors = Vec::new();
        for raw in content.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') && line.len() != 7 && line.len() != 9 {
                continue;
            }
            let hex = line.trim_start_matches('#');
            if hex.len() == 8
                && let Ok(v) = u32::from_str_radix(hex, 16)
            {
                let a = ((v >> 24) & 0xff) as u8;
                let r = ((v >> 16) & 0xff) as u8;
                let g = ((v >> 8) & 0xff) as u8;
                let b = (v & 0xff) as u8;
                colors.push([r, g, b, a]);
            } else if hex.len() == 6
                && let Ok(v) = u32::from_str_radix(hex, 16)
            {
                let r = ((v >> 16) & 0xff) as u8;
                let g = ((v >> 8) & 0xff) as u8;
                let b = (v & 0xff) as u8;
                colors.push([r, g, b, 255]);
            }
        }
        colors
    }

    pub fn serialize_palette_txt(&self) -> String {
        let mut out = format!("; {} (AARRGGBB)\n", super::i18n::logical_text("palette-file-comment"));
        for [r, g, b, a] in &self.swatches {
            out.push_str(&format!("{a:02X}{r:02X}{g:02X}{b:02X}\n"));
        }
        out
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        fs::write(path, self.serialize_palette_txt())
    }

    pub fn load_from(&mut self, path: &Path) -> io::Result<()> {
        let text = fs::read_to_string(path)?;
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
    pub bar_rect: Rect,
    pub preview_rect: Rect,
    pub toggle_clicked: bool,
}

pub const RIBBON_CONTENT_HEIGHT: f32 = 24.0;

pub fn ribbon(
    ui: &mut egui::Ui,
    palette: &mut ColorPalette,
    open: bool,
    theme: Theme,
    checker: &egui::TextureHandle,
) -> RibbonOutput {
    ui.set_height(RIBBON_CONTENT_HEIGHT);
    let (preview_rect, bar_rect, chevron_rect, toggle_clicked) = ui
        .horizontal(|ui| {
            ui.set_height(RIBBON_CONTENT_HEIGHT);
            ui.spacing_mut().item_spacing.x = 6.0;

            // Left: 18x18 primary color preview square
            let (preview_slot, preview_resp) =
                ui.allocate_exact_size(vec2(18.0, RIBBON_CONTENT_HEIGHT), Sense::click());
            let preview_rect = Rect::from_center_size(preview_slot.center(), vec2(18.0, 18.0));
            let px = palette.primary;
            let p = ui.painter();
            if px.0[3] < 255 {
                p.image(
                    checker.id(),
                    preview_rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(2.0, 2.0)),
                    Color32::WHITE,
                );
            }
            p.rect_filled(
                preview_rect,
                CornerRadius::same(4),
                Color32::from_rgba_unmultiplied(px.0[0], px.0[1], px.0[2], px.0[3]),
            );
            p.rect_stroke(
                preview_rect,
                CornerRadius::same(4),
                Stroke::new(1.0_f32, theme.border_color()),
                StrokeKind::Outside,
            );
            if preview_resp.clicked() {
                palette.editing_secondary = false;
            }
            preview_resp.on_hover_text(super::i18n::format(
                "primary-color-value",
                &[&format_hex(px.0)],
            ));

            // Middle: fill remaining space before the 26px right Chevron button
            let chevron_w = 26.0_f32;
            let gap = ui.spacing().item_spacing.x;
            let bar_w = (ui.available_width() - chevron_w - gap).max(32.0);
            let (bar_slot, bar_resp) = ui.allocate_exact_size(
                vec2(bar_w, RIBBON_CONTENT_HEIGHT),
                Sense::click_and_drag(),
            );
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
            let steps = 48usize;
            let mut mesh = Mesh::default();
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let x = egui::lerp(strip_rect.left()..=strip_rect.right(), t);
                let color: Color32 = Hsva::new(t, 1.0, 1.0, 1.0).into();
                mesh.colored_vertex(pos2(x, strip_rect.top()), color);
                mesh.colored_vertex(pos2(x, strip_rect.bottom()), color);
            }
            for i in 0..steps {
                let base = (i * 2) as u32;
                mesh.add_triangle(base, base + 1, base + 2);
                mesh.add_triangle(base + 1, base + 3, base + 2);
            }
            p.add(mesh);
            p.rect_stroke(
                bar_rect,
                CornerRadius::same(radius.round() as u8),
                Stroke::new(1.0_f32, theme.border_color()),
                StrokeKind::Outside,
            );

            // Indicator: white-ringed 12px circle aligned to active hue
            let hue = palette.primary_hsva.h.clamp(0.0, 1.0);
            let marker_x = egui::lerp(strip_rect.left()..=strip_rect.right(), hue);
            let marker_pos = pos2(marker_x, bar_rect.center().y);
            let marker_fill: Color32 = Hsva::new(hue, 1.0, 1.0, 1.0).into();
            p.circle_stroke(marker_pos, 6.5, Stroke::new(1.0_f32, Color32::from_black_alpha(110)));
            p.circle_filled(marker_pos, 6.0, marker_fill);
            p.circle_stroke(marker_pos, 6.0, Stroke::new(2.0_f32, Color32::WHITE));

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
            (
                preview_rect,
                bar_rect,
                chevron_resp.rect,
                chevron_resp.clicked(),
            )
        })
        .inner;
    RibbonOutput {
        chevron_rect,
        bar_rect,
        preview_rect,
        toggle_clicked,
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
            let p = ui.painter();
            let c = Color32::from_rgba_unmultiplied(px.0[0], px.0[1], px.0[2], px.0[3]);
            if px.0[3] < 255 {
                p.image(
                    checker.id(),
                    rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(2.0, 2.0)),
                    Color32::WHITE,
                );
            }
            p.rect_filled(rect, CornerRadius::same(4), c);
            p.rect_stroke(
                rect,
                CornerRadius::same(4),
                Stroke::new(
                    if selected { 1.8_f32 } else { 1.0_f32 },
                    if selected {
                        theme.accent()
                    } else {
                        theme.border_color()
                    },
                ),
                StrokeKind::Outside,
            );
        };
        if palette.editing_secondary {
            draw_swatch(prim_rect, palette.primary, false);
            draw_swatch(sec_rect, palette.secondary, true);
        } else {
            draw_swatch(sec_rect, palette.secondary, false);
            draw_swatch(prim_rect, palette.primary, true);
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
            let (pixel, hsva) = palette.active_mut();
            let mut rgb = [pixel.0[0], pixel.0[1], pixel.0[2]];
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                pixel.0[0..3].copy_from_slice(&rgb);
                let prev_a = hsva.a;
                *hsva = Hsva::from_srgb(rgb);
                hsva.a = prev_a;
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
    if let Some(pointer) = response.interact_pointer_pos() {
        let active_btn = ui.ctx().input(|input| {
            BUTTONS
                .into_iter()
                .find(|&btn| input.pointer.button_down(btn) || input.pointer.button_pressed(btn))
        });
        if let Some(btn) = active_btn {
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
                let (pixel, hsva) = if target_secondary {
                    (&mut palette.secondary, &mut palette.secondary_hsva)
                } else {
                    (&mut palette.primary, &mut palette.primary_hsva)
                };
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
                let srgb = hsva.to_srgb();
                pixel.0[0..3].copy_from_slice(&srgb);
            }
        }
    }

    let active_hsva = if palette.editing_secondary {
        palette.secondary_hsva
    } else {
        palette.primary_hsva
    };
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
    let (h, s, v_cur, a_cur) = {
        let (_, hsva) = palette.active_mut();
        (hsva.h, hsva.s, hsva.v, hsva.a)
    };
    if let Some(next_v) = gradient_bar(
        ui,
        theme,
        "color_value_bar",
        vec2(bar_width, 13.0),
        v_cur,
        None,
        |t| Hsva::new(h, s, t, 1.0).into(),
        tr("brightness"),
    ) {
        let (pixel, hsva) = palette.active_mut();
        hsva.v = next_v;
        let srgb = hsva.to_srgb();
        pixel.0[0..3].copy_from_slice(&srgb);
    }

    let full_rgb = Hsva::new(h, s, v_cur, 1.0).to_srgb();
    if let Some(next_a) = gradient_bar(
        ui,
        theme,
        "color_alpha_bar",
        vec2(bar_width, 13.0),
        a_cur,
        Some(checker),
        |t| {
            Color32::from_rgba_unmultiplied(
                full_rgb[0],
                full_rgb[1],
                full_rgb[2],
                (t * 255.0).round() as u8,
            )
        },
        tr("opacity"),
    ) {
        let (pixel, hsva) = palette.active_mut();
        hsva.a = next_a;
        pixel.0[3] = (next_a * 255.0).round() as u8;
    }

    // 4. Expanded numeric RGB / HSV / Hex / Alpha editor
    if palette.expanded {
        let current = palette.active_pixel().0;
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

        let (_, hsva) = palette.active_mut();
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
            let (pixel, hsva) = palette.active_mut();
            hsva.h = (h_deg.rem_euclid(360) as f32) / 360.0;
            hsva.s = (s_pct.clamp(0, 100) as f32) / 100.0;
            hsva.v = (v_pct.clamp(0, 100) as f32) / 100.0;
            let srgb = hsva.to_srgb();
            pixel.0[0..3].copy_from_slice(&srgb);
        }

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(RichText::new("Hex").size(11.0).color(theme.text_muted()));
            let resp = ui.add(
                egui::TextEdit::singleline(&mut palette.hex_draft)
                    .desired_width(68.0),
            );
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
                let (pixel, hsva) = palette.active_mut();
                pixel.0[3] = alpha;
                hsva.a = f32::from(alpha) / 255.0;
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

    let active_rgba = palette.active_pixel().0;
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
                let (rect, resp) = ui.allocate_exact_size(
                    vec2(swatch_size, swatch_size),
                    Sense::click(),
                );
                if rgba[3] < 255 {
                    ui.painter().image(
                        checker.id(),
                        rect,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(2.0, 2.0)),
                        Color32::WHITE,
                    );
                }
                let c = Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
                let selected = active_rgba == rgba;
                ui.painter().rect_filled(rect, CornerRadius::same(4), c);
                ui.painter().rect_stroke(
                    rect,
                    CornerRadius::same(4),
                    Stroke::new(
                        if selected { 1.8_f32 } else { 1.0_f32 },
                        if selected {
                            theme.accent()
                        } else {
                            theme.border_color()
                        },
                    ),
                    StrokeKind::Outside,
                );
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
    let hex = raw.trim().trim_start_matches('#');
    if hex.len() == 6 {
        let v = u32::from_str_radix(hex, 16).ok()?;
        Some([((v >> 16) & 0xff) as u8, ((v >> 8) & 0xff) as u8, (v & 0xff) as u8, 255])
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

#[allow(clippy::too_many_arguments)]
fn gradient_bar(
    ui: &mut egui::Ui,
    theme: Theme,
    id_salt: &str,
    size: Vec2,
    value: f32,
    checker: Option<&egui::TextureHandle>,
    color_at: impl Fn(f32) -> Color32,
    tooltip: &str,
) -> Option<f32> {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let inner = Rect::from_min_max(rect.min + vec2(4.0, 2.0), rect.max - vec2(4.0, 2.0));
    let p = ui.painter();
    if let Some(tex) = checker {
        let tiles_x = (inner.width() / 8.0).max(1.0);
        let tiles_y = (inner.height() / 8.0).max(1.0);
        p.image(
            tex.id(),
            inner,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(tiles_x, tiles_y)),
            Color32::WHITE,
        );
    }
    let segments = 24;
    let mut mesh = Mesh::default();
    for i in 0..=segments {
        let t = i as f32 / segments as f32;
        let x = egui::lerp(inner.left()..=inner.right(), t);
        let c = color_at(t);
        mesh.colored_vertex(Pos2::new(x, inner.top()), c);
        mesh.colored_vertex(Pos2::new(x, inner.bottom()), c);
    }
    for i in 0..segments {
        let base = (i * 2) as u32;
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base + 1, base + 3, base + 2);
    }
    p.add(mesh);
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

    let updated = resp.interact_pointer_pos().map(|pos| {
        ((pos.x - inner.left()) / inner.width().max(1.0)).clamp(0.0, 1.0)
    });
    resp.on_hover_text(tooltip);
    let _ = id_salt;
    updated
}

fn snap_hue(h: f32) -> f32 {
    let step = 1.0 / 24.0;
    ((h / step).round() * step).rem_euclid(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

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
