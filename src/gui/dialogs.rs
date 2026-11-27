use super::i18n::text as tr;
use std::{
    io,
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::Duration,
};

use eframe::egui::{self, Color32, TextureHandle, TextureOptions};
use openpaintr::{Document, Pixel, PngCompression, PngDepth, ThumbnailGrid, is_valid_layer_name};

use super::{
    editor::LayerMeta,
    storage::{
        JSON_LIMIT, JsonWrite, UserFile, atomic_write_bytes, exe_adjacent, migrate, read_json,
        state_file, write_json_atomic,
    },
    theme::CARD_CORNER_RADIUS,
    widgets::{
        blend_mode_combo, enum_combo_box, opacity_slider, paint_thumbnail, pixels_to_color_image,
    },
    window_chrome,
};

const RECENT_FILE: &str = "recent.json";
/// The name used beside the executable by earlier versions.
const LEGACY_RECENT_FILE: &str = "recentes.json";
const MAX_RECENT: usize = 10;

pub(super) fn modal(ctx: &egui::Context, id: &str) -> egui::Modal {
    let modal = egui::Modal::new(egui::Id::new(id));
    let fullscreen = ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));
    let radius = window_chrome::corner_radius(ctx, fullscreen);
    // Keep the shaded area inside the rounded window while retaining the modal's input blocker.
    ctx.layer_painter(modal.area.layer()).rect_filled(
        ctx.content_rect(),
        radius,
        modal.backdrop_color,
    );
    modal
        .backdrop_color(Color32::TRANSPARENT)
        .frame(egui::Frame::popup(&ctx.style()).corner_radius(CARD_CORNER_RADIUS))
}

#[derive(Default)]
pub struct RecentFiles {
    path: Option<PathBuf>,
    pub entries: Vec<PathBuf>,
    save_error: Option<io::Error>,
}

impl RecentFiles {
    pub fn from_user_files() -> Self {
        state_file(RECENT_FILE).map_or_else(
            |_| Self::default(),
            |path| Self::load(migrate(path, exe_adjacent(LEGACY_RECENT_FILE), JSON_LIMIT)),
        )
    }

    fn load(file: UserFile) -> Self {
        Self {
            entries: read_json::<Vec<PathBuf>>(&file.read_from).unwrap_or_default(),
            path: Some(file.path),
            save_error: file.migration_error,
        }
    }

    /// The latest failure to save or migrate the list, once.
    pub fn take_save_error(&mut self) -> Option<io::Error> {
        self.save_error.take()
    }

    pub fn push(&mut self, path: PathBuf) {
        self.entries.retain(|existing| existing != &path);
        self.entries.insert(0, path);
        self.entries.truncate(MAX_RECENT);
        self.save();
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.save();
    }

    fn save(&mut self) {
        if let Some(path) = &self.path
            && let Err(error) = write_json_atomic(path, &self.entries, JsonWrite::PRETTY)
        {
            self.save_error = Some(error);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SaveFormat {
    Pdn {
        effort: u32,
    },
    Png {
        depth: PngDepth,
        compression: PngCompression,
    },
}

impl SaveFormat {
    pub fn pdn() -> Self {
        Self::Pdn { effort: 1 }
    }

    pub fn png() -> Self {
        Self::Png {
            depth: PngDepth::Rgba32,
            compression: PngCompression::Balanced,
        }
    }
}

/// Quiet time after the last change before the size is estimated again.
const ESTIMATE_DELAY: f64 = 0.2;
const PREVIEW_SIDE: u32 = 160;

/// A document encoded in one format, and the preview of that encoding.
struct Estimate {
    bytes: io::Result<Vec<u8>>,
    preview: Option<(u32, u32, Vec<Pixel>)>,
}

impl SaveFormat {
    fn encode(self, document: &Document) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        match self {
            Self::Pdn { effort } => document.write_pdn_with_effort(&mut bytes, effort)?,
            Self::Png { depth, compression } => {
                document.write_png_with_options(&mut bytes, depth, compression)?;
            }
        }
        Ok(bytes)
    }

    fn estimate(self, document: &Document) -> Estimate {
        let preview =
            document
                .composite_thumbnail(PREVIEW_SIDE)
                .ok()
                .map(|(width, height, mut pixels)| {
                    if let Self::Png { depth, .. } = self {
                        for pixel in &mut pixels {
                            *pixel = depth.flatten_pixel(*pixel);
                        }
                    }
                    (width, height, pixels)
                });
        Estimate {
            bytes: self.encode(document),
            preview,
        }
    }
}

pub struct SaveOptionsDialog {
    pub path: PathBuf,
    pub format: SaveFormat,
    pub needs_overwrite_confirm: bool,
    pub overwrite_confirmed: bool,
    /// The document as it was when the dialog opened; nothing edits it while
    /// the dialog is up.
    snapshot: Option<Arc<Document>>,
    /// Counts format changes, to recognize outdated estimates.
    generation: u64,
    /// When the format last changed; `None` once its estimate has started.
    changed_at: Option<f64>,
    /// The generation being estimated on a worker thread.
    running: Option<(u64, Receiver<Estimate>)>,
    /// The encoding of the current format.
    encoded: Option<Vec<u8>>,
    preview: Option<TextureHandle>,
    error: Option<String>,
}

impl SaveOptionsDialog {
    pub fn new(
        path: PathBuf,
        format: SaveFormat,
        needs_overwrite_confirm: bool,
        document: &Document,
        ctx: &egui::Context,
    ) -> Self {
        let (snapshot, error) = match document.snapshot() {
            Ok(snapshot) => (Some(Arc::new(snapshot)), None),
            Err(error) => (None, Some(super::i18n::error_message(&error))),
        };
        let mut dialog = Self {
            path,
            format,
            needs_overwrite_confirm,
            overwrite_confirmed: !needs_overwrite_confirm,
            snapshot,
            generation: 0,
            changed_at: None,
            running: None,
            encoded: None,
            preview: None,
            error,
        };
        dialog.start_estimate(ctx);
        dialog
    }

    /// Size of the current format's encoding, once estimated.
    #[cfg(test)]
    pub fn encoded_len(&self) -> Option<usize> {
        self.encoded.as_ref().map(Vec::len)
    }

    pub fn is_project(&self) -> bool {
        matches!(self.format, SaveFormat::Pdn { .. })
    }

    /// Notes a change of format at time `now`.
    pub fn format_changed(&mut self, now: f64) {
        self.generation += 1;
        self.changed_at = Some(now);
        self.encoded = None;
        self.error = None;
    }

    fn start_estimate(&mut self, ctx: &egui::Context) {
        let Some(document) = self.snapshot.clone() else {
            return;
        };
        let (sender, receiver) = mpsc::channel();
        let (format, repaint) = (self.format, ctx.clone());
        let spawned = thread::Builder::new()
            .name("save-estimate".into())
            .spawn(move || {
                // The dialog may have closed meanwhile.
                let _ = sender.send(format.estimate(&document));
                repaint.request_repaint();
            });
        match spawned {
            Ok(_) => self.running = Some((self.generation, receiver)),
            Err(error) => self.error = Some(super::i18n::error_message(&error)),
        }
        self.changed_at = None;
    }

    /// Collects a finished estimate and starts the next one when due.
    pub fn poll(&mut self, ctx: &egui::Context) {
        if let Some((generation, receiver)) = &self.running {
            match receiver.try_recv() {
                Ok(estimate) => {
                    if *generation == self.generation {
                        self.apply(ctx, estimate);
                    }
                    self.running = None;
                }
                Err(TryRecvError::Disconnected) => self.running = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        // One estimate at a time, so that a long drag doesn't pile them up.
        if self.running.is_none()
            && let Some(changed_at) = self.changed_at
        {
            let wait = changed_at + ESTIMATE_DELAY - ctx.input(|input| input.time);
            if wait <= 0.0 {
                self.start_estimate(ctx);
            } else {
                ctx.request_repaint_after(Duration::from_secs_f64(wait));
            }
        }
    }

    fn apply(&mut self, ctx: &egui::Context, estimate: Estimate) {
        match estimate.bytes {
            Ok(bytes) => self.encoded = Some(bytes),
            Err(error) => self.error = Some(super::i18n::error_message(&error)),
        }
        if let Some((width, height, pixels)) = estimate.preview {
            let image = pixels_to_color_image(width as usize, height as usize, &pixels);
            self.preview = Some(ctx.load_texture("save_preview", image, TextureOptions::LINEAR));
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, checker: &TextureHandle) -> SaveDialogAction {
        self.poll(ctx);
        let document = self.snapshot.clone();
        let mut changed = false;
        let response = modal(ctx, "save_options").show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 40.0).clamp(320.0, 480.0));
            let title = if self.is_project() {
                tr("pdn-save-options")
            } else {
                tr("png-export-options")
            };
            ui.heading(title);
            ui.weak(self.path.display().to_string());
            ui.separator();

            let (w, h) = document
                .as_ref()
                .map_or((1, 1), |document| document.dimensions());
            ui.horizontal(|ui| {
                // Reserved while the first preview is computed, so nothing moves.
                let grid = ThumbnailGrid::new(w, h, PREVIEW_SIDE);
                let size = egui::vec2(grid.width as f32, grid.height as f32);
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                if let Some(tex) = &self.preview {
                    let painter = ui.painter_at(rect);
                    paint_thumbnail(&painter, checker, tex.id(), rect, 0, None, 16.0);
                }
                ui.vertical(|ui| {
                    ui.label(super::i18n::format("dimensions", &[&w, &h]));
                    if self.is_project()
                        && let Some(document) = &document
                    {
                        ui.label(super::i18n::format(
                            "layer-count",
                            &[&document.layer_count()],
                        ));
                    }
                    if let Some(encoded) = &self.encoded {
                        ui.label(super::i18n::format(
                            "estimated-size",
                            &[&format_bytes(encoded.len())],
                        ));
                    } else if self.error.is_none() {
                        ui.weak(tr("estimating-size"));
                    }
                });
            });

            ui.separator();
            match &mut self.format {
                SaveFormat::Pdn { effort } => {
                    ui.label(tr("layer-compression"));
                    if ui
                        .add(egui::Slider::new(effort, 1..=9).text(tr("compression-level")))
                        .changed()
                    {
                        changed = true;
                    }
                }
                SaveFormat::Png { depth, compression } => {
                    changed |= enum_combo_box(
                        ui,
                        egui::ComboBox::new("color-depth", tr("color-depth")),
                        depth,
                        &PngDepth::ALL,
                        |opt| tr(opt.label_key()),
                    );
                    changed |= enum_combo_box(
                        ui,
                        egui::ComboBox::new("png-compression", tr("png-compression")),
                        compression,
                        &PngCompression::ALL,
                        |opt| tr(opt.label_key()),
                    );
                }
            }

            if self.needs_overwrite_confirm {
                ui.checkbox(&mut self.overwrite_confirmed, tr("confirm-overwrite"));
            }
            if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.horizontal(|ui| {
                let save = ui
                    .add_enabled(
                        self.error.is_none() && self.overwrite_confirmed,
                        egui::Button::new(tr("save")),
                    )
                    .clicked();
                let cancel = ui.button(tr("cancel")).clicked();
                (save, cancel)
            })
            .inner
        });

        if changed {
            let now = ctx.input(|input| input.time);
            self.format_changed(now);
            ctx.request_repaint_after(Duration::from_secs_f64(ESTIMATE_DELAY));
        }
        let (save, cancel) = response.inner;
        if save {
            SaveDialogAction::Save
        } else if cancel || response.should_close() {
            SaveDialogAction::Cancel
        } else {
            SaveDialogAction::Open
        }
    }

    pub fn write_to_disk(&self) -> io::Result<()> {
        if let Some(encoded) = &self.encoded {
            return atomic_write_bytes(&self.path, encoded, true);
        }
        // Saved before the estimate finished: encode now.
        let document = self
            .snapshot
            .as_ref()
            .ok_or_else(|| io::Error::from(io::ErrorKind::OutOfMemory))?;
        atomic_write_bytes(&self.path, &self.format.encode(document)?, true)
    }
}

pub enum SaveDialogAction {
    Open,
    Save,
    Cancel,
}

pub struct LayerPropertiesDialog {
    pub layer: usize,
    pub original: LayerMeta,
    pub draft: LayerMeta,
    pub error: Option<String>,
}

impl LayerPropertiesDialog {
    pub fn new(layer: usize, meta: LayerMeta) -> Self {
        Self {
            layer,
            original: meta.clone(),
            draft: meta,
            error: None,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> LayerDialogAction {
        let mut live_changed = false;
        let response = modal(ctx, "layer_properties").show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 40.0).clamp(280.0, 380.0));
            ui.heading(tr("layer-properties"));
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(tr("name"));
                if ui.text_edit_singleline(&mut self.draft.name).changed() {
                    self.error = None;
                }
            });
            if ui
                .checkbox(&mut self.draft.visible, tr("visible"))
                .changed()
            {
                live_changed = true;
            }
            live_changed |= ui
                .add(opacity_slider(&mut self.draft.opacity).text(tr("opacity")))
                .changed();
            live_changed |= blend_mode_combo(
                ui,
                egui::ComboBox::new("blend-mode", tr("blend-mode")),
                &mut self.draft.blend,
            );
            if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.horizontal(|ui| (ui.button("OK").clicked(), ui.button(tr("cancel")).clicked()))
                .inner
        });
        let (ok, cancel) = response.inner;
        if ok {
            if !is_valid_layer_name(&self.draft.name) {
                self.error = Some(tr("invalid-layer-name").into());
                LayerDialogAction::Open(live_changed)
            } else {
                LayerDialogAction::Confirm
            }
        } else if cancel || response.should_close() {
            LayerDialogAction::Cancel
        } else {
            LayerDialogAction::Open(live_changed)
        }
    }
}

pub enum LayerDialogAction {
    Open(bool),
    Confirm,
    Cancel,
}

fn format_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} MiB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_files_migrate_and_report_failed_saves() -> io::Result<()> {
        let temp = tempfile::tempdir()?;
        let legacy = temp.path().join(LEGACY_RECENT_FILE);
        std::fs::write(&legacy, r#"["/tmp/old.pdn"]"#)?;
        let new = temp.path().join("state").join(RECENT_FILE);
        let mut recent = RecentFiles::load(migrate(new.clone(), Some(legacy), JSON_LIMIT));
        assert_eq!(recent.entries, [PathBuf::from("/tmp/old.pdn")]);
        assert!(new.is_file() && recent.take_save_error().is_none());

        // A file where the directory should be makes every save fail.
        let blocker = temp.path().join("blocker");
        std::fs::write(&blocker, "")?;
        let mut recent = RecentFiles::load(migrate(blocker.join(RECENT_FILE), None, JSON_LIMIT));
        recent.push("/tmp/new.pdn".into());
        assert!(recent.take_save_error().is_some());
        assert!(recent.take_save_error().is_none());
        Ok(())
    }
}
