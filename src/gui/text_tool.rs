use super::i18n::text as tr;
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};

use eframe::egui::{
    self, Align, Color32, Rect, Stroke, Vec2,
    epaint::{
        FontFamily, FontId,
        text::{FontData, Fonts, Galley, LayoutJob, LayoutSection, TextFormat},
    },
    pos2, vec2,
};
use openpaintr::Pixel;

use super::{
    i18n,
    paint_dashed_line,
    selection::{Point, Selection},
    session::{
        HandleDrag, SessionOutcome, handle_response, paint_crosshair, rectangle_outline,
        set_grab_cursor, track_handle_drag,
    },
    tolerance::{self, StrokeBlend},
    view::CanvasViewport,
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

/// Built-in family IDs; shown through `family_label` translations.
pub const DEFAULT_FAMILY: &str = "Default (Sans)";
pub const MONOSPACE_FAMILY: &str = "Monospace";

pub fn family_label(family: &str) -> &str {
    match family {
        DEFAULT_FAMILY => tr("default-sans"),
        MONOSPACE_FAMILY => tr("monospace"),
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
        let mut families = vec![DEFAULT_FAMILY.to_owned(), MONOSPACE_FAMILY.to_owned()];
        let candidates = [
            ("DejaVu Sans", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
            ("DejaVu Serif", "/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf"),
            ("Liberation Sans", "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf"),
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
                defs.families.insert(FontFamily::Name(Arc::from(name)), family);
                families.push(name.to_owned());
            }
        }
        let fonts = Fonts::new(4096, Default::default(), defs);
        Self { families, fonts }
    }

    fn font_family(&self, name: &str) -> FontFamily {
        match name {
            "" | DEFAULT_FAMILY => FontFamily::Proportional,
            MONOSPACE_FAMILY => FontFamily::Monospace,
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
        let line_w = (session.params.size_px * 0.075).max(1.0);
        let format = TextFormat {
            font_id: FontId::new(session.params.size_px, self.font_family(&session.params.family)),
            color,
            italics: session.params.italic,
            underline: if session.params.underline {
                Stroke::new(line_w, color)
            } else {
                Stroke::NONE
            },
            strikethrough: if session.params.strikethrough {
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
            halign: session.params.align.egui(),
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

#[derive(Clone, Debug, PartialEq)]
pub struct TextParams {
    pub family: String,
    pub size_px: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub align: TextAlign,
    pub blend: StrokeBlend,
}

impl Default for TextParams {
    fn default() -> Self {
        Self {
            family: DEFAULT_FAMILY.to_owned(),
            size_px: 28.0,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            align: TextAlign::Left,
            blend: StrokeBlend::Normal,
        }
    }
}

#[derive(Clone)]
pub struct TextSession {
    pub anchor: Vec2,
    pub text: String,
    pub color: Pixel,
    pub params: TextParams,
    pub last_bounds: Vec2,
    dragging_anchor: Option<HandleDrag<(), Vec2>>,
}

impl TextSession {
    pub fn new(anchor: Vec2, color: Pixel, mut params: TextParams) -> Self {
        params.size_px = params.size_px.clamp(6.0, 240.0);
        let min_h = params.size_px.max(12.0);
        Self {
            anchor,
            text: String::new(),
            color,
            params,
            last_bounds: vec2(24.0, min_h),
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
        if self.text.trim().is_empty() || size.0 == 0 || size.1 == 0 {
            self.last_bounds = vec2(12.0, self.params.size_px.max(12.0));
            return out;
        }
        let (galley, atlas) = catalog.layout_4x(self);
        self.last_bounds = galley.size().max(vec2(12.0, self.params.size_px.max(12.0)));
        if atlas.width() == 0 || atlas.height() == 0 {
            return out;
        }

        // Accumulate 4x4 subpixel alpha coverage in a sparse row-major pixel map.
        let mut cov_map: BTreeMap<Point, [u8; 16]> = BTreeMap::new();
        let bold_dx = if self.params.bold {
            (self.params.size_px * 0.045).clamp(0.45, 2.2)
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
            let a_at = |px: i32, py: i32| f32::from(atlas.pixels[py as usize * atlas_w as usize + px as usize].a());
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
                let max_x = ((p0.x.max(p1.x).max(p2.x) + bold_dx).ceil() as i32)
                    .min(size.0 as i32 - 1);
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
                        for (sy, &oy) in super::stroke_raster::SSAA_OFFSETS_UNIT.iter().enumerate() {
                            let y_sub = py as f32 + oy as f32;
                            for (sx, &ox) in super::stroke_raster::SSAA_OFFSETS_UNIT.iter().enumerate() {
                                let x_sub = px as f32 + ox as f32;
                                let mut a = eval_sample(x_sub, y_sub);
                                if bold_dx > 0.0 {
                                    a = a.max(eval_sample(x_sub - bold_dx, y_sub));
                                }
                                if a > 0 {
                                    let slot = cov_map.entry((px as u32, py as u32)).or_insert([0; 16]);
                                    let idx = sy * 4 + sx;
                                    slot[idx] = slot[idx].max(a);
                                }
                            }
                        }
                    }
                }
            }
        }

        tolerance::blend_coverage_in_place(
            &mut out,
            size,
            selection,
            cov_map.into_iter().map(|(pt, subs)| {
                let sum: u32 = subs.into_iter().map(u32::from).sum();
                (pt, super::stroke_raster::avg16_u8(sum))
            }),
            self.params.blend,
            |_| self.color,
        );
        out
    }

    pub fn handles(&mut self, viewport: CanvasViewport, ui: &egui::Ui) -> SessionOutcome {
        let anchor_screen = viewport.to_screen(self.anchor);

        let left_offset = match self.params.align {
            TextAlign::Left => 0.0,
            TextAlign::Center => -self.last_bounds.x * 0.5,
            TextAlign::Right => -self.last_bounds.x,
        };
        let box_min = self.anchor + vec2(left_offset, 0.0);
        let box_rect = Rect::from_min_size(viewport.to_screen(box_min), self.last_bounds * viewport.zoom);

        let bg_response = ui.interact(
            ui.clip_rect(),
            ui.id().with("text_bg"),
            egui::Sense::click(),
        );
        let anchor_resp = handle_response(ui, anchor_screen, 16.0, "text_anchor");
        let mut changed = false;
        set_grab_cursor(ui.ctx(), &anchor_resp);
        let anchor = self.anchor;
        if let Some((drag, current)) =
            track_handle_drag(&mut self.dragging_anchor, (), &anchor_resp, viewport, false, || anchor)
        {
            self.anchor = drag.start + (current - drag.origin);
            changed = true;
        }

        let painter = ui.painter();
        let accent = ui.visuals().selection.stroke.color;
        paint_dashed_line(painter, &rectangle_outline(box_rect));
        paint_crosshair(painter, anchor_screen, 5.5, 210, 4.0);
        // Blinking caret hint at top-right / cursor end when editing
        if ((ui.input(|i| i.time) * 2.0) as u64).is_multiple_of(2) {
            let caret_x = match self.params.align {
                TextAlign::Left => {
                    box_rect.right().min(box_rect.left() + self.last_bounds.x * viewport.zoom)
                }
                TextAlign::Center => box_rect.center().x + self.last_bounds.x * viewport.zoom * 0.5,
                TextAlign::Right => box_rect.right(),
            };
            painter.line_segment(
                [pos2(caret_x, box_rect.top()), pos2(caret_x, box_rect.bottom())],
                Stroke::new(1.4_f32, accent),
            );
        }

        SessionOutcome::from_interaction(bg_response.clicked(), self.dragging_anchor.is_none(), changed)
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
            Pixel([25, 45, 210, 255]),
            TextParams {
                size_px: 20.0,
                bold: true,
                italic: true,
                underline: true,
                ..TextParams::default()
            },
        );
        session.text = "Aa".to_owned();
        let pixels = session.render_preview(&mut catalog, &base, size, None);
        assert!(pixels.iter().any(|p| p.0[3] > 200));
        assert!(pixels.iter().any(|p| (1..255).contains(&p.0[3])));
    }
}
