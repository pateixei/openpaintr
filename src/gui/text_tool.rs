use super::i18n::text as tr;
use std::{fs, path::Path, sync::Arc};

use eframe::egui::{
    self, Align, Color32, Rect, Stroke, Vec2,
    epaint::{
        FontFamily, FontId,
        text::{FontData, Fonts, Galley, Glyph, LayoutJob, LayoutSection, TextFormat},
    },
    pos2, vec2,
};
use gfx::{RULE_NONZERO, path::Path as GfxPath};
use openpaintr::Pixel;

use super::{
    app_settings::AaMode,
    glyph_outlines::OutlineFonts,
    i18n, paint_dashed_line,
    selection::{Point, Selection},
    session::{
        HandleDrag, SessionOutcome, handle_response, paint_crosshair, rectangle_outline,
        set_grab_cursor, track_handle_drag,
    },
    tolerance::{self, StrokeBlend},
    vector_tools::{CoverageMask, rasterize_path_analytical, rasterize_path_ssaa, trace_path},
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
    outlines: OutlineFonts,
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
        let outlines = OutlineFonts::new(defs.clone());
        let fonts = Fonts::new(4096, Default::default(), defs);
        Self {
            families,
            fonts,
            outlines,
        }
    }

    fn font_family(&self, name: &str) -> FontFamily {
        match name {
            "" | DEFAULT_FAMILY => FontFamily::Proportional,
            MONOSPACE_FAMILY => FontFamily::Monospace,
            other if self.families.iter().any(|f| f == other) => FontFamily::Name(Arc::from(other)),
            _ => FontFamily::Proportional,
        }
    }

    /// Positions the glyphs, with one point per canvas pixel.
    fn layout(&mut self, session: &TextSession) -> Arc<Galley> {
        self.fonts.begin_pass(4096, Default::default());
        let text = if session.text.is_empty() {
            " ".to_owned()
        } else {
            i18n::display(&session.text).into_owned()
        };
        let color = Color32::WHITE;
        let line_w = (session.params.size_px * 0.075).max(1.0);
        let format = TextFormat {
            font_id: FontId::new(
                session.params.size_px,
                self.font_family(&session.params.family),
            ),
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
        let resolution = layout_resolution(session.params.size_px);
        self.fonts.with_pixels_per_point(resolution).layout_job(job)
    }
}

/// Glyph pixels up to which egui lays text out at a finer resolution.
const LAYOUT_GLYPH_PIXELS: f32 = 240.0;

/// Pixels per point for the layout. egui rounds positions to this grid and also
/// draws every glyph at this resolution into its font atlas: finer for small
/// text, where rounding shows, and coarser for large text, whose glyphs would
/// overflow the atlas and be redrawn on every keystroke.
fn layout_resolution(size_px: f32) -> f32 {
    (LAYOUT_GLYPH_PIXELS / size_px).floor().clamp(1.0, 4.0)
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
        aa_mode: AaMode,
    ) -> Vec<Pixel> {
        let mut out = base_layer.to_vec();
        if self.text.trim().is_empty() || size.0 == 0 || size.1 == 0 {
            self.last_bounds = vec2(12.0, self.params.size_px.max(12.0));
            return out;
        }
        let galley = catalog.layout(self);
        self.last_bounds = galley.size().max(vec2(12.0, self.params.size_px.max(12.0)));

        let scale = aa_mode.path_scale();
        let mut paths = self.paths(catalog, &galley, scale);
        // Each shape is rasterized on its own and then combined: fonts disagree
        // on the winding of their contours, so overlaps could cancel out.
        let coverage = match aa_mode {
            AaMode::Analytical => CoverageMask::combined(
                paths
                    .iter_mut()
                    .map(|path| rasterize_path_analytical(path, RULE_NONZERO, size))
                    .collect(),
                u8::max,
            ),
            AaMode::Ssaa4x4 => CoverageMask::combined(
                paths
                    .iter_mut()
                    .map(|path| rasterize_path_ssaa(path, RULE_NONZERO, size))
                    .collect(),
                |a, b| a | b,
            )
            .map(super::stroke_raster::mask16_to_u8),
        };
        tolerance::blend_coverage_in_place(
            &mut out,
            size,
            selection,
            coverage.pixels(),
            self.params.blend,
            |_| self.color,
        );
        out
    }

    /// The outlines of the glyphs and decoration lines of a layout, in canvas
    /// coordinates multiplied by `scale`.
    fn paths(&self, catalog: &mut FontCatalog, galley: &Galley, scale: f32) -> Vec<GfxPath> {
        let family = catalog.font_family(&self.params.family);
        let size = self.params.size_px;
        // Bold is simulated by drawing each glyph again, slightly to the right.
        let bold_dx = self.params.bold.then(|| (size * 0.045).clamp(0.45, 2.2));
        let line_width = (size * 0.075).max(1.0);
        let mut paths = Vec::new();
        for row in &galley.rows {
            let origin = self.anchor + row.pos.to_vec2();
            for glyph in &row.glyphs {
                let Some(outline) = catalog.outlines.glyph(&family, glyph.chr, size) else {
                    continue;
                };
                let pen = origin + glyph.pos.to_vec2() + vec2(0.0, outline.y_offset);
                // Italic slants the glyph from its lowest point.
                let bottom = pen.y + outline.bottom();
                let slant = if self.params.italic { 0.25 } else { 0.0 };
                let mut path = GfxPath::default();
                for offset in std::iter::once(0.0).chain(bold_dx) {
                    outline.trace(&mut path, pen.to_pos2(), |x, y| {
                        ((x + offset + slant * (bottom - y)) * scale, y * scale)
                    });
                }
                paths.push(path);
            }
            let underline = |glyph: &Glyph| glyph.logical_rect().bottom();
            let strikethrough = |glyph: &Glyph| glyph.logical_rect().center().y;
            let decorations = [
                (self.params.underline, underline as fn(&Glyph) -> f32),
                (self.params.strikethrough, strikethrough),
            ];
            for (_, y_of) in decorations.into_iter().filter(|(enabled, _)| *enabled) {
                // One line per run of glyphs at the same height.
                for run in row.glyphs.chunk_by(|a, b| y_of(a) == y_of(b)) {
                    let (Some(first), Some(last)) = (run.first(), run.last()) else {
                        continue;
                    };
                    let mut y = y_of(first);
                    // As egui places lines, to a quarter of a pixel.
                    Stroke::new(line_width, Color32::WHITE).round_center_to_pixel(4.0, &mut y);
                    let top = origin.y + y - line_width * 0.5;
                    let corners = [
                        (origin.x + first.pos.x, top),
                        (origin.x + last.max_x(), top),
                        (origin.x + last.max_x(), top + line_width),
                        (origin.x + first.pos.x, top + line_width),
                    ];
                    let mut path = GfxPath::default();
                    trace_path(
                        &mut path,
                        corners.map(|(x, y)| (x * scale, y * scale)),
                        true,
                    );
                    paths.push(path);
                }
            }
        }
        paths
    }

    pub fn handles(&mut self, viewport: CanvasViewport, ui: &egui::Ui) -> SessionOutcome {
        let anchor_screen = viewport.to_screen(self.anchor);

        let left_offset = match self.params.align {
            TextAlign::Left => 0.0,
            TextAlign::Center => -self.last_bounds.x * 0.5,
            TextAlign::Right => -self.last_bounds.x,
        };
        let box_min = self.anchor + vec2(left_offset, 0.0);
        let box_rect = Rect::from_min_size(
            viewport.to_screen(box_min),
            self.last_bounds * viewport.zoom,
        );

        let bg_response = ui.interact(
            ui.clip_rect(),
            ui.id().with("text_bg"),
            egui::Sense::click(),
        );
        let anchor_resp = handle_response(ui, anchor_screen, 16.0, "text_anchor");
        let mut changed = false;
        set_grab_cursor(ui.ctx(), &anchor_resp);
        let anchor = self.anchor;
        if let Some((drag, current)) = track_handle_drag(
            &mut self.dragging_anchor,
            (),
            &anchor_resp,
            viewport,
            false,
            || anchor,
        ) {
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
                TextAlign::Left => box_rect
                    .right()
                    .min(box_rect.left() + self.last_bounds.x * viewport.zoom),
                TextAlign::Center => box_rect.center().x + self.last_bounds.x * viewport.zoom * 0.5,
                TextAlign::Right => box_rect.right(),
            };
            painter.line_segment(
                [
                    pos2(caret_x, box_rect.top()),
                    pos2(caret_x, box_rect.bottom()),
                ],
                Stroke::new(1.4_f32, accent),
            );
        }

        SessionOutcome::from_interaction(
            bg_response.clicked(),
            self.dragging_anchor.is_none(),
            changed,
        )
    }
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
        let pixels = session.render_preview(&mut catalog, &base, size, None, AaMode::Analytical);
        assert!(pixels.iter().any(|p| p.0[3] > 200));
        assert!(pixels.iter().any(|p| (1..255).contains(&p.0[3])));
    }

    /// Text over a transparent layer, in an opaque color.
    fn coverage(text: &str, size_px: f32, size: Point, aa_mode: AaMode) -> Vec<u8> {
        let params = TextParams {
            size_px,
            ..TextParams::default()
        };
        let mut session = TextSession::new(vec2(5.3, 4.6), Pixel([0, 0, 0, 255]), params);
        session.text = text.to_owned();
        let base = vec![Pixel::default(); (size.0 * size.1) as usize];
        session
            .render_preview(&mut FontCatalog::new(), &base, size, None, aa_mode)
            .iter()
            .map(|pixel| pixel.0[3])
            .collect()
    }

    #[test]
    fn text_edges_follow_the_antialiasing_mode() {
        let size = (200, 70);
        let analytical = coverage("Ag 45", 48.0, size, AaMode::Analytical);
        let supersampled = coverage("Ag 45", 48.0, size, AaMode::Ssaa4x4);
        assert_ne!(analytical, supersampled);
        // 4 x 4 supersampling has 17 levels of coverage.
        let levels: Vec<u8> = (0..=16_u32)
            .map(|count| super::super::stroke_raster::mask16_to_u8(((1_u32 << count) - 1) as u16))
            .collect();
        assert!(supersampled.iter().all(|alpha| levels.contains(alpha)));
        assert!(analytical.iter().any(|alpha| !levels.contains(alpha)));
    }

    #[test]
    fn long_large_text_renders_to_the_end() {
        // Far more distinct glyphs than a font atlas holds at this size.
        let text: String = ('a'..='z')
            .chain('A'..='Z')
            .chain("こんにちは日本語汉字测试样例水火木金土山川田人口目耳手足力".chars())
            .chain(['W'])
            .collect();
        let size = (24_000, 330);
        let drawn = coverage(&text, 240.0, size, AaMode::Analytical);
        let params = TextParams {
            size_px: 240.0,
            ..TextParams::default()
        };
        let mut session = TextSession::new(vec2(5.3, 4.6), Pixel::default(), params);
        session.text = text;
        let galley = FontCatalog::new().layout(&session);
        let glyphs = &galley.rows[0].glyphs;
        assert_eq!(glyphs.len(), session.text.chars().count());
        // Every glyph has ink within its advance.
        for glyph in glyphs {
            let from = (5.3 + glyph.pos.x).floor() as usize;
            let to = (5.3 + glyph.max_x()).ceil() as usize;
            let inked = drawn
                .chunks_exact(size.0 as usize)
                .any(|row| row[from..to].iter().any(|&alpha| alpha > 128));
            assert!(inked, "{:?} at {from} is missing", glyph.chr);
        }
    }

    #[test]
    fn outlines_come_from_the_fonts_egui_lays_out() {
        let mut catalog = FontCatalog::new();
        let text = "Ag.fi \u{E000}\u{25FB} مرحبا こんにちは 汉字 \u{1F600} ∑";
        for family in catalog.families.clone() {
            for size_px in [13.0, 96.0] {
                let params = TextParams {
                    family: family.clone(),
                    size_px,
                    ..TextParams::default()
                };
                let mut session = TextSession::new(Vec2::ZERO, Pixel::default(), params);
                session.text = text.to_owned();
                let galley = catalog.layout(&session);
                let font = catalog.font_family(&family);
                let mut compared = 0;
                for glyph in galley.rows.iter().flat_map(|row| &row.glyphs) {
                    let Some(outline) = catalog.outlines.glyph(&font, glyph.chr, size_px) else {
                        continue;
                    };
                    compared += 1;
                    assert!(
                        (outline.advance - glyph.advance_width).abs() < 0.01,
                        "{family} {size_px} {:?}: {} != {}",
                        glyph.chr,
                        outline.advance,
                        glyph.advance_width
                    );
                }
                assert!(compared > 10, "{family}: {compared}");
            }
        }
    }
}
