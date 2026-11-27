use super::i18n::text as tr;
use std::{io, path::PathBuf};

use eframe::egui::{self, Color32, TextureHandle, TextureOptions};
use openpaintr::{Document, PngCompression, PngDepth, is_valid_layer_name};

use super::{
    editor::LayerMeta,
    storage::{JsonWrite, atomic_write_bytes, exe_adjacent, read_json, write_json_atomic},
    theme::CARD_CORNER_RADIUS,
    widgets::{
        blend_mode_combo, enum_combo_box, opacity_slider, paint_thumbnail, pixels_to_color_image,
    },
    window_chrome,
};

const RECENT_FILE: &str = "recentes.json";
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

pub fn make_thumbnail_texture(
    ctx: &egui::Context,
    name: &str,
    document: &Document,
    layer: Option<usize>,
    max_side: u32,
) -> Option<TextureHandle> {
    let (w, h, pixels) = match layer {
        Some(idx) => document.layer_thumbnail(idx, max_side)?,
        None => document.composite_thumbnail(max_side).ok()?,
    };
    let image = pixels_to_color_image(w as usize, h as usize, &pixels);
    Some(ctx.load_texture(name, image, TextureOptions::LINEAR))
}

#[derive(Default)]
pub struct RecentFiles {
    path: Option<PathBuf>,
    pub entries: Vec<PathBuf>,
}

impl RecentFiles {
    pub fn from_executable() -> Self {
        let path = exe_adjacent(RECENT_FILE).ok();
        let entries = path
            .as_deref()
            .and_then(|p| read_json::<Vec<PathBuf>>(p).ok())
            .unwrap_or_default();
        Self { path, entries }
    }

    pub fn push(&mut self, path: PathBuf) {
        self.entries.retain(|existing| existing != &path);
        self.entries.insert(0, path);
        self.entries.truncate(MAX_RECENT);
        let _ = self.save();
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        let _ = self.save();
    }

    fn save(&self) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        write_json_atomic(path, &self.entries, JsonWrite::PRETTY)
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

pub struct SaveOptionsDialog {
    pub path: PathBuf,
    pub format: SaveFormat,
    pub needs_overwrite_confirm: bool,
    pub overwrite_confirmed: bool,
    encoded: Vec<u8>,
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
        let mut dialog = Self {
            path,
            format,
            needs_overwrite_confirm,
            overwrite_confirmed: !needs_overwrite_confirm,
            encoded: Vec::new(),
            preview: None,
            error: None,
        };
        dialog.refresh(document, ctx);
        dialog
    }

    pub fn is_project(&self) -> bool {
        matches!(self.format, SaveFormat::Pdn { .. })
    }

    fn refresh(&mut self, document: &Document, ctx: &egui::Context) {
        self.encoded.clear();
        let res = match self.format {
            SaveFormat::Pdn { effort } => document.write_pdn_with_effort(&mut self.encoded, effort),
            SaveFormat::Png { depth, compression } => {
                document.write_png_with_options(&mut self.encoded, depth, compression)
            }
        };
        self.error = res.err().as_ref().map(super::i18n::error_message);
        if let Ok((w, h, mut pixels)) = document.composite_thumbnail(160) {
            if let SaveFormat::Png { depth, .. } = self.format {
                for px in &mut pixels {
                    *px = depth.flatten_pixel(*px);
                }
            }
            let image = pixels_to_color_image(w as usize, h as usize, &pixels);
            self.preview = Some(ctx.load_texture("save_preview", image, TextureOptions::LINEAR));
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        document: &Document,
        checker: &TextureHandle,
    ) -> SaveDialogAction {
        let mut changed = false;
        let response = modal(ctx, "opcoes_salvamento").show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 40.0).clamp(320.0, 480.0));
            let title = if self.is_project() {
                tr("pdn-save-options")
            } else {
                tr("png-export-options")
            };
            ui.heading(title);
            ui.weak(self.path.display().to_string());
            ui.separator();

            let (w, h) = document.dimensions();
            ui.horizontal(|ui| {
                if let Some(tex) = &self.preview {
                    let size = tex.size_vec2();
                    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    let painter = ui.painter_at(rect);
                    paint_thumbnail(&painter, checker, tex.id(), rect, 0, None, 16.0);
                }
                ui.vertical(|ui| {
                    ui.label(super::i18n::format("dimensions", &[&w, &h]));
                    if self.is_project() {
                        ui.label(super::i18n::format(
                            "layer-count",
                            &[&document.layer_count()],
                        ));
                    }
                    if self.error.is_none() {
                        ui.label(super::i18n::format(
                            "estimated-size",
                            &[&format_bytes(self.encoded.len())],
                        ));
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
            self.refresh(document, ctx);
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
        atomic_write_bytes(&self.path, &self.encoded, true)
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
        let response = modal(ctx, "propriedades_camada").show(ctx, |ui| {
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
