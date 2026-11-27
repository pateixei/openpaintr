use super::i18n::text as tr;
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};

use eframe::egui::{
    self, Align, Color32, CursorIcon, Rect, Stroke, Vec2,
    epaint::{
        FontFamily, FontId,
        text::{FontData, Fonts, Galley, LayoutJob, LayoutSection, TextFormat},
    },
    pos2, vec2,
};
use openpaintr::Pixel;

use super::{
    gradient_tool::SessionOutcome,
    i18n, paint_dashed_line,
    selection::{Point, Selection},
    tolerance::{self, StrokeBlend},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

impl TextAlign {
    pub const ALL: [Self; 3] = [Self::Left, Self::Center, Self::Right];

    pub fn label(self) -> &'static str {
        match self {
            Self::Left => tr("left-align"),
            Self::Center => tr("center-align"),
            Self::Right => tr("right-align"),
        }
    }

    fn egui(self) -> Align {
        match self {
            Self::Left => Align::LEFT,
            Self::Center => Align::Center,
            Self::Right => Align::RIGHT,
        }
    }
}

pub struct FontCatalog {
    pub families: Vec<String>,
    fonts: Fonts,
}

pub fn family_label(family: &str) -> &str {
    match family {
        "Padrão (Sans)" => tr("default-sans"),
        "Monoespaçada" => tr("monospace"),
        name => name,
    }
}

impl Default for FontCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl FontCatalog {
    pub fn new() -> Self {
        Self::for_locale(i18n::Locale::default())
    }

    pub fn for_locale(locale: i18n::Locale) -> Self {
        let mut defs = i18n::fonts(locale);
        let mut families = vec!["Padrão (Sans)".to_owned(), "Monoespaçada".to_owned()];
        let candidates = [
            (
                "DejaVu Sans",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            ),
            (
                "DejaVu Serif",
                "/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf",
            ),
            (
                "Liberation Sans",
                "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            ),
            ("Ubuntu", "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf"),
            ("Arial", "C:\\Windows\\Fonts\\arial.ttf"),
            ("Times New Roman", "C:\\Windows\\Fonts\\times.ttf"),
            ("Courier New", "C:\\Windows\\Fonts\\cour.ttf"),
        ];
        for (name, path_str) in candidates {
            let path = Path::new(path_str);
            if path.is_file()
                && let Ok(bytes) = fs::read(path)
            {
                defs.font_data
                    .insert(name.to_owned(), Arc::new(FontData::from_owned(bytes)));
                let mut family = vec![name.to_owned()];
                family.extend(defs.families[&FontFamily::Proportional].iter().cloned());
                defs.families
                    .insert(FontFamily::Name(Arc::from(name)), family);
                families.push(name.to_owned());
            }
        }
        let fonts = Fonts::new(4096, Default::default(), defs);
        Self { families, fonts }
    }

    fn font_family(&self, name: &str) -> FontFamily {
        match name {
            "" | "Padrão (Sans)" => FontFamily::Proportional,
            "Monoespaçada" => FontFamily::Monospace,
            other if self.families.iter().any(|f| f == other) => FontFamily::Name(Arc::from(other)),
            _ => FontFamily::Proportional,
        }
    }

    fn layout_4x(&mut self, session: &TextSession) -> (Arc<Galley>, egui::ColorImage) {
        self.fonts.begin_pass(4096, Default::default());
        let text = if session.text.is_empty() {
            " ".to_owned()
        } else {
            i18n::display(&session.text).into_owned()
        };
        let color = Color32::WHITE;
        let line_w = (session.size_px * 0.075).max(1.0);
        let format = TextFormat {
            font_id: FontId::new(session.size_px, self.font_family(&session.family)),
            color,
            italics: session.italic,
            underline: if session.underline {
                Stroke::new(line_w, color)
            } else {
                Stroke::NONE
            },
            strikethrough: if session.strikethrough {
                Stroke::new(line_w, color)
            } else {
                Stroke::NONE
            },
            ..Default::default()
        };
        let job = LayoutJob {
            text: text.clone(),
            sections: vec![LayoutSection {
                leading_space: 0.0,
                byte_range: 0..text.len(),
                format,
            }],
            halign: session.align.egui(),
            break_on_newline: true,
            ..Default::default()
        };
        let galley = {
            let mut view = self.fonts.with_pixels_per_point(4.0);
            view.layout_job(job)
        };
        let image = self.fonts.image();
        (galley, image)
    }
}

#[derive(Clone)]
pub struct TextSession {
    pub anchor: Vec2,
    pub text: String,
    pub family: String,
    pub size_px: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub align: TextAlign,
    pub color: Pixel,
    pub blend: StrokeBlend,
    pub last_bounds: Vec2,
    dragging_anchor: Option<(Vec2, Vec2)>,
}

impl TextSession {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        anchor: Vec2,
        family: String,
        size_px: f32,
        bold: bool,
        italic: bool,
        underline: bool,
        strikethrough: bool,
        align: TextAlign,
        color: Pixel,
        blend: StrokeBlend,
    ) -> Self {
        Self {
            anchor,
            text: String::new(),
            family,
            size_px: size_px.clamp(6.0, 240.0),
            bold,
            italic,
            underline,
            strikethrough,
            align,
            color,
            blend,
            last_bounds: vec2(24.0, size_px.max(12.0)),
            dragging_anchor: None,
        }
    }

    pub fn nudge(&mut self, dx: f32, dy: f32) {
        self.anchor += vec2(dx, dy);
    }

    pub fn render_preview(
        &mut self,
        catalog: &mut FontCatalog,
        base_layer: &[Pixel],
        size: Point,
        selection: Option<&Selection>,
    ) -> Vec<Pixel> {
        let mut out = base_layer.to_vec();
        if self.text.is_empty() || size.0 == 0 || size.1 == 0 {
            self.last_bounds = vec2(12.0, self.size_px.max(12.0));
            return out;
        }
        let (galley, atlas) = catalog.layout_4x(self);
        self.last_bounds = galley.size().max(vec2(12.0, self.size_px.max(12.0)));
        if atlas.width() == 0 || atlas.height() == 0 {
            return out;
        }

        // Accumulate 4x4 subpixel alpha coverage in a sparse row-major pixel map.
        let mut cov_map: BTreeMap<Point, [u8; 16]> = BTreeMap::new();
        let bold_dx = if self.bold {
            (self.size_px * 0.045).clamp(0.45, 2.2)
        } else {
            0.0
        };
        let atlas_w = atlas.width() as i32;
        let atlas_h = atlas.height() as i32;
        let sample_atlas = |u: f32, v: f32| -> u8 {
            let x = u.floor() as i32;
            let y = v.floor() as i32;
            if x < 0 || y < 0 || x >= atlas_w || y >= atlas_h {
                return 0;
            }
            let x1 = (x + 1).min(atlas_w - 1);
            let y1 = (y + 1).min(atlas_h - 1);
            let fx = u - x as f32;
            let fy = v - y as f32;
            let a_at = |px: i32, py: i32| {
                f32::from(atlas.pixels[py as usize * atlas_w as usize + px as usize].a())
            };
            let top = a_at(x, y) * (1.0 - fx) + a_at(x1, y) * fx;
            let bot = a_at(x, y1) * (1.0 - fx) + a_at(x1, y1) * fx;
            (top * (1.0 - fy) + bot * fy).round().clamp(0.0, 255.0) as u8
        };

        for row in &galley.rows {
            if row.visuals.mesh.is_empty() {
                continue;
            }
            let row_pos = self.anchor + row.pos.to_vec2();
            let mesh = &row.visuals.mesh;
            for tri in mesh.indices.as_chunks::<3>().0 {
                let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
                let is_glyph = row.visuals.glyph_vertex_range.contains(&i0);
                let (v0, v1, v2) = (mesh.vertices[i0], mesh.vertices[i1], mesh.vertices[i2]);
                let p0 = row_pos + v0.pos.to_vec2();
                let p1 = row_pos + v1.pos.to_vec2();
                let p2 = row_pos + v2.pos.to_vec2();

                let min_x = (p0.x.min(p1.x).min(p2.x).floor() as i32).max(0);
                let min_y = (p0.y.min(p1.y).min(p2.y).floor() as i32).max(0);
                let max_x =
                    ((p0.x.max(p1.x).max(p2.x) + bold_dx).ceil() as i32).min(size.0 as i32 - 1);
                let max_y = (p0.y.max(p1.y).max(p2.y).ceil() as i32).min(size.1 as i32 - 1);
                if min_x > max_x || min_y > max_y {
                    continue;
                }

                let area = edge_fn(p0, p1, p2);
                if area.abs() < 1e-6 {
                    continue;
                }
                let inv_area = 1.0 / area;

                let eval_sample = |px: f32, py: f32| -> u8 {
                    let pt = vec2(px, py);
                    let w0 = edge_fn(p1, p2, pt) * inv_area;
                    let w1 = edge_fn(p2, p0, pt) * inv_area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                        return 0;
                    }
                    if !is_glyph {
                        return 255;
                    }
                    let u = w0 * v0.uv.x + w1 * v1.uv.x + w2 * v2.uv.x;
                    let v = w0 * v0.uv.y + w1 * v1.uv.y + w2 * v2.uv.y;
                    sample_atlas(u, v)
                };

                for py in min_y..=max_y {
                    for px in min_x..=max_x {
                        for sy in 0..4 {
                            let y_sub = py as f32 + (sy as f32 + 0.5) / 4.0;
                            for sx in 0..4 {
                                let x_sub = px as f32 + (sx as f32 + 0.5) / 4.0;
                                let mut a = eval_sample(x_sub, y_sub);
                                if bold_dx > 0.0 {
                                    a = a.max(eval_sample(x_sub - bold_dx, y_sub));
                                }
                                if a > 0 {
                                    let slot =
                                        cov_map.entry((px as u32, py as u32)).or_insert([0; 16]);
                                    let idx = (sy * 4 + sx) as usize;
                                    slot[idx] = slot[idx].max(a);
                                }
                            }
                        }
                    }
                }
            }
        }

        let w = size.0 as usize;
        for (pt @ (x, y), subs) in cov_map {
            let sel_cov = selection.map_or(255, |sel| sel.coverage_u8(pt));
            if sel_cov == 0 {
                continue;
            }
            let sum: u32 = subs.into_iter().map(u32::from).sum();
            let ssaa_alpha = ((sum + 8) / 16).min(255) as u8;
            let eff = ((u32::from(ssaa_alpha) * u32::from(sel_cov) + 127) / 255) as u8;
            if eff == 0 {
                continue;
            }
            let idx = y as usize * w + x as usize;
            out[idx] = tolerance::blend_covered(self.color, base_layer[idx], eff, self.blend);
        }
        out
    }

    pub fn handles(&mut self, rect: Rect, zoom: f32, ui: &egui::Ui) -> SessionOutcome {
        let to_screen = |pt: Vec2| rect.min + pt * zoom;
        let to_world = |pos: egui::Pos2| (pos - rect.min) / zoom;
        let anchor_screen = to_screen(self.anchor);

        let left_offset = match self.align {
            TextAlign::Left => 0.0,
            TextAlign::Center => -self.last_bounds.x * 0.5,
            TextAlign::Right => -self.last_bounds.x,
        };
        let box_min = self.anchor + vec2(left_offset, 0.0);
        let box_rect = Rect::from_min_size(to_screen(box_min), self.last_bounds * zoom);

        let bg_response = ui.interact(
            ui.clip_rect(),
            ui.id().with("text_bg"),
            egui::Sense::click(),
        );
        let anchor_resp = ui.interact(
            Rect::from_center_size(anchor_screen, Vec2::splat(16.0)),
            ui.id().with("text_anchor"),
            egui::Sense::drag(),
        );
        let mut changed = false;
        if anchor_resp.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if anchor_resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }
        if anchor_resp.drag_started()
            && let Some(ptr) = anchor_resp.interact_pointer_pos()
        {
            let origin = to_world(ptr - anchor_resp.total_drag_delta().unwrap_or_default());
            self.dragging_anchor = Some((self.anchor, origin));
        }
        if let Some((start_anchor, origin)) = self.dragging_anchor {
            if anchor_resp.drag_stopped() {
                self.dragging_anchor = None;
            }
            if let Some(ptr) = anchor_resp.interact_pointer_pos() {
                self.anchor = start_anchor + (to_world(ptr) - origin);
                changed = true;
            }
        }

        let painter = ui.painter();
        let outline = [
            box_rect.left_top(),
            box_rect.right_top(),
            box_rect.right_bottom(),
            box_rect.left_bottom(),
            box_rect.left_top(),
        ];
        paint_dashed_line(painter, &outline);
        painter.circle(
            anchor_screen,
            5.5,
            Color32::from_white_alpha(210),
            Stroke::new(1.2_f32, Color32::BLACK),
        );
        painter.line_segment(
            [
                anchor_screen - vec2(4.0, 0.0),
                anchor_screen + vec2(4.0, 0.0),
            ],
            Stroke::new(1.0_f32, Color32::BLACK),
        );
        painter.line_segment(
            [
                anchor_screen - vec2(0.0, 4.0),
                anchor_screen + vec2(0.0, 4.0),
            ],
            Stroke::new(1.0_f32, Color32::BLACK),
        );
        // Blinking caret hint at top-right / cursor end when editing
        if ((ui.input(|i| i.time) * 2.0) as u64).is_multiple_of(2) {
            let caret_x = match self.align {
                TextAlign::Left => box_rect
                    .right()
                    .min(box_rect.left() + self.last_bounds.x * zoom),
                TextAlign::Center => box_rect.center().x + self.last_bounds.x * zoom * 0.5,
                TextAlign::Right => box_rect.right(),
            };
            painter.line_segment(
                [
                    pos2(caret_x, box_rect.top()),
                    pos2(caret_x, box_rect.bottom()),
                ],
                Stroke::new(1.4_f32, Color32::from_rgb(103, 75, 195)),
            );
        }

        if bg_response.clicked() && self.dragging_anchor.is_none() && !changed {
            SessionOutcome::Commit
        } else if changed {
            SessionOutcome::Changed
        } else {
            SessionOutcome::None
        }
    }
}

#[inline]
fn edge_fn(a: Vec2, b: Vec2, p: Vec2) -> f32 {
    (p.x - a.x) * (b.y - a.y) - (p.y - a.y) * (b.x - a.x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_renders_with_ssaa_partial_alpha() {
        let mut catalog = FontCatalog::new();
        let size = (48, 32);
        let base = vec![Pixel::default(); (size.0 * size.1) as usize];
        let mut session = TextSession::new(
            vec2(4.0, 4.0),
            "Padrão (Sans)".to_owned(),
            20.0,
            true,
            true,
            true,
            false,
            TextAlign::Left,
            Pixel([25, 45, 210, 255]),
            StrokeBlend::Normal,
        );
        session.text = "Aa".to_owned();
        let pixels = session.render_preview(&mut catalog, &base, size, None);
        assert!(pixels.iter().any(|p| p.0[3] > 200));
        assert!(pixels.iter().any(|p| (1..255).contains(&p.0[3])));
    }
}
