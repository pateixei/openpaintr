use super::i18n::text as tr;
use std::io;

use eframe::egui::{self, Vec2, vec2};
use openpaintr::Document;

use super::{
    Action, App, MAX_BRUSH_DIAMETER, Tool,
    dialogs::modal,
    editor::Editor,
    geometry::{self, Affine, Orthogonal, Resampling},
    selection::{Point, Selection, Shape},
    session::ActiveSession,
    shortcuts::Command,
    transform::{Transform, TransformTarget},
    view::{MAX_ZOOM, MIN_ZOOM, Unit, ZOOM_IN_FACTOR, ZOOM_OUT_FACTOR},
    widgets::{dimension_inputs, enum_combo, max_canvas_side, max_texture_side},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SizeKind {
    Resize,
    Canvas,
}

pub struct SizeDialog {
    kind: SizeKind,
    dimensions: Point,
    original: Point,
    proportional: bool,
    anchor: (i64, i64),
    method: Resampling,
}

pub(super) fn resampling_selector(ui: &mut egui::Ui, method: &mut Resampling) {
    enum_combo(
        ui,
        "reamostragem",
        None,
        method,
        &Resampling::ALL,
        Resampling::label,
    );
}

pub fn menu_check_item(ui: &mut egui::Ui, checked: bool, label: &str, hint: &str) -> bool {
    let prefix = if checked { "✓ " } else { "    " };
    let button = if hint.is_empty() {
        egui::Button::new(format!("{prefix}{label}"))
    } else {
        egui::Button::new(format!("{prefix}{label}")).shortcut_text(hint)
    };
    if ui.add(button).clicked() {
        ui.close();
        true
    } else {
        false
    }
}

impl App {
    pub fn phase2_menus(&mut self, ui: &mut egui::Ui) {
        ui.menu_button(tr("edit"), |ui| self.edit_menu(ui));
        ui.menu_button(tr("view"), |ui| self.view_menu(ui));
        ui.menu_button(tr("image"), |ui| self.image_menu(ui));
        ui.menu_button(tr("layers"), |ui| self.layer_menu(ui));
    }

    pub fn shortcut_item(&mut self, ui: &mut egui::Ui, command: Command, enabled: bool) {
        if ui
            .add_enabled(enabled, self.shortcuts.button(ui, command))
            .clicked()
        {
            self.execute_command(ui.ctx(), command);
            ui.close();
        }
    }

    fn edit_menu(&mut self, ui: &mut egui::Ui) {
        use Command::*;
        self.shortcut_item(ui, Undo, self.tab().editor.has_edits());
        self.shortcut_item(ui, Redo, self.tab().editor.can_redo());
        ui.separator();
        for command in [Cut, Copy, CopyComposite, Paste, PasteLayer, PasteImage] {
            self.shortcut_item(ui, command, true);
        }
        ui.separator();
        let selected = self.tab().editor.selection.is_some();
        self.shortcut_item(ui, CopyMask, selected);
        ui.add_enabled_ui(self.clipboard.mask.is_some(), |ui| {
            ui.menu_button(tr("paste-mask"), |ui| {
                self.shortcut_item(ui, PasteMask, true);
                for mode in super::selection::Combine::ALL.into_iter().skip(1) {
                    if ui.button(mode.label()).clicked() {
                        self.paste_mask(mode);
                        ui.close();
                    }
                }
            });
        });
        ui.separator();
        self.shortcut_item(ui, MoveMask, selected);
        self.shortcut_item(ui, MovePixels, selected);
    }

    fn view_menu(&mut self, ui: &mut egui::Ui) {
        use Command::*;
        let has_selection = self.tab().editor.selection.is_some();
        for command in [ZoomIn, ZoomOut, Fit, FitSelection, ActualSize, Zoom, Pan] {
            self.shortcut_item(ui, command, command != FitSelection || has_selection);
        }
        ui.separator();
        for (command, enabled) in [
            (Fullscreen, self.fullscreen),
            (Grid, self.tab().view.grid),
            (Rulers, self.tab().view.rulers),
        ] {
            let hint = self.shortcuts.hint(command);
            if menu_check_item(ui, enabled, command.label(), &hint) {
                self.execute_command(ui.ctx(), command);
            }
        }
        ui.separator();
        for unit in [Unit::Pixels, Unit::Inches, Unit::Centimeters] {
            let selected = self.tab().view.unit == unit;
            if menu_check_item(ui, selected, unit.label(), "") {
                self.tab_mut().view.unit = unit;
            }
        }
    }

    fn image_menu(&mut self, ui: &mut egui::Ui) {
        use Command::*;
        self.shortcut_item(ui, Crop, self.tab().editor.selection.is_some());
        for command in [
            Resize, CanvasSize, FlipImageX, FlipImageY, RotateCw, RotateCcw, RotateHalf, Flatten,
        ] {
            self.shortcut_item(ui, command, true);
        }
    }

    fn layer_menu(&mut self, ui: &mut egui::Ui) {
        use Command::*;
        let count = self.tab().editor.document.layer_count();
        let selected = self.tab().editor.selected;
        self.shortcut_item(ui, AddLayer, true);
        self.shortcut_item(ui, DeleteLayer, count > 1);
        self.shortcut_item(ui, DuplicateLayer, true);
        self.shortcut_item(ui, MergeLayerDown, selected > 0);
        self.shortcut_item(ui, ImportLayer, true);
        ui.separator();
        self.shortcut_item(ui, MoveLayerTop, selected + 1 < count);
        self.shortcut_item(ui, MoveLayerUp, selected + 1 < count);
        self.shortcut_item(ui, MoveLayerDown, selected > 0);
        self.shortcut_item(ui, MoveLayerBottom, selected > 0);
        ui.separator();
        self.shortcut_item(ui, GoTopLayer, selected + 1 < count);
        self.shortcut_item(ui, GoUpLayer, selected + 1 < count);
        self.shortcut_item(ui, GoDownLayer, selected > 0);
        self.shortcut_item(ui, GoBottomLayer, selected > 0);
        ui.separator();
        self.shortcut_item(ui, ToggleLayer, true);
        self.shortcut_item(ui, LayerProperties, true);
        ui.separator();
        for command in [FlipLayerX, FlipLayerY, RotateLayerHalf, RotateZoom] {
            self.shortcut_item(ui, command, true);
        }
    }

    pub fn step_zoom(&mut self, factor: f32) {
        let next = (self.tab().display_zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.tab_mut().zoom = Some(next);
    }

    fn crop(&mut self, ctx: &egui::Context) {
        let dimensions = self.tab().editor.document.dimensions();
        let Some((min, max)) = self
            .tab()
            .editor
            .selection
            .as_ref()
            .and_then(|selection| selection.bounds(dimensions))
        else {
            return;
        };
        let size = super::selection::bounds_size((min, max));
        let limit = max_texture_side(ctx);
        if size.0.max(size.1) as usize > limit {
            self.report(Err(io::Error::other(tr("selection-too-large"))), "");
            return;
        }
        let selected = self.tab().editor.selected;
        let result = {
            let tab = self.tab();
            let selection = tab.editor.selection.as_ref().unwrap();
            tab.editor.document.remap(size.0, size.1, |source, x, y| {
                selection.mask_pixel(source, dimensions, (min.0 + x, min.1 + y))
            })
        };
        self.install_geometry(result, selected, "crop-selection");
    }

    pub(super) fn install_geometry(
        &mut self,
        result: io::Result<Document>,
        selected: usize,
        label: &'static str,
    ) {
        self.finish_opacity();
        let result = result.and_then(|document| {
            self.tab_mut()
                .editor
                .replace_geometry(document, selected, label)
        });
        if result.is_ok() {
            self.tab_mut().view.scroll_to = Some(Vec2::ZERO);
        }
        self.finish_geometry(result);
    }

    pub(super) fn finish_geometry(&mut self, result: io::Result<()>) {
        if result.is_ok() {
            self.tab_mut().thumbs_dirty = true;
            self.tab_mut().editor.mark_full_refresh();
        }
        self.report(result, "");
    }

    fn copy_to_clipboard(&mut self, flattened: bool, cut: bool) {
        self.finish_opacity();
        self.tab_mut().editor.finish_stroke();
        match self.tab().editor.copy_image(flattened) {
            Ok(image) => {
                let system = self.clipboard.put(image);
                if cut {
                    match self.tab_mut().editor.cut_image() {
                        Ok(changed) => {
                            if changed {
                                self.tab_mut().thumbs_dirty = true;
                                self.tab_mut().editor.mark_full_refresh();
                            }
                        }
                        Err(error) => {
                            self.report(Err(error), "");
                            return;
                        }
                    }
                }
                self.report(
                    Ok(()),
                    if system {
                        tr("image-copied")
                    } else {
                        tr("image-copied-internally")
                    },
                );
            }
            Err(error) => self.report(Err(error), ""),
        }
    }

    pub(super) fn paste_from_clipboard(&mut self, new_layer: bool) {
        self.finish_opacity();
        self.commit_active_session();
        if new_layer {
            let result = self
                .clipboard
                .get()
                .and_then(|image| self.tab_mut().editor.paste_into_new_layer(image));
            self.finish_geometry(result);
            return;
        }
        match self.clipboard.get() {
            Ok(mut image) => {
                let size = self.tab().editor.document.dimensions();
                image.origin = image.clamped_origin(size);
                let end = (
                    (image.origin.0 + image.size.0.saturating_sub(1)).min(size.0 - 1),
                    (image.origin.1 + image.size.1.saturating_sub(1)).min(size.1 - 1),
                );
                let min = vec2(image.origin.0 as f32, image.origin.1 as f32);
                let max = vec2(end.0 as f32 + 1.0, end.1 as f32 + 1.0);
                let mut affine = Affine::centered(size);
                let center = (min + max) * 0.5;
                affine.center = (f64::from(center.x), f64::from(center.y));
                let tab = self.tab_mut();
                tab.editor.set_selection(
                    Some(Selection::new(Shape::Rectangle, image.origin, end)),
                    "paste",
                );
                tab.session = Some(ActiveSession::Transform(Transform::with_floating(
                    affine,
                    (min, max),
                    image,
                )));
                tab.editor.mark_full_refresh();
            }
            Err(error) => self.report(Err(error), ""),
        }
    }

    pub fn commit_transform(&mut self, transform: Transform) {
        let result = if let Some(floating) = transform.floating {
            self.tab_mut()
                .editor
                .commit_floating(&floating, transform.affine, transform.method)
        } else {
            match transform.target {
                TransformTarget::Mask => self.tab_mut().editor.transform_mask(transform.affine),
                target => self.tab_mut().editor.apply_transform(
                    transform.affine,
                    transform.method,
                    target == TransformTarget::Pixels,
                    transform.clone_source,
                ),
            }
        };
        self.finish_geometry(result);
    }

    fn run_layer_op(&mut self, op: impl FnOnce(&mut Editor) -> io::Result<()>) {
        self.finish_opacity();
        self.commit_active_session();
        let result = op(&mut self.tab_mut().editor);
        self.finish_geometry(result);
    }

    pub fn paste_new_document(&mut self, ctx: &egui::Context) {
        let result = (|| {
            let image = self.clipboard.get()?;
            let (width, height) = image.size;
            let limit = max_canvas_side(ctx) as usize;
            if width.max(height) as usize > limit {
                return Err(io::Error::other(tr("clipboard-image-too-large")));
            }
            let mut doc = super::new_document(width, height)?;
            let mut pixels = image.pixels;
            doc.swap_layer_pixels(0, &mut pixels)?;
            self.open_in_new_tab(doc, super::i18n::logical_text("image-pasted").into(), None);
            self.tab_mut().editor.mark_new_content();
            Ok(())
        })();
        self.report(result, "");
    }

    fn copy_mask(&mut self) {
        let dims = self.tab().editor.document.dimensions();
        if let Some(mask) = &self.tab().editor.selection {
            self.clipboard.mask = Some((mask.clone(), dims));
        }
    }

    fn paste_mask(&mut self, mode: super::selection::Combine) {
        let Some((mask, old_size)) = &self.clipboard.mask else {
            return;
        };
        let size = self.tab().editor.document.dimensions();
        let result = mask
            .fit_to(*old_size, size)
            .and_then(|mask| mask.combine(self.tab().editor.selection.as_ref(), size, mode));
        match result {
            Ok(mask) => self
                .tab_mut()
                .editor
                .set_selection(Some(mask), "paste-mask"),
            Err(error) => self.report(Err(error), ""),
        }
    }

    pub fn execute_command(&mut self, ctx: &egui::Context, command: Command) {
        use Command::*;
        if matches!(command, Deselect) && self.has_active_session() {
            self.commit_active_session();
            self.tab_mut().editor.set_selection(None, "deselect");
            return;
        }
        match command {
            Brush => self.activate_tool(Tool::Brush),
            Pencil => self.activate_tool(Tool::Pencil),
            Bucket => self.activate_tool(Tool::Bucket),
            Eraser => self.activate_tool(Tool::Eraser),
            Eyedropper => self.activate_tool(Tool::Eyedropper),
            CloneStamp => self.activate_tool(Tool::CloneStamp),
            Recolor => self.activate_tool(Tool::Recolor),
            Gradient => self.activate_tool(Tool::Gradient),
            LineCurve => self.activate_tool(Tool::LineCurve),
            Shape => self.activate_tool(Tool::DrawShape),
            Text => self.activate_tool(Tool::Text),
            Rectangle => self.activate_tool(Tool::Select(super::selection::Shape::Rectangle)),
            Ellipse => self.activate_tool(Tool::Select(super::selection::Shape::Ellipse)),
            Lasso => self.activate_tool(Tool::Lasso),
            Wand => self.activate_tool(Tool::Wand),
            Pan => self.activate_tool(Tool::Pan),
            Zoom => self.activate_tool(Tool::Zoom),
            TemporaryPan => {}
            BrushSmaller | BrushLarger | BrushSmallerFast | BrushLargerFast => {
                let delta = match command {
                    BrushSmaller => -1,
                    BrushLarger => 1,
                    BrushSmallerFast => -5,
                    _ => 5,
                };
                if !self.adjust_stroke_size(delta) {
                    self.diameter = self
                        .diameter
                        .saturating_add_signed(delta)
                        .clamp(1, MAX_BRUSH_DIAMETER);
                }
            }
            New => self.perform(ctx, Action::New),
            Open => self.perform(ctx, Action::Open),
            Save | SaveAs => {
                self.save_project(ctx, command == SaveAs);
            }
            Export => self.export(ctx),
            Close => self.request(ctx, Action::CloseDocument),
            CloseAll => self.request(ctx, Action::CloseAll),
            Exit => self.request(ctx, Action::Close),
            NextDocument => self.cycle_tab(1),
            PrevDocument => self.cycle_tab(-1),
            Undo | Redo => self.step_history(command == Redo),
            Cut => self.copy_to_clipboard(false, true),
            Copy => self.copy_to_clipboard(false, false),
            CopyComposite => self.copy_to_clipboard(true, false),
            Paste | PasteLayer => self.paste_from_clipboard(command == PasteLayer),
            PasteImage => self.paste_new_document(ctx),
            CopyMask => self.copy_mask(),
            PasteMask => self.paste_mask(super::selection::Combine::Replace),
            SelectAll => self.tab_mut().editor.select_all(),
            Deselect => self.tab_mut().editor.set_selection(None, "deselect"),
            Invert => self.tab_mut().editor.invert_selection(),
            Fill | Erase => self.paint_selection(command == Erase),
            MovePixels | FreeTransform if self.tab().editor.selection.is_some() => {
                self.start_transform(TransformTarget::Pixels)
            }
            MovePixels | FreeTransform => self.start_transform(TransformTarget::Layer),
            MoveMask => self.start_transform(TransformTarget::Mask),
            ZoomIn => self.step_zoom(ZOOM_IN_FACTOR),
            ZoomOut => self.step_zoom(ZOOM_OUT_FACTOR),
            Fit => {
                let tab = self.tab_mut();
                tab.zoom = None;
                tab.view.fit_selection = false;
            }
            FitSelection => {
                let tab = self.tab_mut();
                tab.view.fit_selection = tab.editor.selection.is_some();
            }
            ActualSize => self.tab_mut().zoom = Some(1.0),
            Fullscreen => {
                let next = !self.fullscreen;
                self.fullscreen = next;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(next));
            }
            Grid => {
                let tab = self.tab_mut();
                tab.view.grid = !tab.view.grid;
            }
            Rulers => {
                let tab = self.tab_mut();
                tab.view.rulers = !tab.view.rulers;
            }
            Crop => self.crop(ctx),
            Resize | CanvasSize => {
                let original = self.tab().editor.document.dimensions();
                self.size_dialog = Some(SizeDialog {
                    kind: if command == Resize {
                        SizeKind::Resize
                    } else {
                        SizeKind::Canvas
                    },
                    original,
                    dimensions: original,
                    proportional: true,
                    anchor: (1, 1),
                    method: Resampling::Antialiased,
                });
            }
            FlipImageX | FlipImageY | RotateCw | RotateCcw | RotateHalf => {
                let operation = match command {
                    FlipImageX => Orthogonal::FlipX,
                    FlipImageY => Orthogonal::FlipY,
                    RotateCw => Orthogonal::Clockwise,
                    RotateCcw => Orthogonal::Counterclockwise,
                    _ => Orthogonal::HalfTurn,
                };
                let selected = self.tab().editor.selected;
                let result = geometry::orthogonal(&self.tab().editor.document, operation);
                self.install_geometry(result, selected, operation.label());
            }
            Flatten => {
                let result = self
                    .tab()
                    .editor
                    .document
                    .flattened()
                    .and_then(|mut document| {
                        document.set_layer_name(0, super::i18n::logical_text("merged-image"))?;
                        Ok(document)
                    });
                self.install_geometry(result, 0, "flatten");
            }
            AddLayer => self.run_layer_op(Editor::add_layer),
            InsertLayerInPanel => self.run_layer_op(|ed| ed.insert_blank_layer(ed.selected + 1)),
            DeleteLayer | DeleteLayerInPanel => {
                if command == DeleteLayerInPanel && self.tab().editor.document.layer_count() == 1 {
                    return;
                }
                self.run_layer_op(|ed| ed.delete_layer(ed.selected));
            }
            DuplicateLayer => self.run_layer_op(|ed| ed.duplicate_layer(ed.selected)),
            MergeLayerDown => self.run_layer_op(|ed| ed.merge_layer_down(ed.selected)),
            ImportLayer => self.import_layer_from_file(ctx),
            LayerProperties => self.open_layer_properties(self.tab().editor.selected),
            MoveLayerTop | MoveLayerUp | MoveLayerDown | MoveLayerBottom => {
                self.run_layer_op(|ed| {
                    let count = ed.document.layer_count();
                    let selected = ed.selected;
                    let (to, label) = match command {
                        MoveLayerTop => (count - 1, "layer-to-top"),
                        MoveLayerUp => ((selected + 1).min(count - 1), "layer-up"),
                        MoveLayerDown => (selected.saturating_sub(1), "layer-down"),
                        _ => (0, "layer-to-bottom"),
                    };
                    ed.move_layer(selected, to, label)
                });
            }
            GoTopLayer | GoUpLayer | GoDownLayer | GoBottomLayer => {
                self.finish_opacity();
                let ed = &mut self.tab_mut().editor;
                match command {
                    GoTopLayer => ed.select_first_layer(),
                    GoUpLayer => ed.select_prev_layer(),
                    GoDownLayer => ed.select_next_layer(),
                    _ => ed.select_last_layer(),
                }
            }
            ToggleLayer => {
                self.commit_active_session();
                let layer = self.tab().editor.selected;
                self.toggle_layer_visibility(layer);
            }
            FlipLayerX | FlipLayerY | RotateLayerHalf => {
                let operation = match command {
                    FlipLayerX => Orthogonal::FlipX,
                    FlipLayerY => Orthogonal::FlipY,
                    _ => Orthogonal::HalfTurn,
                };
                self.run_layer_op(|ed| ed.orthogonal_layer(operation));
            }
            RotateZoom => self.start_transform(TransformTarget::Layer),
            SwapColors => self.palette.swap(),
            ResetColors => self.palette.reset(),
            Settings => self.shortcut_dialog = Some(self.shortcuts.dialog()),
        }
    }

    pub fn start_transform(&mut self, target: TransformTarget) {
        self.finish_opacity();
        self.commit_active_session();
        self.tab_mut().editor.finish_stroke();
        let size = self.tab().editor.document.dimensions();
        let mut affine = Affine::centered(size);
        let (min, max) = if target == TransformTarget::Layer {
            ((0, 0), (size.0 - 1, size.1 - 1))
        } else {
            let Some(bounds) = self
                .tab()
                .editor
                .selection
                .as_ref()
                .and_then(|selection| selection.bounds(size))
            else {
                return;
            };
            bounds
        };
        let min = vec2(min.0 as f32, min.1 as f32);
        let max = vec2(max.0 as f32 + 1.0, max.1 as f32 + 1.0);
        let center = (min + max) * 0.5;
        affine.center = (f64::from(center.x), f64::from(center.y));
        self.tab_mut().session = Some(ActiveSession::Transform(Transform::new(
            target,
            affine,
            (min, max),
        )));
    }

    pub fn geometry_dialogs(&mut self, ctx: &egui::Context) {
        if let Some(mut dialog) = self.size_dialog.take() {
            let limit = max_canvas_side(ctx);
            let response = modal(ctx, "dimensions").show(ctx, |ui| {
                ui.heading(if dialog.kind == SizeKind::Resize {
                    tr("resize")
                } else {
                    tr("canvas-size")
                });
                ui.horizontal(|ui| {
                    let (w, h) = dimension_inputs(
                        ui,
                        &mut dialog.dimensions.0,
                        &mut dialog.dimensions.1,
                        limit,
                        true,
                    );
                    if dialog.kind == SizeKind::Resize && dialog.proportional {
                        if w {
                            dialog.dimensions =
                                constrained_size(dialog.dimensions.0, dialog.original, limit);
                        } else if h {
                            let (height, width) = constrained_size(
                                dialog.dimensions.1,
                                (dialog.original.1, dialog.original.0),
                                limit,
                            );
                            dialog.dimensions = (width, height);
                        }
                    }
                });
                if dialog.kind == SizeKind::Resize {
                    ui.checkbox(&mut dialog.proportional, tr("maintain-aspect"));
                    resampling_selector(ui, &mut dialog.method);
                } else {
                    ui.label(tr("anchor"));
                    for row in 0..3 {
                        ui.horizontal(|ui| {
                            for col in 0..3 {
                                ui.selectable_value(
                                    &mut dialog.anchor,
                                    (col, row),
                                    [["↖", "↑", "↗"], ["←", "•", "→"], ["↙", "↓", "↘"]]
                                        [row as usize][col as usize],
                                );
                            }
                        });
                    }
                }
                ui.horizontal(|ui| {
                    (
                        ui.button(tr("apply")).clicked(),
                        ui.button(tr("cancel")).clicked(),
                    )
                })
                .inner
            });
            if response.inner.0 {
                let selected = self.tab().editor.selected;
                let size = dialog.dimensions;
                let (result, label) = if dialog.kind == SizeKind::Resize {
                    (
                        geometry::resize(&self.tab().editor.document, size, dialog.method),
                        "resize",
                    )
                } else {
                    let shift = anchor_offset(dialog.original, size, dialog.anchor);
                    (
                        geometry::canvas_size(&self.tab().editor.document, size, shift),
                        "canvas-size",
                    )
                };
                self.install_geometry(result, selected, label);
            } else if !response.inner.1 && !response.should_close() {
                self.size_dialog = Some(dialog);
            }
        }
    }
}

fn constrained_size(value: u32, original: Point, limit: u32) -> Point {
    let ratio = f64::from(original.1) / f64::from(original.0);
    let width = f64::from(value).min(f64::from(limit) / ratio);
    (
        (width.round() as u32).max(1),
        ((width * ratio).round() as u32).clamp(1, limit),
    )
}

pub fn anchor_offset(original: Point, size: Point, anchor: (i64, i64)) -> (i64, i64) {
    let offset = |old, new, anchor| {
        let delta = i64::from(new) - i64::from(old);
        match anchor {
            0 => 0,
            1 => delta.div_euclid(2),
            _ => delta,
        }
    };
    (
        offset(original.0, size.0, anchor.0),
        offset(original.1, size.1, anchor.1),
    )
}

#[cfg(test)]
mod tests {
    use openpaintr::Pixel;

    use super::*;

    #[test]
    fn anchor_and_aspect_ratio_cover_shrink_and_texture_limit() {
        assert_eq!(anchor_offset((8, 6), (3, 9), (0, 2)), (0, 3));
        assert_eq!(anchor_offset((8, 6), (3, 9), (1, 1)), (-3, 1));
        assert_eq!(constrained_size(100, (1, 4), 128), (32, 128));
        assert_eq!(constrained_size(16, (32, 8), 128), (16, 4));
    }

    #[test]
    fn crop_clears_color_outside_irregular_mask_and_undo_restores_document() -> io::Result<()> {
        let context = egui::Context::default();
        let mut document = Document::new(8, 8)?;
        let red = Pixel([244, 12, 23, 255]);
        document.fill_layer(0, red)?;
        let mut app = App::new(&context, document);
        let mask = super::super::Selection::lasso((8, 8), &[(1, 1), (6, 1), (1, 6)])?;
        let (min, max) = mask.bounds((8, 8)).unwrap();
        app.tab_mut()
            .editor
            .set_selection(Some(mask.clone()), "Laço");
        app.crop(&context);
        assert_eq!(
            app.tab().editor.document.dimensions(),
            (max.0 - min.0 + 1, max.1 - min.1 + 1)
        );
        assert_eq!(app.tab().editor.document.pixel(0, 1, 1), Some(red));
        assert_eq!(
            &app.tab().editor.document.pixel(0, 0, 0).unwrap().0[..3],
            &red.0[..3]
        );
        assert_eq!(
            app.tab()
                .editor
                .document
                .pixel(0, max.0 - min.0, max.1 - min.1),
            Some(Pixel::default())
        );
        assert!(app.tab_mut().editor.undo()?);
        assert_eq!(app.tab().editor.document.dimensions(), (8, 8));
        assert_eq!(app.tab().editor.selection, Some(mask));
        Ok(())
    }
}
