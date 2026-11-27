//! Outlines of the glyphs in a text layout. egui lays the text out; this
//! module finds each character's outline in the same font egui chose for it.

use std::{borrow::Cow, collections::HashMap};

use ab_glyph::{Font as _, FontArc, FontRef, FontVec, GlyphId, Outline, OutlineCurve, Point};
use eframe::egui::{
    Pos2,
    emath::GuiRounding as _,
    epaint::{FontFamily, text::FontDefinitions},
};
use gfx::path::Path as GfxPath;

pub struct OutlineFonts {
    definitions: FontDefinitions,
    /// Parsed on first use; `None` if the data could not be parsed.
    faces: HashMap<String, Option<FontArc>>,
}

/// A character's glyph and how its font places it.
pub struct GlyphOutline {
    pub outline: Outline,
    /// Pixels per font unit.
    pub scale: f32,
    /// The font's vertical adjustment, in pixels.
    pub y_offset: f32,
    #[cfg(test)]
    pub advance: f32,
}

/// Characters that egui lays out without drawing: controls and blank spacing.
fn is_blank(chr: char) -> bool {
    matches!(
        chr,
        '\t' | '\r' | '\u{2009}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206F}' | '\u{FEFF}'
    )
}

/// Characters egui refuses to take from its built-in fonts.
fn is_ignored(font: &str, chr: char) -> bool {
    FontDefinitions::builtin_font_names().contains(&font)
        && matches!(
            chr,
            '\u{534d}' | '\u{5350}' | '\u{E0FF}' | '\u{EFFD}' | '\u{F0FF}' | '\u{F200}'
        )
}

#[allow(clippy::ptr_arg)] // The borrowed case keeps the font's static data uncopied.
fn parse(data: &Cow<'static, [u8]>, index: u32) -> Option<FontArc> {
    match data {
        Cow::Borrowed(bytes) => FontRef::try_from_slice_and_index(bytes, index)
            .ok()
            .map(FontArc::new),
        Cow::Owned(bytes) => FontVec::try_from_vec_and_index(bytes.clone(), index)
            .ok()
            .map(FontArc::new),
    }
}

impl OutlineFonts {
    pub fn new(definitions: FontDefinitions) -> Self {
        Self {
            definitions,
            faces: HashMap::new(),
        }
    }

    /// The first font of the family that has the character.
    fn find(&mut self, family: &FontFamily, chr: char) -> Option<(String, FontArc, GlyphId)> {
        let names = self.definitions.families.get(family)?;
        for name in names {
            if is_ignored(name, chr) {
                continue;
            }
            let face = self.faces.entry(name.clone()).or_insert_with(|| {
                let data = self.definitions.font_data.get(name)?;
                parse(&data.font, data.index)
            });
            let Some(face) = face else {
                continue;
            };
            let id = face.glyph_id(chr);
            if id.0 != 0 {
                return Some((name.clone(), face.clone(), id));
            }
        }
        None
    }

    /// The glyph egui draws for `chr` at `size` pixels, if it draws one.
    pub fn glyph(&mut self, family: &FontFamily, chr: char, size: f32) -> Option<GlyphOutline> {
        if is_blank(chr) {
            return None;
        }
        // egui substitutes the same characters for missing glyphs.
        let (name, face, id) = [chr, '\u{25FB}', '?']
            .into_iter()
            .find_map(|chr| self.find(family, chr))?;
        let tweak = self.definitions.font_data.get(&name)?.tweak;
        let scale = size * tweak.scale / face.units_per_em()?;
        let y_offset = (size * tweak.scale * tweak.y_offset_factor + tweak.y_offset).round_ui();
        Some(GlyphOutline {
            outline: face.outline(id)?,
            scale,
            y_offset,
            #[cfg(test)]
            advance: face.h_advance_unscaled(id) * scale,
        })
    }
}

impl GlyphOutline {
    /// The lowest pixel row boundary of the glyph below a baseline at zero.
    pub fn bottom(&self) -> f32 {
        let bounds = self.outline.bounds;
        (-bounds.min.y.min(bounds.max.y) * self.scale).ceil()
    }

    /// Appends the outline to `path`, with the pen at `origin` on the
    /// baseline. `place` maps canvas pixels to path coordinates.
    pub fn trace(&self, path: &mut GfxPath, origin: Pos2, place: impl Fn(f32, f32) -> (f32, f32)) {
        // Font units grow upward.
        let at = |point: Point| {
            place(
                origin.x + point.x * self.scale,
                origin.y - point.y * self.scale,
            )
        };
        let mut pen: Option<Point> = None;
        for curve in &self.outline.curves {
            let start = match *curve {
                OutlineCurve::Line(start, _)
                | OutlineCurve::Quad(start, ..)
                | OutlineCurve::Cubic(start, ..) => start,
            };
            // A curve that doesn't continue the previous one starts a contour.
            if pen != Some(start) {
                if pen.is_some() {
                    path.close();
                }
                let (x, y) = at(start);
                path.move_f(x, y);
            }
            pen = Some(match *curve {
                OutlineCurve::Line(_, end) => {
                    let (x, y) = at(end);
                    path.line_f(x, y);
                    end
                }
                OutlineCurve::Quad(_, control, end) => {
                    let ((cx, cy), (x, y)) = (at(control), at(end));
                    path.quad_f(cx, cy, x, y);
                    end
                }
                OutlineCurve::Cubic(_, first, second, end) => {
                    path.cubic_f(at(first), at(second), at(end));
                    end
                }
            });
        }
        if pen.is_some() {
            path.close();
        }
    }
}
