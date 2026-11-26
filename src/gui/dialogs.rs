use std::{
    fs::File,
    io::{self, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};
use openpaintr::{BLEND_MODES, Document, Pixel, PngCompression, PngDepth};

use super::editor::LayerMeta;

const RECENT_FILE: &str = "recentes.json";
const MAX_RECENT: usize = 10;

pub fn atomic_write_bytes(path: &Path, bytes: &[u8], replace: bool) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let mut file = tempfile::NamedTempFile::new_in(parent.unwrap_or(Path::new(".")))?;
    {
        let mut output = BufWriter::new(file.as_file_mut());
        output.write_all(bytes)?;
        output.flush()?;
    }
    file.as_file().sync_all()?;
    if replace {
        file.persist(path)?;
    } else {
        file.persist_noclobber(path)?;
    }
    Ok(())
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
    let colors = pixels
        .into_iter()
        .map(|Pixel([r, g, b, a])| Color32::from_rgba_unmultiplied(r, g, b, a))
        .collect();
    let image = ColorImage::new([w as usize, h as usize], colors);
    Some(ctx.load_texture(name, image, TextureOptions::LINEAR))
}

#[derive(Default)]
pub struct RecentFiles {
    path: Option<PathBuf>,
    pub entries: Vec<PathBuf>,
}

impl RecentFiles {
    pub fn from_executable() -> Self {
        let path = std::env::current_exe().ok().and_then(|exe| {
            exe.parent()
                .map(|parent| parent.join(RECENT_FILE))
        });
        let entries = path
            .as_deref()
            .and_then(|p| File::open(p).ok())
            .and_then(|f| serde_json::from_reader::<_, Vec<PathBuf>>(BufReader::new(f)).ok())
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
        let bytes = serde_json::to_vec_pretty(&self.entries).map_err(io::Error::other)?;
        atomic_write_bytes(path, &bytes, true)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SaveFormat {
    Pdn { effort: u32 },
    Png { depth: PngDepth, compression: PngCompression },
}

pub struct SaveOptionsDialog {
    pub path: PathBuf,
    pub format: SaveFormat,
    pub needs_overwrite_confirm: bool,
    pub overwrite_confirmed: bool,
    pub is_project: bool,
    encoded: Vec<u8>,
    preview: Option<TextureHandle>,
    error: Option<String>,
}

impl SaveOptionsDialog {
    pub fn new_pdn(
        path: PathBuf,
        needs_overwrite_confirm: bool,
        document: &Document,
        ctx: &egui::Context,
    ) -> Self {
        let mut dialog = Self {
            path,
            format: SaveFormat::Pdn { effort: 1 },
            needs_overwrite_confirm,
            overwrite_confirmed: !needs_overwrite_confirm,
            is_project: true,
            encoded: Vec::new(),
            preview: None,
            error: None,
        };
        dialog.refresh(document, ctx);
        dialog
    }

    pub fn new_png(
        path: PathBuf,
        needs_overwrite_confirm: bool,
        document: &Document,
        ctx: &egui::Context,
    ) -> Self {
        let mut dialog = Self {
            path,
            format: SaveFormat::Png {
                depth: PngDepth::Rgba32,
                compression: PngCompression::Balanced,
            },
            needs_overwrite_confirm,
            overwrite_confirmed: !needs_overwrite_confirm,
            is_project: false,
            encoded: Vec::new(),
            preview: None,
            error: None,
        };
        dialog.refresh(document, ctx);
        dialog
    }

    fn refresh(&mut self, document: &Document, ctx: &egui::Context) {
        self.encoded.clear();
        let res = match self.format {
            SaveFormat::Pdn { effort } => document.write_pdn_with_effort(&mut self.encoded, effort),
            SaveFormat::Png { depth, compression } => {
                document.write_png_with_options(&mut self.encoded, depth, compression)
            }
        };
        self.error = res.err().map(|e| e.to_string());
        if let Ok((w, h, mut pixels)) = document.composite_thumbnail(160) {
            if let SaveFormat::Png {
                depth: depth @ (PngDepth::Rgb24White | PngDepth::Rgb24Black),
                ..
            } = self.format
            {
                let bg = if depth == PngDepth::Rgb24White {
                    Pixel([255, 255, 255, 255])
                } else {
                    Pixel([0, 0, 0, 255])
                };
                for px in &mut pixels {
                    if px.0[3] < 255 {
                        *px = px.over(bg, 255);
                    }
                }
            }
            let colors = pixels
                .into_iter()
                .map(|Pixel([r, g, b, a])| Color32::from_rgba_unmultiplied(r, g, b, a))
                .collect();
            let image = ColorImage::new([w as usize, h as usize], colors);
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
        let response = egui::Modal::new(egui::Id::new("opcoes_salvamento")).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 40.0).clamp(320.0, 480.0));
            let title = match self.format {
                SaveFormat::Pdn { .. } => "Opções de salvamento (.pdn)",
                SaveFormat::Png { .. } => "Opções de exportação (.png)",
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
                    painter.image(
                        checker.id(),
                        rect,
                        egui::Rect::from_min_max(
                            egui::pos2(0.0, 0.0),
                            egui::pos2(size.x / 16.0, size.y / 16.0),
                        ),
                        Color32::WHITE,
                    );
                    painter.image(
                        tex.id(),
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                ui.vertical(|ui| {
                    ui.label(format!("Dimensões: {w} × {h} px"));
                    if matches!(self.format, SaveFormat::Pdn { .. }) {
                        ui.label(format!("Camadas: {}", document.layer_count()));
                    }
                    if self.error.is_none() {
                        ui.label(format!("Tamanho estimado: {}", format_bytes(self.encoded.len())));
                    }
                });
            });

            ui.separator();
            match &mut self.format {
                SaveFormat::Pdn { effort } => {
                    ui.label("Compressão das camadas (DEFLATE)");
                    if ui
                        .add(egui::Slider::new(effort, 1..=9).text("Nível"))
                        .changed()
                    {
                        changed = true;
                    }
                }
                SaveFormat::Png { depth, compression } => {
                    egui::ComboBox::from_label("Profundidade de cor")
                        .selected_text(depth.label())
                        .show_ui(ui, |ui| {
                            for option in PngDepth::ALL {
                                if ui.selectable_value(depth, option, option.label()).changed() {
                                    changed = true;
                                }
                            }
                        });
                    egui::ComboBox::from_label("Compressão PNG")
                        .selected_text(compression.label())
                        .show_ui(ui, |ui| {
                            for option in PngCompression::ALL {
                                if ui
                                    .selectable_value(compression, option, option.label())
                                    .changed()
                                {
                                    changed = true;
                                }
                            }
                        });
                }
            }

            if self.needs_overwrite_confirm {
                ui.checkbox(
                    &mut self.overwrite_confirmed,
                    "O arquivo já existe no destino — confirmar substituição",
                );
            }
            if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.horizontal(|ui| {
                let save = ui
                    .add_enabled(
                        self.error.is_none() && self.overwrite_confirmed,
                        egui::Button::new("Salvar"),
                    )
                    .clicked();
                let cancel = ui.button("Cancelar").clicked();
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
        let response = egui::Modal::new(egui::Id::new("propriedades_camada")).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 40.0).clamp(280.0, 380.0));
            ui.heading("Propriedades da camada");
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Nome");
                if ui.text_edit_singleline(&mut self.draft.name).changed() {
                    self.error = None;
                }
            });
            if ui.checkbox(&mut self.draft.visible, "Visível").changed() {
                live_changed = true;
            }
            if ui
                .add(
                    egui::Slider::new(&mut self.draft.opacity, 0..=255)
                        .custom_formatter(|value, _| format!("{:.0}%", value * 100.0 / 255.0))
                        .custom_parser(|text| {
                            text.trim_end_matches('%')
                                .parse::<f64>()
                                .ok()
                                .map(|value| value * 255.0 / 100.0)
                        })
                        .text("Opacidade"),
                )
                .changed()
            {
                live_changed = true;
            }
            egui::ComboBox::from_label("Modo de mesclagem")
                .selected_text(BLEND_MODES[usize::from(self.draft.blend).min(13)])
                .show_ui(ui, |ui| {
                    for (idx, name) in BLEND_MODES.iter().enumerate() {
                        if ui
                            .selectable_value(&mut self.draft.blend, idx as u8, *name)
                            .changed()
                        {
                            live_changed = true;
                        }
                    }
                });
            if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.horizontal(|ui| (ui.button("OK").clicked(), ui.button("Cancelar").clicked()))
                .inner
        });
        let (ok, cancel) = response.inner;
        if ok {
            if self.draft.name.trim().is_empty() || self.draft.name.contains('\0') {
                self.error = Some("Informe um nome válido para a camada.".into());
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
