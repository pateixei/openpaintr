use super::i18n::text as tr;
use std::{
    borrow::Cow,
    collections::{HashMap, VecDeque},
    io, mem,
};

use openpaintr::{Document, LayerData, Pixel, mul_u8, try_vec_filled, try_vec_with_capacity};

use super::selection::{Bounds, Point, Selection, Shape, bounds_size};
use super::{
    app_settings::AaMode,
    bucket_edges,
    clipboard::Image,
    geometry::{self, Affine, Resampling},
    stroke_raster,
    tolerance::{self, AlphaMatchMode, FillPattern, StrokeBlend},
};

const MAX_HISTORY_BYTES: usize = 512 * 1024 * 1024;
const TILE_SHIFT: u32 = 6;
const TILE_SIDE: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerMeta {
    pub name: String,
    pub visible: bool,
    pub opacity: u8,
    pub blend: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DirtyState {
    pub full: bool,
    pub rect: Option<Bounds>,
}

type PixelChange = (Point, Pixel, Pixel);

enum Edit {
    Raster {
        label: &'static str,
        layer: usize,
        pixels: Vec<PixelChange>,
    },
    Selection {
        label: &'static str,
        before: Option<Selection>,
        after: Option<Selection>,
    },
    Document {
        label: &'static str,
        other: Document,
        other_layer: usize,
        other_selection: Option<Selection>,
    },
    LayerPixels {
        label: &'static str,
        layer: usize,
        other: Vec<Pixel>,
        other_selection: Option<Selection>,
    },
    LayerInserted {
        label: &'static str,
        index: usize,
        data: Option<LayerData>,
        prev_selected: usize,
        prev_selection: Option<Selection>,
    },
    LayerRemoved {
        label: &'static str,
        index: usize,
        data: Option<LayerData>,
        prev_selected: usize,
    },
    LayerMoved {
        label: &'static str,
        from: usize,
        to: usize,
        prev_selected: usize,
    },
    LayerMergedDown {
        top_index: usize,
        top: Option<LayerData>,
        bottom_before: Option<LayerData>,
        prev_selected: usize,
    },
    LayerMetadata {
        label: &'static str,
        layer: usize,
        before: LayerMeta,
        after: LayerMeta,
    },
}

impl Edit {
    fn label(&self) -> &str {
        match self {
            Self::Raster { label, .. }
            | Self::Selection { label, .. }
            | Self::Document { label, .. }
            | Self::LayerPixels { label, .. }
            | Self::LayerInserted { label, .. }
            | Self::LayerRemoved { label, .. }
            | Self::LayerMoved { label, .. }
            | Self::LayerMetadata { label, .. } => label,
            Self::LayerMergedDown { .. } => "merge-layer-down",
        }
    }

    fn approx_bytes(&self) -> usize {
        match self {
            Self::Raster { pixels, .. } => pixels.len() * mem::size_of::<PixelChange>() + 64,
            Self::Selection { .. } => 256,
            Self::Document { other, .. } => {
                let (w, h) = other.dimensions();
                w as usize * h as usize * 4 * other.layer_count() + 256
            }
            Self::LayerPixels { other, .. } => other.len() * mem::size_of::<Pixel>() + 128,
            Self::LayerInserted { data, .. } | Self::LayerRemoved { data, .. } => {
                data.as_ref().map_or(0, |d| d.pixels.len() * 4) + 128
            }
            Self::LayerMoved { .. } | Self::LayerMetadata { .. } => 128,
            Self::LayerMergedDown {
                top, bottom_before, ..
            } => {
                top.as_ref().map_or(0, |d| d.pixels.len() * 4)
                    + bottom_before.as_ref().map_or(0, |d| d.pixels.len() * 4)
                    + 128
            }
        }
    }

    fn touches_pixels(&self) -> bool {
        !matches!(self, Self::Selection { .. })
    }

    fn apply(&mut self, editor: &mut Editor, redo: bool) -> io::Result<()> {
        let document = &mut editor.document;
        match self {
            Self::Raster { layer, pixels, .. } => {
                for &((x, y), before, after) in pixels.iter() {
                    editor
                        .document
                        .set_pixel(*layer, x, y, if redo { after } else { before })?;
                    editor.mark_dirty_point((x, y));
                }
                editor.selected = *layer;
            }
            Self::Selection { before, after, .. } => {
                editor.selection = if redo { after.clone() } else { before.clone() };
            }
            Self::Document {
                other,
                other_layer,
                other_selection,
                ..
            } => {
                mem::swap(document, other);
                mem::swap(&mut editor.selected, other_layer);
                mem::swap(&mut editor.selection, other_selection);
                editor.mark_full_refresh();
            }
            Self::LayerPixels {
                layer,
                other,
                other_selection,
                ..
            } => {
                document.swap_layer_pixels(*layer, other)?;
                editor.selected = *layer;
                mem::swap(&mut editor.selection, other_selection);
                editor.mark_full_refresh();
            }
            Self::LayerInserted {
                index,
                data,
                prev_selected,
                prev_selection,
                ..
            } => {
                if redo {
                    let layer_data = data.take().expect("inserted layer data");
                    document.insert_layer_data(*index, layer_data)?;
                    editor.selected = *index;
                } else {
                    *data = Some(document.remove_layer(*index)?);
                    editor.selected = (*prev_selected).min(document.layer_count() - 1);
                }
                mem::swap(&mut editor.selection, prev_selection);
                editor.mark_full_refresh();
            }
            Self::LayerRemoved {
                index,
                data,
                prev_selected,
                ..
            } => {
                if redo {
                    *data = Some(document.remove_layer(*index)?);
                    editor.selected = (*index).min(document.layer_count() - 1);
                } else {
                    let layer_data = data.take().expect("removed layer data");
                    document.insert_layer_data(*index, layer_data)?;
                    editor.selected = *prev_selected;
                }
                editor.mark_full_refresh();
            }
            Self::LayerMoved {
                from,
                to,
                prev_selected,
                ..
            } => {
                if redo {
                    document.move_layer(*from, *to)?;
                    editor.selected = *to;
                } else {
                    document.move_layer(*to, *from)?;
                    editor.selected = *prev_selected;
                }
                editor.mark_full_refresh();
            }
            Self::LayerMergedDown {
                top_index,
                top,
                bottom_before,
                prev_selected,
            } => {
                if redo {
                    let (removed_top, prev_bottom) = document.merge_layer_down(*top_index)?;
                    *top = Some(removed_top);
                    *bottom_before = Some(prev_bottom);
                    editor.selected = *top_index - 1;
                } else {
                    let prev_bottom = bottom_before.take().expect("previous bottom layer");
                    let removed_top = top.take().expect("merged top layer");
                    document.replace_layer_data(*top_index - 1, prev_bottom)?;
                    document.insert_layer_data(*top_index, removed_top)?;
                    editor.selected = *prev_selected;
                }
                editor.mark_full_refresh();
            }
            Self::LayerMetadata {
                layer,
                before,
                after,
                ..
            } => {
                let target = if redo { after } else { before };
                document.set_layer_name(*layer, target.name.as_str())?;
                document.set_layer_properties(*layer, target.visible, target.opacity)?;
                document.set_layer_blend_mode(*layer, target.blend)?;
                editor.selected = *layer;
                editor.mark_full_refresh();
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub enum StrokeSpec {
    Paint {
        primary: Pixel,
        secondary: Pixel,
        pattern: FillPattern,
        blend: StrokeBlend,
        hardness: u8,
        spacing: u8,
        is_eraser: bool,
    },
    Recolor {
        target: Pixel,
        replacement: Pixel,
        tolerance_pct: u8,
        alpha_mode: AlphaMatchMode,
        hardness: u8,
        spacing: u8,
    },
    Clone {
        source_pixels: Vec<Pixel>,
        source_size: Point,
        offset: (i64, i64),
        blend: StrokeBlend,
        hardness: u8,
        spacing: u8,
    },
}

impl StrokeSpec {
    fn label(&self) -> &'static str {
        match self {
            Self::Paint {
                is_eraser: true, ..
            } => "eraser",
            Self::Paint { .. } => "brush",
            Self::Recolor { .. } => "recolor",
            Self::Clone { .. } => "clone-stamp",
        }
    }

    fn hardness(&self) -> u8 {
        match self {
            Self::Paint { hardness, .. }
            | Self::Recolor { hardness, .. }
            | Self::Clone { hardness, .. } => *hardness,
        }
    }

    fn spacing(&self) -> u8 {
        match self {
            Self::Paint { spacing, .. }
            | Self::Recolor { spacing, .. }
            | Self::Clone { spacing, .. } => *spacing,
        }
    }

    fn apply_pixel(&self, (x, y): Point, before: Pixel, stroke_alpha: u8, sel_cov: u8) -> Pixel {
        let eff = mul_u8(stroke_alpha, sel_cov);
        if eff == 0 {
            return before;
        }
        match self {
            Self::Paint {
                primary,
                secondary,
                pattern,
                blend,
                is_eraser,
                ..
            } => {
                if *is_eraser {
                    stroke_raster::apply_u8(Pixel::default(), before, eff)
                } else {
                    let fg = pattern.sample(x, y, *primary, *secondary);
                    tolerance::blend_covered(fg, before, eff, *blend)
                }
            }
            Self::Recolor {
                target,
                replacement,
                tolerance_pct,
                alpha_mode,
                ..
            } => {
                let Some(recolored) = tolerance::recolor_pixel(
                    before,
                    *target,
                    *replacement,
                    *tolerance_pct,
                    *alpha_mode,
                ) else {
                    return before;
                };
                tolerance::blend_covered(recolored, before, eff, StrokeBlend::Overwrite)
            }
            Self::Clone {
                source_pixels,
                source_size,
                offset,
                blend,
                ..
            } => {
                let Some(fg) = geometry::pixel_checked(
                    source_pixels,
                    *source_size,
                    i64::from(x) + offset.0,
                    i64::from(y) + offset.1,
                ) else {
                    return before;
                };
                tolerance::blend_covered(fg, before, eff, *blend)
            }
        }
    }
}

struct StrokeTile {
    entries: Box<[(Pixel, u16, u8, u8); TILE_SIDE * TILE_SIDE]>,
    touched: Vec<Point>,
}

impl StrokeTile {
    fn new() -> Self {
        Self {
            entries: vec![(Pixel::default(), 0, 0, 255); TILE_SIDE * TILE_SIDE]
                .into_boxed_slice()
                .try_into()
                .expect("tamanho fixo de tile"),
            touched: Vec::new(),
        }
    }

    fn slot(point: Point) -> usize {
        (point.1 as usize & (TILE_SIDE - 1)) * TILE_SIDE + (point.0 as usize & (TILE_SIDE - 1))
    }
}

struct Stroke {
    layer: usize,
    tiles: HashMap<(u16, u16), StrokeTile>,
    spec: StrokeSpec,
    size: u32,
    aa_mode: AaMode,
    carry: f64,
    last: Option<Point>,
}

impl Stroke {
    fn is_empty(&self) -> bool {
        self.tiles.values().all(|tile| tile.touched.is_empty())
    }

    fn get(&self, point: Point) -> Option<(Pixel, u16, u8, u8)> {
        let key = (
            (point.0 >> TILE_SHIFT) as u16,
            (point.1 >> TILE_SHIFT) as u16,
        );
        let entry = self.tiles.get(&key)?.entries[StrokeTile::slot(point)];
        (entry.1 != 0 || entry.2 != 0).then_some(entry)
    }

    fn insert(&mut self, point: Point, before: Pixel, mask: u16, alpha: u8, sel_cov: u8) {
        let key = (
            (point.0 >> TILE_SHIFT) as u16,
            (point.1 >> TILE_SHIFT) as u16,
        );
        let tile = self.tiles.entry(key).or_insert_with(StrokeTile::new);
        let slot = StrokeTile::slot(point);
        if tile.entries[slot].1 == 0 && tile.entries[slot].2 == 0 {
            tile.touched.push(point);
        }
        tile.entries[slot] = (before, mask, alpha, sel_cov);
    }

    fn into_changes(self) -> Vec<PixelChange> {
        let mut pixels = Vec::new();
        for tile in self.tiles.into_values() {
            for point in tile.touched {
                let (before, _mask, alpha, sel_cov) = tile.entries[StrokeTile::slot(point)];
                let after = self.spec.apply_pixel(point, before, alpha, sel_cov);
                if before != after {
                    pixels.push((point, before, after));
                }
            }
        }
        pixels
    }
}

struct HistoryEntry {
    edit: Edit,
    before: u64,
    after: u64,
    bytes: usize,
}

pub struct Editor {
    pub document: Document,
    pub selected: usize,
    pub selection: Option<Selection>,
    undo: VecDeque<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    undo_bytes: usize,
    revision: u64,
    saved_revision: u64,
    next_revision: u64,
    stroke: Option<Stroke>,
    dirty: DirtyState,
}

impl Editor {
    pub fn new(document: Document) -> Self {
        Self {
            document,
            selected: 0,
            selection: None,
            undo: VecDeque::new(),
            redo: Vec::new(),
            undo_bytes: 0,
            revision: 0,
            saved_revision: 0,
            next_revision: 1,
            stroke: None,
            dirty: DirtyState {
                full: true,
                rect: None,
            },
        }
    }

    pub fn history(&self) -> impl DoubleEndedIterator<Item = &str> + ExactSizeIterator {
        self.undo.iter().map(|entry| entry.edit.label())
    }

    pub fn redo_history(&self) -> impl DoubleEndedIterator<Item = &str> + ExactSizeIterator {
        self.redo.iter().map(|entry| entry.edit.label())
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    #[cfg(test)]
    pub fn undo_bytes(&self) -> usize {
        self.undo_bytes
    }

    /// Bytes accounted for the newest undo entry.
    #[cfg(test)]
    pub fn last_undo_entry_bytes(&self) -> Option<usize> {
        self.undo.back().map(|entry| entry.bytes)
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn has_edits(&self) -> bool {
        !self.undo.is_empty()
            || self
                .stroke
                .as_ref()
                .is_some_and(|stroke| !stroke.is_empty())
    }

    pub fn has_unsaved_edits(&self) -> bool {
        self.saved_revision != self.revision
    }

    pub fn mark_saved(&mut self) {
        self.saved_revision = self.revision;
    }

    pub fn mark_new_content(&mut self) {
        self.revision = self.next_revision;
        self.next_revision += 1;
    }

    pub fn mark_full_refresh(&mut self) {
        self.dirty.full = true;
        self.dirty.rect = None;
    }

    pub fn mark_dirty_point(&mut self, point: Point) {
        if self.dirty.full {
            return;
        }
        self.dirty.rect = Some(match self.dirty.rect {
            None => (point, point),
            Some((min, max)) => (
                (min.0.min(point.0), min.1.min(point.1)),
                (max.0.max(point.0), max.1.max(point.1)),
            ),
        });
    }

    pub fn take_dirty(&mut self) -> DirtyState {
        mem::take(&mut self.dirty)
    }

    pub fn layer_meta(&self, layer: usize) -> Option<LayerMeta> {
        let name = self.document.layer_name(layer)?.to_owned();
        let (visible, opacity) = self.document.layer_properties(layer)?;
        let blend = self.document.layer_blend_id(layer)?;
        Some(LayerMeta {
            name,
            visible,
            opacity,
            blend,
        })
    }

    pub fn set_layer_meta(&mut self, layer: usize, meta: LayerMeta) -> io::Result<()> {
        self.document.set_layer_name(layer, meta.name)?;
        self.document
            .set_layer_properties(layer, meta.visible, meta.opacity)?;
        self.document.set_layer_blend_mode(layer, meta.blend)?;
        self.mark_full_refresh();
        Ok(())
    }

    pub fn record_layer_meta(&mut self, layer: usize, before: LayerMeta, label: &'static str) {
        let after = self.layer_meta(layer).expect("existing layer");
        if before != after {
            self.record(Edit::LayerMetadata {
                label,
                layer,
                before,
                after,
            });
        }
    }

    fn record_inserted_layer(
        &mut self,
        label: &'static str,
        index: usize,
        prev_selected: usize,
        prev_selection: Option<Selection>,
    ) {
        self.selected = index;
        self.mark_full_refresh();
        self.record(Edit::LayerInserted {
            label,
            index,
            data: None,
            prev_selected,
            prev_selection,
        });
    }

    fn commit_raster_changes(
        &mut self,
        pixels: Vec<PixelChange>,
        label: &'static str,
    ) -> io::Result<bool> {
        if pixels.is_empty() {
            return Ok(false);
        }
        let mut edit = Edit::Raster {
            label,
            layer: self.selected,
            pixels,
        };
        edit.apply(self, true)?;
        self.record(edit);
        Ok(true)
    }

    pub fn sample_buffer(&self, composite: bool) -> io::Result<Cow<'_, [Pixel]>> {
        if composite {
            Ok(Cow::Owned(self.document.composite()?))
        } else {
            Ok(Cow::Borrowed(
                self.document
                    .layer_pixels(self.selected)
                    .expect("active layer"),
            ))
        }
    }

    pub fn sample_pixel(&self, point: Point, composite: bool) -> Pixel {
        if composite {
            self.document
                .composite_pixel(point.0, point.1)
                .unwrap_or_default()
        } else {
            self.document
                .pixel(self.selected, point.0, point.1)
                .unwrap_or_default()
        }
    }

    pub fn add_layer(&mut self) -> io::Result<()> {
        self.insert_blank_layer(self.document.layer_count())
    }

    pub fn insert_blank_layer(&mut self, index: usize) -> io::Result<()> {
        self.finish_stroke();
        let name =
            super::i18n::format_logical("layer-number", &[&(self.document.layer_count() + 1)]);
        self.document.insert_blank_layer(index, name)?;
        let prev_selected = self.selected;
        self.record_inserted_layer("add-layer", index, prev_selected, self.selection.clone());
        Ok(())
    }

    pub fn delete_layer(&mut self, index: usize) -> io::Result<()> {
        self.finish_stroke();
        let prev_selected = self.selected;
        let removed = self.document.remove_layer(index)?;
        self.selected = index.min(self.document.layer_count() - 1);
        self.mark_full_refresh();
        self.record(Edit::LayerRemoved {
            label: "delete-layer",
            index,
            data: Some(removed),
            prev_selected,
        });
        Ok(())
    }

    pub fn duplicate_layer(&mut self, index: usize) -> io::Result<()> {
        self.finish_stroke();
        let prev_selected = self.selected;
        let target = self.document.duplicate_layer_with_name(index, |name| {
            super::i18n::format_logical("layer-copy", &[&name])
        })?;
        self.record_inserted_layer(
            "duplicate-layer",
            target,
            prev_selected,
            self.selection.clone(),
        );
        Ok(())
    }

    pub fn merge_layer_down(&mut self, index: usize) -> io::Result<()> {
        self.finish_stroke();
        let prev_selected = self.selected;
        let (top, bottom_before) = self.document.merge_layer_down(index)?;
        self.selected = index - 1;
        self.mark_full_refresh();
        self.record(Edit::LayerMergedDown {
            top_index: index,
            top: Some(top),
            bottom_before: Some(bottom_before),
            prev_selected,
        });
        Ok(())
    }

    pub fn move_layer(&mut self, from: usize, to: usize, label: &'static str) -> io::Result<()> {
        if from == to {
            return Ok(());
        }
        self.finish_stroke();
        let prev_selected = self.selected;
        self.document.move_layer(from, to)?;
        self.selected = to;
        self.mark_full_refresh();
        self.record(Edit::LayerMoved {
            label,
            from,
            to,
            prev_selected,
        });
        Ok(())
    }

    pub fn import_document_layers(&mut self, imported: Document) -> io::Result<()> {
        self.finish_stroke();
        let cur = self.document.dimensions();
        let imp = imported.dimensions();
        let target_size = (cur.0.max(imp.0), cur.1.max(imp.1));
        if imported.layer_count() == 1 && target_size == cur {
            let mut data = imported.clone_layer_data(0)?;
            if imp != cur {
                data.pixels = blit_onto_blank(&data.pixels, imp, cur, (0, 0))?;
            }
            let index = self.document.layer_count();
            let prev_selected = self.selected;
            self.document.insert_layer_data(index, data)?;
            self.record_inserted_layer(
                "import-layer",
                index,
                prev_selected,
                self.selection.clone(),
            );
            return Ok(());
        }

        let mut next = if target_size == cur {
            self.document.remap(cur.0, cur.1, |src, x, y| {
                geometry::pixel(src, cur, i64::from(x), i64::from(y))
            })?
        } else {
            geometry::canvas_size(&self.document, target_size, (0, 0))?
        };
        for idx in 0..imported.layer_count() {
            let mut data = imported.clone_layer_data(idx)?;
            if imp != target_size {
                data.pixels = blit_onto_blank(&data.pixels, imp, target_size, (0, 0))?;
            }
            next.insert_layer_data(next.layer_count(), data)?;
        }
        let target_layer = next.layer_count() - 1;
        self.replace_geometry(next, target_layer, "import-layer")
    }

    pub fn select_first_layer(&mut self) {
        self.selected = self.document.layer_count() - 1;
    }

    pub fn select_last_layer(&mut self) {
        self.selected = 0;
    }

    pub fn select_prev_layer(&mut self) {
        if self.selected + 1 < self.document.layer_count() {
            self.selected += 1;
        }
    }

    pub fn select_next_layer(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn set_properties(&mut self, layer: usize, properties: (bool, u8)) -> io::Result<()> {
        self.document
            .set_layer_properties(layer, properties.0, properties.1)?;
        self.mark_full_refresh();
        Ok(())
    }

    pub fn record_properties(&mut self, layer: usize, before: (bool, u8)) {
        let Some(after) = self.layer_meta(layer) else {
            return;
        };
        let prev = LayerMeta {
            name: after.name.clone(),
            visible: before.0,
            opacity: before.1,
            blend: after.blend,
        };
        let label = if prev.visible != after.visible {
            "visibility"
        } else {
            "opacity"
        };
        self.record_layer_meta(layer, prev, label);
    }

    pub fn set_selection(&mut self, selection: Option<Selection>, label: &'static str) {
        self.finish_stroke();
        let size = self.document.dimensions();
        let selection = selection.filter(|s| s.bounds(size).is_some());
        if self.selection != selection {
            let before = mem::replace(&mut self.selection, selection.clone());
            self.record(Edit::Selection {
                label,
                before,
                after: selection,
            });
        }
    }

    pub fn select_all(&mut self) {
        let (width, height) = self.document.dimensions();
        self.set_selection(
            Some(Selection::new(
                Shape::Rectangle,
                (0, 0),
                (width - 1, height - 1),
            )),
            "select-all",
        );
    }

    pub fn invert_selection(&mut self) {
        let Some(mut selection) = self.selection.clone() else {
            self.select_all();
            return;
        };
        selection.inverted = !selection.inverted;
        self.set_selection(Some(selection), "invert-selection");
    }

    pub fn fill_selection(&mut self, color: Pixel, label: &'static str) -> io::Result<bool> {
        self.edit_selection(color, label)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn bucket_fill(
        &mut self,
        seed: Point,
        primary: Pixel,
        secondary: Pixel,
        pattern: FillPattern,
        blend: StrokeBlend,
        tolerance_pct: u8,
        alpha_mode: AlphaMatchMode,
        contiguous: bool,
        sample_composite: bool,
        aa_mode: AaMode,
    ) -> io::Result<bool> {
        self.finish_stroke();
        let Some(target_layer) = self.document.pixel(self.selected, seed.0, seed.1) else {
            return Ok(false);
        };
        let selection = self.selection.as_ref();
        let coverage = |point| selection.map_or(255, |selection| selection.coverage_u8(point));
        if coverage(seed) == 0 {
            return Ok(false);
        }

        let size = self.document.dimensions();
        let sample_src = self.sample_buffer(sample_composite)?;
        let source = self
            .document
            .layer_pixels(self.selected)
            .expect("active layer");
        let index = |(x, y): Point| y as usize * size.0 as usize + x as usize;
        let sample_px = |point: Point| sample_src[index(point)];
        let target = if sample_composite {
            sample_px(seed)
        } else {
            target_layer
        };

        let mut pixels = try_vec_with_capacity(source.len())?;
        pixels.extend_from_slice(source);
        let mut changed = false;
        let matches = |point: Point| {
            coverage(point) != 0
                && tolerance::color_matches(target, sample_px(point), tolerance_pct, alpha_mode)
        };

        let paint_at = |point: Point, base: Pixel, mask: u8| {
            let fg = pattern.sample(point.0, point.1, primary, secondary);
            tolerance::blend_covered(fg, base, mask, blend)
        };

        if !contiguous {
            for y in 0..size.1 {
                for x in 0..size.0 {
                    let point = (x, y);
                    if matches(point) {
                        let i = index(point);
                        let after = paint_at(point, source[i], coverage(point));
                        if after != source[i] {
                            pixels[i] = after;
                            changed = true;
                        }
                    }
                }
            }
        } else {
            Selection::visit_connected(size, seed, matches, |point| {
                let i = index(point);
                let after = paint_at(point, source[i], coverage(point));
                pixels[i] = after;
                changed |= after != source[i];

                for direction in bucket_edges::NEIGHBORS {
                    let Some(edge) = bucket_edges::offset(point, direction, size) else {
                        continue;
                    };
                    let edge_index = index(edge);
                    let mask = coverage(edge);
                    if mask == 0 || matches(edge) || source[edge_index] != pixels[edge_index] {
                        continue;
                    }
                    let local_bg = source[i];
                    if let Some(transition) =
                        bucket_edges::detect(edge, direction, size, local_bg, aa_mode, |p| {
                            source[index(p)]
                        })
                    {
                        let edge_fg = paint_at(edge, local_bg, mask);
                        let recolored = transition.over(edge_fg);
                        changed |= recolored != source[edge_index];
                        pixels[edge_index] = recolored;

                        if let Some(deeper) = bucket_edges::offset(edge, direction, size) {
                            let di = index(deeper);
                            let dmask = coverage(deeper);
                            if dmask != 0
                                && !matches(deeper)
                                && pixels[di] == source[di]
                                && let Some(deeper_edge) =
                                    transition.deeper(source[di], local_bg, aa_mode)
                            {
                                let deeper_fg = paint_at(deeper, local_bg, dmask);
                                let recolored = deeper_edge.over(deeper_fg);
                                changed |= recolored != source[di];
                                pixels[di] = recolored;
                            }
                        }
                    }
                }
            })?;
        }
        if changed {
            self.replace_layer_pixels(pixels, self.selection.clone(), "bucket")?;
        }
        Ok(changed)
    }

    pub fn commit_layer_preview(
        &mut self,
        after_pixels: Vec<Pixel>,
        label: &'static str,
    ) -> io::Result<bool> {
        self.finish_stroke();
        let size = self.document.dimensions();
        let source = self
            .document
            .layer_pixels(self.selected)
            .ok_or_else(|| io::Error::other(tr("missing-active-layer")))?;
        if after_pixels.len() != source.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                tr("invalid-preview-size"),
            ));
        }
        let mut pixels = Vec::new();
        for (idx, (&before, &after)) in source.iter().zip(&after_pixels).enumerate() {
            if before != after {
                let x = (idx % size.0 as usize) as u32;
                let y = (idx / size.0 as usize) as u32;
                pixels.try_reserve(1).map_err(io::Error::other)?;
                pixels.push(((x, y), before, after));
            }
        }
        if pixels.is_empty() {
            return Ok(false);
        }
        self.commit_raster_changes(pixels, label)
    }

    fn edit_selection(&mut self, color: Pixel, label: &'static str) -> io::Result<bool> {
        self.finish_stroke();
        let Some(selection) = &self.selection else {
            return Ok(false);
        };
        let size = self.document.dimensions();
        let bounds = if selection.inverted {
            Some(((0, 0), (size.0 - 1, size.1 - 1)))
        } else {
            selection.bounds(size)
        };
        let Some((min, max)) = bounds else {
            return Ok(false);
        };
        let mut pixels = Vec::new();
        for y in min.1..=max.1 {
            for x in min.0..=max.0 {
                let coverage = selection.coverage_u8((x, y));
                if coverage != 0
                    && let Some(previous) = self.document.pixel(self.selected, x, y)
                {
                    let after = if coverage == 255 {
                        color
                    } else {
                        stroke_raster::apply_u8(color, previous, coverage)
                    };
                    if previous != after {
                        pixels.try_reserve(1).map_err(io::Error::other)?;
                        pixels.push(((x, y), previous, after));
                    }
                }
            }
        }
        self.commit_raster_changes(pixels, label)
    }

    pub fn replace_geometry(
        &mut self,
        document: Document,
        layer: usize,
        label: &'static str,
    ) -> io::Result<()> {
        self.finish_stroke();
        let mut edit = Edit::Document {
            label,
            other: document,
            other_layer: layer,
            other_selection: None,
        };
        edit.apply(self, true)?;
        self.record(edit);
        Ok(())
    }

    pub fn copy_image(&self, flattened: bool) -> io::Result<Image> {
        let size = self.document.dimensions();
        let (min, max) = match self.selection.as_ref() {
            Some(selection) => selection
                .bounds(size)
                .ok_or_else(|| io::Error::other(tr("empty-selection")))?,
            None => ((0, 0), (size.0 - 1, size.1 - 1)),
        };
        let source = self.sample_buffer(flattened)?;
        let copied_size = bounds_size((min, max));
        let mut pixels = try_vec_with_capacity(copied_size.0 as usize * copied_size.1 as usize)?;
        for y in min.1..=max.1 {
            for x in min.0..=max.0 {
                let px = match self.selection.as_ref() {
                    Some(sel) => sel.mask_pixel(&source, size, (x, y)),
                    None => geometry::pixel(&source, size, i64::from(x), i64::from(y)),
                };
                pixels.push(px);
            }
        }
        Ok(Image {
            size: copied_size,
            origin: min,
            pixels,
        })
    }

    pub fn cut_image(&mut self) -> io::Result<bool> {
        if self.selection.is_some() {
            self.edit_selection(Pixel::default(), "cut")
        } else {
            let size = self.document.dimensions();
            let pixels = try_vec_filled(size.0 as usize * size.1 as usize, Pixel::default())?;
            self.replace_layer_pixels(pixels, None, "cut")
                .map(|()| true)
        }
    }

    pub fn paste_into_new_layer(&mut self, image: Image) -> io::Result<()> {
        self.finish_stroke();
        let size = self.document.dimensions();
        let origin = image.clamped_origin(size);
        let mut pixels = blit_onto_blank(&image.pixels, image.size, size, origin)?;
        let index = self
            .document
            .add_layer(super::i18n::logical_text("paste-in-progress"))?;
        self.document.swap_layer_pixels(index, &mut pixels)?;
        let prev_selected = self.selected;
        let prev_selection = self.selection.take();
        self.record_inserted_layer("paste-new-layer", index, prev_selected, prev_selection);
        Ok(())
    }

    fn replace_layer_pixels(
        &mut self,
        pixels: Vec<Pixel>,
        selection: Option<Selection>,
        label: &'static str,
    ) -> io::Result<()> {
        self.finish_stroke();
        let mut edit = Edit::LayerPixels {
            label,
            layer: self.selected,
            other: pixels,
            other_selection: selection,
        };
        edit.apply(self, true)?;
        self.record(edit);
        Ok(())
    }

    pub fn orthogonal_layer(&mut self, operation: geometry::Orthogonal) -> io::Result<()> {
        let size = self.document.dimensions();
        let source = self.document.layer_pixels(self.selected).unwrap();
        let mut pixels = try_vec_with_capacity(source.len())?;
        for y in 0..size.1 {
            for x in 0..size.0 {
                let (sx, sy) = operation.source(size, (x, y));
                pixels.push(geometry::pixel(source, size, i64::from(sx), i64::from(sy)));
            }
        }
        self.replace_layer_pixels(pixels, self.selection.clone(), operation.label())
    }

    pub fn transform_mask(&mut self, transform: Affine) -> io::Result<()> {
        if let Some(selection) = self.transformed_mask(transform)? {
            self.set_selection(Some(selection), "transform-selection");
        }
        Ok(())
    }

    fn transformed_mask(&self, transform: Affine) -> io::Result<Option<Selection>> {
        let size = self.document.dimensions();
        self.selection
            .as_ref()
            .map(|selection| {
                Selection::from_coverage(size, |(x, y)| {
                    let (x, y) = transform.inverse((f64::from(x) + 0.5, f64::from(y) + 0.5));
                    if x >= 0.0 && y >= 0.0 && x < f64::from(size.0) && y < f64::from(size.1) {
                        selection.coverage_u8((x as u32, y as u32))
                    } else {
                        0
                    }
                })
            })
            .transpose()
    }

    fn resample_over(
        &self,
        transform: Affine,
        method: Resampling,
        overlay_on_base: bool,
        at: impl Fn(i64, i64) -> Pixel,
        base_at: impl Fn(u32, u32) -> Pixel,
    ) -> io::Result<Vec<Pixel>> {
        let size = self.document.dimensions();
        let mut out = try_vec_with_capacity(size.0 as usize * size.1 as usize)?;
        for y in 0..size.1 {
            for x in 0..size.0 {
                let fg = geometry::resample(&at, |pt| transform.inverse(pt), (x, y), method);
                if !overlay_on_base {
                    out.push(fg);
                } else {
                    let base = base_at(x, y);
                    out.push(if fg.0[3] == 0 {
                        base
                    } else {
                        fg.over(base, 255)
                    });
                }
            }
        }
        Ok(out)
    }

    pub fn transform_pixels(
        &self,
        transform: Affine,
        method: Resampling,
        use_selection: bool,
        clone_source: bool,
    ) -> io::Result<Vec<Pixel>> {
        let size = self.document.dimensions();
        let source = self.document.layer_pixels(self.selected).unwrap();
        let selection = self.selection.as_ref().filter(|_| use_selection);
        self.resample_over(
            transform,
            method,
            selection.is_some(),
            |x, y| match selection {
                Some(sel) if x >= 0 && y >= 0 && x < i64::from(size.0) && y < i64::from(size.1) => {
                    sel.mask_pixel(source, size, (x as u32, y as u32))
                }
                Some(_) => Pixel::default(),
                None => geometry::pixel(source, size, x, y),
            },
            |x, y| {
                let src_px = geometry::pixel(source, size, i64::from(x), i64::from(y));
                match selection {
                    None => Pixel::default(),
                    Some(_) if clone_source => src_px,
                    Some(sel) => src_px.scale_alpha(255 - sel.coverage_u8((x, y))),
                }
            },
        )
    }

    pub fn preview_floating(
        &self,
        floating: &Image,
        transform: Affine,
        method: Resampling,
    ) -> io::Result<Vec<Pixel>> {
        let size = self.document.dimensions();
        let source = self.document.layer_pixels(self.selected).unwrap();
        let origin = floating.origin;
        self.resample_over(
            transform,
            method,
            true,
            |x, y| {
                geometry::pixel(
                    &floating.pixels,
                    floating.size,
                    x - i64::from(origin.0),
                    y - i64::from(origin.1),
                )
            },
            |x, y| geometry::pixel(source, size, i64::from(x), i64::from(y)),
        )
    }

    pub fn commit_floating(
        &mut self,
        floating: &Image,
        transform: Affine,
        method: Resampling,
    ) -> io::Result<()> {
        let pixels = self.preview_floating(floating, transform, method)?;
        let selection = self.transformed_mask(transform)?;
        self.replace_layer_pixels(pixels, selection, "paste")
    }

    pub fn apply_transform(
        &mut self,
        transform: Affine,
        method: Resampling,
        use_selection: bool,
        clone_source: bool,
    ) -> io::Result<()> {
        if transform.is_identity() && !clone_source {
            return Ok(());
        }
        let pixels = self.transform_pixels(transform, method, use_selection, clone_source)?;
        let selection = if use_selection {
            self.transformed_mask(transform)?
        } else {
            self.selection.clone()
        };
        self.replace_layer_pixels(
            pixels,
            selection,
            if use_selection {
                "transform-pixels"
            } else {
                "layer-rotate-zoom"
            },
        )
    }

    pub fn undo(&mut self) -> io::Result<bool> {
        self.finish_stroke();
        let Some(mut entry) = self.undo.pop_back() else {
            return Ok(false);
        };
        if let Err(error) = entry.edit.apply(self, false) {
            self.undo.push_back(entry);
            return Err(error);
        }
        self.undo_bytes -= entry.bytes;
        self.revision = entry.before;
        self.redo.push(entry);
        Ok(true)
    }

    pub fn redo(&mut self) -> io::Result<bool> {
        self.finish_stroke();
        let Some(mut entry) = self.redo.pop() else {
            return Ok(false);
        };
        if let Err(error) = entry.edit.apply(self, true) {
            self.redo.push(entry);
            return Err(error);
        }
        self.undo_bytes += entry.bytes;
        self.revision = entry.after;
        self.undo.push_back(entry);
        self.trim_history();
        Ok(true)
    }

    pub fn jump_to_state(&mut self, target_undo_len: usize) -> io::Result<bool> {
        let mut changed = false;
        while self.undo.len() > target_undo_len {
            if !self.undo()? {
                break;
            }
            changed = true;
        }
        while self.undo.len() < target_undo_len {
            if !self.redo()? {
                break;
            }
            changed = true;
        }
        Ok(changed)
    }

    #[cfg(test)]
    pub fn begin_stroke(&mut self, color: Pixel, size: u32) {
        self.begin_stroke_with_aa(color, size, AaMode::default());
    }

    #[cfg(test)]
    pub fn begin_stroke_with_aa(&mut self, color: Pixel, size: u32, aa_mode: AaMode) {
        let spec = StrokeSpec::Paint {
            primary: color,
            secondary: Pixel::default(),
            pattern: FillPattern::Solid,
            blend: StrokeBlend::Normal,
            hardness: 100,
            spacing: 15,
            is_eraser: color == Pixel::default(),
        };
        self.begin_stroke_with(spec, size, aa_mode);
    }

    pub fn begin_stroke_with(&mut self, spec: StrokeSpec, size: u32, aa_mode: AaMode) {
        self.finish_stroke();
        self.stroke = Some(Stroke {
            layer: self.selected,
            tiles: HashMap::new(),
            spec,
            size: size.max(1),
            aa_mode,
            carry: 0.0,
            last: None,
        });
    }

    pub fn draw_to(&mut self, point: Option<Point>) -> bool {
        let Some(stroke) = self.stroke.as_mut() else {
            return false;
        };
        let previous = mem::replace(&mut stroke.last, point);
        let Some(point) = point else { return false };
        if previous == Some(point) {
            return false;
        }
        let mut changed = false;
        let mut dirty_points = Vec::new();
        let dimensions = self.document.dimensions();
        let hardness = stroke.spec.hardness();
        let spacing = stroke.spec.spacing();
        let stroke_size = stroke.size;
        let selection = self.selection.as_ref();
        let document = &mut self.document;

        let apply_sample = |doc: &mut Document,
                            st: &mut Stroke,
                            dirty: &mut Vec<Point>,
                            ch: &mut bool,
                            pt @ (x, y): Point,
                            next_mask: u16,
                            next_alpha: u8| {
            let sel_cov = match selection {
                Some(selection) => {
                    let cov = selection.coverage_u8(pt);
                    if cov == 0 {
                        return;
                    }
                    cov
                }
                None => 255,
            };
            let current = doc.pixel(st.layer, x, y).expect("pixel existente");
            let entry = st.get(pt).unwrap_or((current, 0, 0, sel_cov));
            let combined_mask = entry.1 | next_mask;
            let mask_alpha = if combined_mask != 0 {
                stroke_raster::mask16_to_u8(combined_mask)
            } else {
                0
            };
            let combined_alpha = entry.2.max(next_alpha).max(mask_alpha);
            if combined_mask == entry.1 && combined_alpha == entry.2 {
                return;
            }
            let next = st.spec.apply_pixel(pt, entry.0, combined_alpha, entry.3);
            if next != entry.0 || combined_alpha != entry.2 || combined_mask != entry.1 {
                st.insert(pt, entry.0, combined_mask, combined_alpha, entry.3);
            }
            if next != current {
                doc.set_pixel(st.layer, x, y, next)
                    .expect("pixel existente");
                dirty.push(pt);
                *ch = true;
            }
        };

        if stroke.aa_mode == AaMode::Ssaa4x4 && hardness >= 100 && spacing <= 15 {
            stroke_raster::capsule(
                previous.unwrap_or(point),
                point,
                stroke_size,
                dimensions,
                |pt, mask| {
                    apply_sample(
                        document,
                        stroke,
                        &mut dirty_points,
                        &mut changed,
                        pt,
                        mask,
                        0,
                    );
                },
            );
        } else if stroke.aa_mode == AaMode::Analytical && spacing <= 15 {
            stroke_raster::capsule_analytical(
                previous.unwrap_or(point),
                point,
                stroke_size,
                hardness,
                dimensions,
                |pt, alpha| {
                    apply_sample(
                        document,
                        stroke,
                        &mut dirty_points,
                        &mut changed,
                        pt,
                        0,
                        alpha,
                    );
                },
            );
        } else {
            let is_first = previous.is_none();
            let mut carry = stroke.carry;
            stroke_raster::segment_dabs(
                previous.unwrap_or(point),
                point,
                &mut carry,
                stroke_size,
                hardness,
                spacing,
                dimensions,
                is_first,
                stroke.aa_mode,
                |pt, alpha| {
                    apply_sample(
                        document,
                        stroke,
                        &mut dirty_points,
                        &mut changed,
                        pt,
                        0,
                        alpha,
                    );
                },
            );
            stroke.carry = carry;
        }
        for pt in dirty_points {
            self.mark_dirty_point(pt);
        }
        changed
    }

    pub fn finish_stroke(&mut self) {
        let Some(stroke) = self.stroke.take() else {
            return;
        };
        let label = stroke.spec.label();
        let layer = stroke.layer;
        let pixels = stroke.into_changes();
        if !pixels.is_empty() {
            self.record(Edit::Raster {
                label,
                layer,
                pixels,
            });
        }
    }

    fn record(&mut self, edit: Edit) {
        let before = self.revision;
        if edit.touches_pixels() {
            self.revision = self.next_revision;
            self.next_revision += 1;
        }
        let bytes = edit.approx_bytes();
        self.undo_bytes += bytes;
        self.undo.push_back(HistoryEntry {
            edit,
            before,
            after: self.revision,
            bytes,
        });
        self.redo.clear();
        self.trim_history();
    }

    fn trim_history(&mut self) {
        while self.undo.len() > 1 && self.undo_bytes > MAX_HISTORY_BYTES {
            if let Some(dropped) = self.undo.pop_front() {
                self.undo_bytes -= dropped.bytes;
            }
        }
    }
}

fn blit_onto_blank(
    src: &[Pixel],
    src_size: Point,
    dst_size: Point,
    origin: Point,
) -> io::Result<Vec<Pixel>> {
    let len = usize::try_from(u64::from(dst_size.0) * u64::from(dst_size.1))
        .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
    let mut pixels = try_vec_filled(len, Pixel::default())?;
    let copy_w = src_size.0.min(dst_size.0.saturating_sub(origin.0)) as usize;
    let copy_h = src_size.1.min(dst_size.1.saturating_sub(origin.1)) as usize;
    let src_stride = src_size.0 as usize;
    let dst_stride = dst_size.0 as usize;
    for y in 0..copy_h {
        let src_start = y * src_stride;
        let dst_start = (y + origin.1 as usize) * dst_stride + origin.0 as usize;
        pixels[dst_start..dst_start + copy_w].copy_from_slice(&src[src_start..src_start + copy_w]);
    }
    Ok(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill_exact(editor: &mut Editor, seed: Point, color: Pixel) -> io::Result<bool> {
        fill_exact_with_aa(editor, seed, color, AaMode::default())
    }

    fn fill_exact_with_aa(
        editor: &mut Editor,
        seed: Point,
        color: Pixel,
        aa_mode: AaMode,
    ) -> io::Result<bool> {
        editor.bucket_fill(
            seed,
            color,
            Pixel::default(),
            FillPattern::Solid,
            StrokeBlend::Overwrite,
            0,
            AlphaMatchMode::Premultiplied,
            true,
            false,
            aa_mode,
        )
    }

    fn assert_pixels_within(
        actual: &[Pixel],
        expected: &[Pixel],
        width: usize,
        tol: u8,
        label: &str,
    ) {
        for (i, (&actual, &expected)) in actual.iter().zip(expected).enumerate() {
            assert!(
                actual
                    .0
                    .iter()
                    .zip(expected.0)
                    .all(|(&a, b)| a.abs_diff(b) <= tol),
                "{label} pixel {},{}: actual {actual:?} vs expected {expected:?}",
                i % width,
                i / width
            );
        }
    }

    fn assert_pdn_roundtrip_layer0(doc: &Document, expected: &[Pixel]) -> io::Result<()> {
        let mut bytes = Vec::new();
        doc.write_pdn(&mut bytes)?;
        assert_eq!(
            Document::read_pdn(bytes.as_slice())?
                .layer_pixels(0)
                .unwrap(),
            expected
        );
        Ok(())
    }

    #[test]
    fn bucket_fills_only_the_active_layer_and_preserves_selection_coverage() -> io::Result<()> {
        let red = Pixel([243, 70, 40, 255]);
        let blue = Pixel([20, 90, 180, 177]);
        let mut editor = Editor::new(Document::new(9, 9)?);
        editor.document.fill_layer(0, red)?;
        editor.add_layer()?;
        assert!(fill_exact(&mut editor, (4, 4), blue)?);
        assert!(!fill_exact(&mut editor, (4, 4), blue)?);
        assert_eq!(editor.document.pixel(0, 4, 4), Some(red));
        assert_eq!(editor.document.pixel(1, 0, 0), Some(blue));
        assert!(editor.undo()?);
        let selection = Selection::new(Shape::Ellipse, (2, 2), (6, 6));
        editor.set_selection(Some(selection.clone()), "Elipse");

        assert!(fill_exact(&mut editor, (4, 4), blue)?);
        assert_eq!(editor.document.pixel(1, 4, 4), Some(blue));
        assert_eq!(editor.document.pixel(1, 0, 0), Some(Pixel::default()));
        let edge = editor.document.pixel(1, 2, 2).unwrap();
        assert!((1..blue.0[3]).contains(&edge.0[3]));
        assert!(editor.undo()?);
        assert_eq!(editor.selection, Some(selection));
        assert_eq!(editor.document.pixel(1, 4, 4), Some(Pixel::default()));
        assert!(editor.redo()?);
        assert_eq!(editor.document.pixel(1, 2, 2), Some(edge));
        Ok(())
    }

    #[test]
    fn bucket_replaces_background_in_an_antialiased_border_and_keeps_both_sides_separate()
    -> io::Result<()> {
        let green = Pixel([50, 220, 75, 255]);
        let orange = Pixel([255, 130, 16, 255]);
        let blue = Pixel([10, 40, 240, 255]);
        let stroke_over = |bg: Pixel, mode: AaMode| -> io::Result<Editor> {
            let mut ed = Editor::new(Document::new(12, 8)?);
            ed.document.fill_layer(0, bg)?;
            ed.begin_stroke_with_aa(orange, 4, mode);
            ed.draw_to(Some((4, 0)));
            ed.draw_to(Some((8, 7)));
            ed.finish_stroke();
            Ok(ed)
        };
        for mode in AaMode::ALL {
            let mut editor = stroke_over(green, mode)?;
            // Compare against the same geometry rendered directly over blue.
            let on_blue = stroke_over(blue, mode)?;
            let before = editor.document.layer_pixels(0).unwrap().to_vec();

            assert!(fill_exact_with_aa(&mut editor, (0, 3), blue, mode)?);
            assert_eq!(editor.document.pixel(0, 0, 3), Some(blue));
            assert_eq!(editor.document.pixel(0, 11, 3), Some(green));
            assert_eq!(editor.document.pixel(0, 5, 3), Some(orange));
            let mut mixed = 0;
            let mut expected = before.clone();
            for y in 0..8 {
                let mut before_solid_line = true;
                for x in 0..12 {
                    let i = y * 12 + x;
                    if before[i] == orange {
                        before_solid_line = false;
                    }
                    if before_solid_line {
                        expected[i] = on_blue.document.pixel(0, x as u32, y as u32).unwrap();
                        mixed += usize::from(expected[i] != blue);
                    }
                }
            }
            assert!(mixed > 0);
            let tol = if mode == AaMode::Ssaa4x4 { 0 } else { 2 };
            assert_pixels_within(
                editor.document.layer_pixels(0).unwrap(),
                &expected,
                12,
                tol,
                &format!("mode {mode:?}"),
            );
            assert!(editor.undo()?);
            assert_eq!(editor.document.layer_pixels(0).unwrap(), before);
            assert!(editor.redo()?);
            assert_pixels_within(
                editor.document.layer_pixels(0).unwrap(),
                &expected,
                12,
                tol,
                &format!("mode {mode:?} redo"),
            );
        }
        Ok(())
    }

    #[test]
    fn bucket_respects_color_barriers_and_never_jumps_to_a_diagonal_neighbor() -> io::Result<()> {
        let white = Pixel([255, 255, 255, 255]);
        let black = Pixel([0, 0, 0, 255]);
        let color = Pixel([103, 75, 195, 255]);
        let mut editor = Editor::new(Document::new(7, 5)?);
        editor.document.fill_layer(0, white)?;
        for y in 0..5 {
            editor.document.set_pixel(0, 3, y, black)?;
        }
        assert!(fill_exact(&mut editor, (1, 2), color)?);
        for y in 0..5 {
            for x in 0..7 {
                let expected = match x {
                    0..=2 => color,
                    3 => black,
                    _ => white,
                };
                assert_eq!(editor.document.pixel(0, x, y), Some(expected));
            }
        }
        assert!(editor.undo()?);
        assert_eq!(editor.document.pixel(0, 1, 2), Some(white));
        assert!(editor.redo()?);
        assert_eq!(editor.document.pixel(0, 5, 2), Some(white));

        let mut editor = Editor::new(Document::new(2, 2)?);
        editor.document.fill_layer(0, black)?;
        editor.document.set_pixel(0, 0, 0, white)?;
        editor.document.set_pixel(0, 1, 1, white)?;
        assert!(fill_exact(&mut editor, (0, 0), color)?);
        assert_eq!(editor.document.pixel(0, 1, 1), Some(white));
        Ok(())
    }

    #[test]
    fn irregular_copy_paste_and_mask_and_pixel_transforms_are_independent() -> io::Result<()> {
        let mut source = Editor::new(Document::new(8, 8)?);
        let red = Pixel([244, 12, 23, 255]);
        source.document.fill_layer(0, red)?;
        let lasso = Selection::lasso((8, 8), &[(1, 1), (6, 1), (1, 6)])?;
        source.set_selection(Some(lasso.clone()), "Laço");
        let source_image = source.copy_image(true)?;
        assert!(source_image.pixels.contains(&red));
        assert!(source_image.pixels.contains(&Pixel::default()));

        let mut destination = Editor::new(Document::new(8, 8)?);
        destination.paste_into_new_layer(source_image)?;
        assert_eq!(destination.document.pixel(1, 2, 2), Some(red));
        assert_eq!(destination.document.pixel(1, 4, 4), Some(Pixel::default()));
        assert!(destination.undo()?);
        assert_eq!(destination.document.layer_count(), 1);
        assert!(destination.redo()?);
        destination.set_selection(Some(lasso.clone()), "Laço");
        destination.mark_saved();
        let mut transform = Affine::centered((8, 8));
        transform.shift = (2.0, 0.0);
        destination.transform_mask(transform)?;
        assert!(!destination.has_unsaved_edits());
        assert!(destination.selection.as_ref().unwrap().contains((4, 2)));
        assert_eq!(destination.document.pixel(1, 2, 2), Some(red));
        assert!(destination.undo()?);
        assert_eq!(destination.selection, Some(lasso));
        destination.apply_transform(transform, Resampling::Nearest, true, false)?;
        assert_eq!(destination.document.pixel(1, 2, 2), Some(Pixel::default()));
        assert_eq!(destination.document.pixel(1, 4, 2), Some(red));
        assert!(destination.has_unsaved_edits());
        assert!(destination.undo()?);
        assert_eq!(destination.document.pixel(1, 2, 2), Some(red));
        assert!(!destination.has_unsaved_edits());
        Ok(())
    }

    #[test]
    fn selection_clips_paint_and_undo_preserves_the_saved_revision() -> io::Result<()> {
        let mut editor = Editor::new(Document::new(8, 8)?);
        let selected = Selection::new(Shape::Rectangle, (5, 5), (2, 2));
        let color = Pixel([200, 40, 70, 255]);
        editor.set_selection(Some(selected.clone()), "Seleção retangular");
        assert!(!editor.has_unsaved_edits());

        editor.begin_stroke(color, 1);
        editor.draw_to(Some((0, 3)));
        editor.draw_to(Some((7, 3)));
        editor.finish_stroke();
        assert_eq!(editor.document.pixel(0, 1, 3), Some(Pixel::default()));
        assert_eq!(editor.document.pixel(0, 2, 3), Some(color));
        assert_eq!(editor.document.pixel(0, 5, 3), Some(color));
        assert_eq!(editor.document.pixel(0, 6, 3), Some(Pixel::default()));
        assert!(editor.has_unsaved_edits());

        assert!(editor.undo()?);
        assert_eq!(editor.selection, Some(selected));
        assert!(!editor.has_unsaved_edits());
        assert!(editor.undo()?);
        assert_eq!(editor.selection, None);
        assert!(!editor.has_unsaved_edits());
        assert!(editor.redo()?);
        assert!(editor.redo()?);
        assert_eq!(editor.document.pixel(0, 2, 3), Some(color));
        Ok(())
    }

    #[test]
    fn inverted_ellipse_fill_and_delete_touch_only_the_selected_side() -> io::Result<()> {
        let mut editor = Editor::new(Document::new(5, 5)?);
        let color = Pixel([30, 90, 160, 255]);
        editor.set_selection(
            Some(Selection::new(Shape::Ellipse, (0, 0), (4, 4))),
            "Elipse",
        );
        editor.invert_selection();
        assert!(editor.fill_selection(color, "Preencher seleção")?);
        let edge = editor.document.pixel(0, 0, 0).unwrap();
        assert_eq!(&edge.0[..3], &color.0[..3]);
        assert!((1..255).contains(&edge.0[3]));
        assert_eq!(editor.document.pixel(0, 2, 2), Some(Pixel::default()));
        assert!(editor.fill_selection(Pixel::default(), "Apagar seleção")?);
        let erased = editor.document.pixel(0, 0, 0).unwrap();
        assert!(erased.0[3] < edge.0[3]);
        assert!(editor.undo()?);
        assert_eq!(editor.document.pixel(0, 0, 0), Some(edge));
        Ok(())
    }

    #[test]
    fn saved_revision_survives_undo_and_redo_but_not_a_new_branch() -> io::Result<()> {
        let mut editor = Editor::new(Document::new(2, 2)?);
        assert!(!editor.has_unsaved_edits());
        editor.add_layer()?;
        assert!(editor.has_unsaved_edits());
        editor.mark_saved();
        assert!(!editor.has_unsaved_edits());
        assert!(editor.undo()?);
        assert!(editor.has_unsaved_edits());
        assert!(editor.redo()?);
        assert!(!editor.has_unsaved_edits());
        assert!(editor.undo()?);
        editor.add_layer()?;
        assert!(editor.has_unsaved_edits());
        Ok(())
    }

    #[test]
    fn history_restores_pixels_and_layer_order() -> io::Result<()> {
        let mut editor = Editor::new(Document::new(10, 10)?);
        let red = Pixel([255, 0, 0, 255]);
        editor.add_layer()?;
        editor.begin_stroke(red, 1);
        editor.draw_to(Some((1, 2)));
        editor.draw_to(Some((7, 2)));
        editor.finish_stroke();
        editor.set_properties(1, (false, 128))?;
        editor.record_properties(1, (true, 255));

        assert_eq!(editor.history().count(), 3);
        assert_eq!(editor.document.pixel(1, 4, 2), Some(red));
        assert!(editor.undo()?);
        assert_eq!(editor.document.layer_properties(1), Some((true, 255)));
        assert!(editor.undo()?);
        assert_eq!(editor.document.pixel(1, 4, 2), Some(Pixel::default()));
        assert!(editor.undo()?);
        assert_eq!(editor.document.layer_count(), 1);
        assert!(editor.redo()?);
        assert!(editor.redo()?);
        assert!(editor.redo()?);
        assert_eq!(editor.document.pixel(1, 4, 2), Some(red));
        assert_eq!(editor.document.layer_properties(1), Some((false, 128)));
        Ok(())
    }

    #[test]
    fn layer_structural_history_and_jump_to_state_are_reversible() -> io::Result<()> {
        let mut editor = Editor::new(Document::new(4, 4)?);
        editor.document.fill_layer(0, Pixel([10, 20, 30, 255]))?;
        editor.add_layer()?;
        editor.document.fill_layer(1, Pixel([200, 50, 50, 128]))?;
        editor.duplicate_layer(0)?;
        assert_eq!(editor.document.layer_count(), 3);
        assert_eq!(editor.selected, 1);

        editor.move_layer(1, 2, "Mover camada para o topo")?;
        assert_eq!(editor.document.layer_name(2), Some("Background (copy)"));

        let before = editor.layer_meta(2).unwrap();
        let mut next = before.clone();
        next.name = "Renomeada".into();
        next.blend = 1;
        editor.set_layer_meta(2, next)?;
        editor.record_layer_meta(2, before, "Propriedades da camada");

        editor.merge_layer_down(2)?;
        assert_eq!(editor.document.layer_count(), 2);

        let total_states = editor.undo_len();
        assert!(editor.jump_to_state(0)?);
        assert_eq!(editor.document.layer_count(), 1);
        assert!(editor.jump_to_state(total_states)?);
        assert_eq!(editor.document.layer_count(), 2);
        editor.delete_layer(1)?;
        assert_eq!(editor.document.layer_count(), 1);
        assert!(editor.undo()?);
        assert_eq!(editor.document.layer_count(), 2);
        Ok(())
    }

    #[test]
    fn diagonal_stroke_saves_partial_alpha_and_retracing_keeps_coverage() -> io::Result<()> {
        for mode in AaMode::ALL {
            let mut editor = Editor::new(Document::new(12, 12)?);
            let color = Pixel([103, 75, 195, 255]);
            editor.begin_stroke_with_aa(color, 5, mode);
            editor.draw_to(Some((2, 2)));
            editor.draw_to(Some((9, 9)));
            let pixels = editor.document.layer_pixels(0).unwrap().to_vec();
            assert!(pixels.iter().any(|pixel| (1..255).contains(&pixel.0[3])));
            assert_eq!(editor.document.pixel(0, 5, 5), Some(color));
            assert!(!editor.draw_to(Some((2, 2))));
            editor.finish_stroke();
            assert!(editor.undo()?);
            assert_eq!(editor.document.pixel(0, 5, 5), Some(Pixel::default()));
            assert!(editor.redo()?);
            assert_eq!(editor.document.layer_pixels(0).unwrap(), pixels);
            assert_pdn_roundtrip_layer0(&editor.document, &pixels)?;
        }
        Ok(())
    }

    #[test]
    fn diagonal_1px_stroke_uses_aa_and_selection_normalization_works() -> io::Result<()> {
        for mode in AaMode::ALL {
            let mut editor = Editor::new(Document::new(12, 12)?);
            let color = Pixel([40, 120, 220, 255]);
            editor.begin_stroke_with_aa(color, 1, mode);
            editor.draw_to(Some((1, 1)));
            editor.draw_to(Some((8, 8)));
            editor.finish_stroke();
            let pixels = editor.document.layer_pixels(0).unwrap().to_vec();
            assert!(pixels.iter().any(|p| (1..255).contains(&p.0[3])));
            assert_pdn_roundtrip_layer0(&editor.document, &pixels)?;

            editor.select_all();
            editor.invert_selection();
            assert_eq!(editor.selection, None);
            editor.invert_selection();
            assert!(editor.selection.is_some());
            assert!(editor.undo_bytes() > 0);
        }
        Ok(())
    }

    #[test]
    fn erasing_preserves_undo_and_replaces_the_redo_branch() -> io::Result<()> {
        let mut editor = Editor::new(Document::new(2, 2)?);
        let blue = Pixel([0, 0, 255, 255]);
        editor.begin_stroke(blue, 1);
        editor.draw_to(Some((0, 0)));
        editor.draw_to(Some((1, 0)));
        editor.finish_stroke();
        let original = editor.document.pixel(0, 0, 0).unwrap();
        let neighbor = editor.document.pixel(0, 1, 0).unwrap();
        editor.add_layer()?;
        assert!(editor.undo()?);
        assert!(editor.can_redo());

        editor.begin_stroke(Pixel::default(), 1);
        editor.draw_to(Some((0, 0)));
        editor.finish_stroke();
        assert!(!editor.can_redo());
        let erased = editor.document.pixel(0, 0, 0).unwrap();
        assert_eq!(&erased.0[..3], &blue.0[..3]);
        assert!((1..original.0[3]).contains(&erased.0[3]));
        assert_eq!(editor.document.pixel(0, 1, 0), Some(neighbor));
        assert!(editor.undo()?);
        assert_eq!(editor.document.pixel(0, 0, 0), Some(original));
        Ok(())
    }
}
