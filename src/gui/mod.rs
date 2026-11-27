mod app_settings;
mod bucket_edges;
mod clipboard;
mod colors;
mod dialogs;
mod editor;
mod feather;
mod flash;
mod geometry;
mod gradient_tool;
mod i18n;
mod icon_atlas;
mod mouse_shortcuts;
mod phase2;
mod selection;
mod session;
mod shortcuts;
mod storage;
mod stroke_raster;
mod text_tool;
mod theme;
mod tolerance;
mod tool_icons;
mod tool_options;
mod transform;
mod vector_tools;
mod view;
mod widgets;
mod window_chrome;

use std::{
    fs::File,
    io::{self, BufReader},
    path::{Path, PathBuf},
};

use eframe::egui::{
    self, Align, Color32, ColorImage, PointerButton, Rect, RichText, TextureHandle, TextureOptions,
    TextureWrapMode, Vec2, pos2, vec2,
};
use openpaintr::{Document, Pixel, is_valid_layer_name};
use rfd::FileDialog;

use app_settings::{AaMode, GlobalSettings};
use colors::{ColorPalette, PaletteAction, paint_color_swatch};
use dialogs::{
    LayerDialogAction, LayerPropertiesDialog, RecentFiles, SaveDialogAction, SaveFormat,
    SaveOptionsDialog, make_thumbnail_texture, modal,
};
use editor::{Editor, LayerMeta, StrokeSpec};
use gradient_tool::{
    GradientChannelMode, GradientParams, GradientRepeat, GradientSession, GradientShape, snap_if,
};
use i18n::text as tr;
use mouse_shortcuts::{MouseAction, Scope, Trigger};
use phase2::SizeDialog;
use selection::{Combine, Point, Region, Selection, Shape};
use session::{
    ActiveSession, CanvasResizeDrag, CanvasResizeSession, SessionOutcome, rectangle_outline,
};
use shortcuts::{Command, ShortcutDialog, Shortcuts};
use text_tool::{FontCatalog, TextAlign, TextParams, TextSession};
use theme::{CARD_CORNER_RADIUS, CARD_GAP, Theme};
use tolerance::{AlphaMatchMode, FillPattern, StrokeBlend};
use transform::TransformTarget;
use vector_tools::{
    CurveKind, DashPreset, LineCapStyle, LineCurveParams, LineCurveSession, ShapeFamily,
    ShapeFillMode, ShapeKind, ShapeParams, ShapeSession,
};
use view::{CanvasViewport, MAX_ZOOM, MIN_ZOOM, View, ZOOM_IN_FACTOR, ZOOM_OUT_FACTOR};
use widgets::{
    blend_mode_combo, dimension_inputs, enum_combo, max_canvas_side, max_texture_side,
    opacity_slider, paint_checker, paint_thumbnail, pixels_to_color_image,
};

const APP_NAME: &str = "OpenPaintR";
const CANVAS_ID: &str = "document_canvas";
const LAYERS_ID: &str = "layers_card_focus";
const DEFAULT_SIZE: (u32, u32) = (640, 480);
const PDN_EXT: &str = "pdn";
const DEFAULT_PRIMARY: [u8; 3] = [103, 75, 195];
const DEFAULT_SECONDARY: [u8; 3] = [255, 255, 255];
const MAX_BRUSH_DIAMETER: u32 = 96;
const MAX_OUTLINE_THICKNESS: u32 = 64;

fn linux_backend_choice(
    explicit_backend: Option<&str>,
    is_wsl: bool,
    has_x11_display: bool,
) -> Option<bool> {
    match explicit_backend.map(str::trim) {
        Some(backend) if backend.eq_ignore_ascii_case("x11") => Some(true),
        Some(backend) if backend.eq_ignore_ascii_case("wayland") => Some(false),
        _ if is_wsl && has_x11_display => Some(true),
        _ => None,
    }
}

pub fn run() -> io::Result<()> {
    let locale = i18n::Locale::load();
    let document = locale.with(|| new_document(DEFAULT_SIZE.0, DEFAULT_SIZE.1))?;
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_transparent(true)
            .with_inner_size([1120.0, 760.0])
            .with_min_inner_size([720.0, 480.0]),
        ..Default::default()
    };
    #[cfg(target_os = "linux")]
    {
        use winit::platform::{
            wayland::EventLoopBuilderExtWayland as _, x11::EventLoopBuilderExtX11 as _,
        };
        let explicit = std::env::var("OPENPAINTR_BACKEND")
            .ok()
            .or_else(|| std::env::var("OPENPAINT_BACKEND").ok())
            .or_else(|| std::env::var("RUSTPDN_BACKEND").ok())
            .or_else(|| std::env::var("WINIT_UNIX_BACKEND").ok());
        let is_wsl = std::env::var_os("WSL_INTEROP").is_some()
            || std::env::var_os("WSL_DISTRO_NAME").is_some()
            || std::env::var_os("WSL2_GUI_APPS_ENABLED").is_some();
        let has_x11 = std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty());
        // WSLg's Weston compositor leaves SCTK's fallback CSD subsurfaces at their
        // windowed geometry when fullscreen/maximizing; use WSLg's X11 RAIL bridge.
        if let Some(use_x11) = linux_backend_choice(explicit.as_deref(), is_wsl, has_x11) {
            options.event_loop_builder = Some(Box::new(move |builder| {
                if use_x11 {
                    builder.with_x11();
                } else {
                    builder.with_wayland();
                }
            }));
        }
    }
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |creation| {
            let mut app = App::with_settings(&creation.egui_ctx, document, GlobalSettings::load());
            #[cfg(target_os = "linux")]
            app.clipboard.attach_window(creation);
            app.locale = locale;
            creation.egui_ctx.set_fonts(i18n::fonts(locale));
            app.font_catalog = FontCatalog::for_locale(locale);
            app.locale.with(|| {
                app.shortcuts = Shortcuts::from_user_files();
                if let Some(error) = &app.shortcuts.storage_error {
                    let message = i18n::format("shortcuts-error", &[error]);
                    app.show_error(message);
                }
                app.recent = RecentFiles::from_user_files();
                app.palette = ColorPalette::from_user_files(DEFAULT_PRIMARY, DEFAULT_SECONDARY);
            });
            Ok(Box::new(app))
        }),
    )
    .map_err(|error| io::Error::other(error.to_string()))
}

#[derive(Clone, Copy)]
enum Action {
    New,
    Open,
    Close,
    CloseAll,
    CloseDocument,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CloseScope {
    Single,
    All,
    Exit,
}

struct PendingClose {
    scope: CloseScope,
    tab_index: usize,
    waiting_save_dialog: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Brush,
    Pencil,
    Bucket,
    Eraser,
    Eyedropper,
    CloneStamp,
    Recolor,
    Gradient,
    LineCurve,
    DrawShape,
    Text,
    Select(Shape),
    Lasso,
    Wand,
    Pan,
    Zoom,
}

impl Tool {
    fn shortcut(self) -> Command {
        match self {
            Self::Brush => Command::Brush,
            Self::Pencil => Command::Pencil,
            Self::Bucket => Command::Bucket,
            Self::Eraser => Command::Eraser,
            Self::Eyedropper => Command::Eyedropper,
            Self::CloneStamp => Command::CloneStamp,
            Self::Recolor => Command::Recolor,
            Self::Gradient => Command::Gradient,
            Self::LineCurve => Command::LineCurve,
            Self::DrawShape => Command::Shape,
            Self::Text => Command::Text,
            Self::Select(Shape::Rectangle) => Command::Rectangle,
            Self::Select(Shape::Ellipse) => Command::Ellipse,
            Self::Lasso => Command::Lasso,
            Self::Wand => Command::Wand,
            Self::Pan => Command::Pan,
            Self::Zoom => Command::Zoom,
        }
    }

    fn label(self) -> &'static str {
        tr(self.label_key())
    }

    fn label_key(self) -> &'static str {
        match self {
            Self::Brush => "brush",
            Self::Pencil => "pencil",
            Self::Bucket => "bucket",
            Self::Eraser => "eraser",
            Self::Eyedropper => "eyedropper",
            Self::CloneStamp => "clone-stamp",
            Self::Recolor => "recolor",
            Self::Gradient => "gradient",
            Self::LineCurve => "line-curve",
            Self::DrawShape => "shapes",
            Self::Text => "text",
            Self::Select(Shape::Rectangle) => "rectangle-select",
            Self::Select(Shape::Ellipse) => "ellipse-select",
            Self::Lasso => "lasso",
            Self::Wand => "magic-wand",
            Self::Pan => "hand",
            Self::Zoom => "zoom",
        }
    }
}

struct SelectionGesture {
    anchor: Point,
    anchor_f: Vec2,
    last_world: Vec2,
    button: PointerButton,
    combine: Combine,
    constrain: bool,
}

struct StrokeGesture {
    button: PointerButton,
    waiting_for_drag: bool,
}

struct PanGesture {
    button: PointerButton,
    last_position: Option<egui::Pos2>,
}

struct DocumentTab {
    editor: Editor,
    name: String,
    project_path: Option<PathBuf>,
    zoom: Option<f32>,
    display_zoom: f32,
    view: View,
    session: Option<ActiveSession>,
    session_creation: Option<(PointerButton, Vec2)>,
    clone_anchor: Option<Point>,
    clone_offset: Option<(i64, i64)>,
    zoom_drag: Option<(PointerButton, egui::Pos2, f32, bool)>,
    image: Option<TextureHandle>,
    tab_thumb: Option<TextureHandle>,
    layer_thumbs: Vec<Option<TextureHandle>>,
    thumbs_dirty: bool,
    selection_gesture: Option<SelectionGesture>,
    selection_path: Vec<Point>,
    stroke_gesture: Option<StrokeGesture>,
    pan_gesture: Option<PanGesture>,
    opacity_edit: Option<(usize, LayerMeta)>,
}

impl DocumentTab {
    fn new(document: Document, name: String, project_path: Option<PathBuf>) -> Self {
        Self {
            editor: Editor::new(document),
            name,
            project_path,
            zoom: None,
            display_zoom: 1.0,
            view: View::default(),
            session: None,
            session_creation: None,
            clone_anchor: None,
            clone_offset: None,
            zoom_drag: None,
            image: None,
            tab_thumb: None,
            layer_thumbs: Vec::new(),
            thumbs_dirty: true,
            selection_gesture: None,
            selection_path: Vec::new(),
            stroke_gesture: None,
            pan_gesture: None,
            opacity_edit: None,
        }
    }

    fn title(&self) -> &str {
        if self.name.is_empty() {
            i18n::logical_text("untitled")
        } else {
            &self.name
        }
    }

    fn is_untouched_default(&self) -> bool {
        !self.editor.has_edits() && self.project_path.is_none() && self.name.is_empty()
    }

    fn clear_gestures(&mut self) {
        self.editor.finish_stroke();
        self.selection_gesture = None;
        self.selection_path.clear();
        self.stroke_gesture = None;
        self.zoom_drag = None;
        self.pan_gesture = None;
    }
}

struct App {
    locale: i18n::Locale,
    tabs: Vec<DocumentTab>,
    active_tab: usize,
    shortcuts: Shortcuts,
    shortcut_dialog: Option<ShortcutDialog>,
    recent: RecentFiles,
    clipboard: clipboard::Clipboard,
    size_dialog: Option<SizeDialog>,
    save_dialog: Option<SaveOptionsDialog>,
    layer_dialog: Option<LayerPropertiesDialog>,
    palette: ColorPalette,
    colors_open: bool,
    history_open: bool,
    diameter: u32,
    pencil_diameter: u32,
    hardness: u8,
    spacing: u8,
    fill_pattern: FillPattern,
    stroke_blend: StrokeBlend,
    tool: Tool,
    last_paint_tool: Tool,
    tool_size_popup: bool,
    selection_combine: Combine,
    selection_feather: u8,
    color_tolerance: u8,
    alpha_match: AlphaMatchMode,
    wand_contiguous: bool,
    wand_composite: bool,
    bucket_contiguous: bool,
    bucket_composite: bool,
    recolor_target: Option<Pixel>,
    eyedropper_composite: bool,
    eyedropper_stay: bool,
    // Per-tool options. `stroke_blend`, `fill_pattern` and `shape_thickness`
    // are shared between tools and stay the source of truth for those fields:
    // read these through `*_params()` so the shared values are applied.
    gradient: GradientParams,
    line_curve: LineCurveParams,
    shape: ShapeParams,
    text: TextParams,
    shape_thickness: u32,
    font_catalog: FontCatalog,
    settings: GlobalSettings,
    checker: TextureHandle,
    notice: Option<(String, bool)>,
    error_flash: Option<flash::Flash>,
    pending_close: Option<PendingClose>,
    new_size: Option<(u32, u32)>,
    allow_close: bool,
    fullscreen: bool,
    observed_fullscreen: Option<bool>,
    color_popup_rect: Option<Rect>,
    layers_clicked: bool,
}

impl App {
    #[cfg(test)]
    fn new(ctx: &egui::Context, document: Document) -> Self {
        Self::with_settings(ctx, document, GlobalSettings::default())
    }

    fn with_settings(ctx: &egui::Context, document: Document, settings: GlobalSettings) -> Self {
        settings.theme.apply(ctx);
        let light = Color32::from_gray(215);
        let dark = Color32::from_gray(172);
        let checker = ctx.load_texture(
            tr("transparency"),
            ColorImage::new([2, 2], vec![light, dark, dark, light]),
            TextureOptions {
                wrap_mode: TextureWrapMode::Repeat,
                ..TextureOptions::NEAREST
            },
        );
        Self {
            locale: i18n::Locale::default(),
            tabs: vec![DocumentTab::new(document, String::new(), None)],
            active_tab: 0,
            shortcuts: Shortcuts::default(),
            shortcut_dialog: None,
            recent: RecentFiles::default(),
            clipboard: clipboard::Clipboard::new(),
            size_dialog: None,
            save_dialog: None,
            layer_dialog: None,
            palette: ColorPalette::new(DEFAULT_PRIMARY, DEFAULT_SECONDARY),
            colors_open: false,
            history_open: true,
            diameter: 12,
            pencil_diameter: 1,
            hardness: 100,
            spacing: 15,
            fill_pattern: FillPattern::Solid,
            stroke_blend: StrokeBlend::Normal,
            tool: Tool::Brush,
            last_paint_tool: Tool::Brush,
            tool_size_popup: false,
            selection_combine: Combine::Replace,
            selection_feather: 0,
            color_tolerance: 25,
            alpha_match: AlphaMatchMode::Premultiplied,
            wand_contiguous: true,
            wand_composite: false,
            bucket_contiguous: true,
            bucket_composite: false,
            recolor_target: None,
            eyedropper_composite: false,
            eyedropper_stay: true,
            gradient: GradientParams::default(),
            line_curve: LineCurveParams::default(),
            shape: ShapeParams::default(),
            text: TextParams::default(),
            shape_thickness: LineCurveParams::default().thickness,
            font_catalog: FontCatalog::new(),
            settings,
            checker,
            notice: None,
            error_flash: None,
            pending_close: None,
            new_size: None,
            allow_close: false,
            fullscreen: false,
            observed_fullscreen: None,
            color_popup_rect: None,
            layers_clicked: false,
        }
    }

    fn tab(&self) -> &DocumentTab {
        &self.tabs[self.active_tab]
    }

    fn tab_mut(&mut self) -> &mut DocumentTab {
        &mut self.tabs[self.active_tab]
    }

    fn finish_opacity(&mut self) {
        let tab = self.tab_mut();
        if let Some((layer, before)) = tab.opacity_edit.take() {
            tab.editor.record_layer_meta(layer, before, "opacity");
        }
    }

    fn switch_tab(&mut self, index: usize) {
        if index >= self.tabs.len() || index == self.active_tab {
            return;
        }
        self.finish_opacity();
        let tab = self.tab_mut();
        tab.editor.finish_stroke();
        tab.pan_gesture = None;
        self.active_tab = index;
        self.notice = None;
    }

    fn cycle_tab(&mut self, delta: isize) {
        if self.tabs.len() <= 1 {
            return;
        }
        let len = self.tabs.len() as isize;
        let next = (self.active_tab as isize + delta).rem_euclid(len) as usize;
        self.switch_tab(next);
    }

    fn reorder_tab(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return;
        }
        let active = self.active_tab;
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.active_tab = if active == from {
            to
        } else if from < active && active <= to {
            active - 1
        } else if to <= active && active < from {
            active + 1
        } else {
            active
        };
    }

    fn open_in_new_tab(&mut self, document: Document, name: String, path: Option<PathBuf>) {
        self.finish_opacity();
        self.tab_mut().editor.finish_stroke();
        let tab = DocumentTab::new(document, name, path);
        if self.tabs.len() == 1 && self.tabs[0].is_untouched_default() {
            self.tabs[0] = tab;
            self.active_tab = 0;
        } else {
            self.tabs.push(tab);
            self.active_tab = self.tabs.len() - 1;
        }
        self.notice = None;
    }

    fn close_tab_at(&mut self, index: usize) {
        if self.tabs.len() <= 1 {
            match new_document(DEFAULT_SIZE.0, DEFAULT_SIZE.1) {
                Ok(document) => {
                    self.tabs[0] = DocumentTab::new(document, String::new(), None);
                    self.active_tab = 0;
                    self.notice = None;
                }
                Err(error) => self.report(Err(error), ""),
            }
            return;
        }
        self.tabs.remove(index);
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        } else if index < self.active_tab {
            self.active_tab -= 1;
        }
        self.notice = None;
    }

    fn flush_edits(&mut self) {
        self.finish_opacity();
        self.tab_mut().editor.finish_stroke();
        self.commit_active_session();
    }

    fn finish_change(&mut self, result: io::Result<bool>) {
        match result {
            Ok(changed) => self.tab_mut().thumbs_dirty |= changed,
            Err(error) => self.report(Err(error), ""),
        }
    }

    pub(super) fn toggle_layer_visibility(&mut self, layer: usize) {
        self.finish_opacity();
        let Some(before) = self.tab().editor.document.layer_properties(layer) else {
            return;
        };
        let result = self
            .tab_mut()
            .editor
            .set_properties(layer, (!before.0, before.1));
        if result.is_ok() {
            self.tab_mut().editor.record_properties(layer, before);
        }
        self.finish_geometry(result);
    }

    fn mark_project_saved(&mut self, path: PathBuf, update_project_path: bool) {
        let tab = self.tab_mut();
        tab.name = display_name(&path);
        if update_project_path {
            tab.project_path = Some(path.clone());
        }
        tab.editor.mark_saved();
        self.recent.push(path);
    }

    fn prompt_save_options(
        &mut self,
        ctx: &egui::Context,
        title: &str,
        filter_label: &str,
        ext: &str,
        bad_ext_key: &str,
        format: SaveFormat,
    ) -> bool {
        let base = Path::new(self.tab().title())
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_else(|| self.tab().title())
            .to_owned();
        if let Some(mut path) = FileDialog::new()
            .set_title(title)
            .add_filter(filter_label, &[ext])
            .set_file_name(format!("{base}.{ext}"))
            .save_file()
        {
            let extension_added = path.extension().is_none();
            if extension_added {
                path.set_extension(ext);
            }
            if !has_extension(&path, ext) {
                self.report(
                    Err(io::Error::new(io::ErrorKind::InvalidInput, tr(bad_ext_key))),
                    "",
                );
                return false;
            }
            let needs_confirm = extension_added && path.exists();
            self.save_dialog = Some(SaveOptionsDialog::new(
                path,
                format,
                needs_confirm,
                &self.tab().editor.document,
                ctx,
            ));
        }
        false
    }

    fn finish_empty_close(&mut self, ctx: &egui::Context, scope: CloseScope) {
        if scope == CloseScope::All {
            self.tabs.truncate(1);
            self.active_tab = 0;
            self.close_tab_at(0);
        } else {
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn request(&mut self, ctx: &egui::Context, action: Action) {
        self.flush_edits();
        match action {
            Action::New | Action::Open => self.perform(ctx, action),
            Action::CloseDocument => self.begin_close_flow(ctx, CloseScope::Single),
            Action::CloseAll => self.begin_close_flow(ctx, CloseScope::All),
            Action::Close => self.begin_close_flow(ctx, CloseScope::Exit),
        }
    }

    fn begin_close_flow(&mut self, ctx: &egui::Context, scope: CloseScope) {
        match scope {
            CloseScope::Single => {
                if self.tab().editor.has_unsaved_edits() {
                    self.pending_close = Some(PendingClose {
                        scope,
                        tab_index: self.active_tab,
                        waiting_save_dialog: false,
                    });
                } else {
                    self.close_tab_at(self.active_tab);
                }
            }
            CloseScope::All | CloseScope::Exit => {
                if let Some(dirty_idx) = self
                    .tabs
                    .iter()
                    .position(|tab| tab.editor.has_unsaved_edits())
                {
                    self.switch_tab(dirty_idx);
                    self.pending_close = Some(PendingClose {
                        scope,
                        tab_index: dirty_idx,
                        waiting_save_dialog: false,
                    });
                } else {
                    self.finish_empty_close(ctx, scope);
                }
            }
        }
    }

    fn advance_close_flow(&mut self, ctx: &egui::Context, scope: CloseScope, closed_idx: usize) {
        match scope {
            CloseScope::Single => {
                self.close_tab_at(closed_idx);
            }
            CloseScope::All | CloseScope::Exit => {
                if scope == CloseScope::All || self.tabs.len() > 1 {
                    self.close_tab_at(closed_idx);
                    if self.tabs.len() == 1
                        && self.tabs[0].is_untouched_default()
                        && scope == CloseScope::All
                    {
                        return;
                    }
                    if scope == CloseScope::All
                        && !self.tabs.iter().any(|t| t.editor.has_unsaved_edits())
                    {
                        self.finish_empty_close(ctx, CloseScope::All);
                        return;
                    }
                } else {
                    self.finish_empty_close(ctx, CloseScope::Exit);
                    return;
                }
                self.begin_close_flow(ctx, scope);
            }
        }
    }

    fn perform(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::New => self.new_size = Some(DEFAULT_SIZE),
            Action::Open => {
                if let Some(path) = pick_image_file(tr("open")) {
                    self.open_path(ctx, path);
                }
            }
            Action::CloseDocument | Action::CloseAll | Action::Close => {
                self.request(ctx, action);
            }
        }
    }

    fn open_path(&mut self, ctx: &egui::Context, path: PathBuf) {
        let result = (|| {
            let (document, pdn) = open_document(&path)?;
            let (width, height) = document.dimensions();
            let limit = max_texture_side(ctx);
            if (width.max(height) as usize) > limit {
                return Err(io::Error::other(i18n::format("canvas-limit", &[&limit])));
            }
            let name = display_name(&path);
            let stored_path = pdn.then(|| path.clone());
            self.recent.push(path);
            self.open_in_new_tab(document, name, stored_path);
            Ok(())
        })();
        self.report(result, tr("file-opened"));
    }

    fn import_layer_from_file(&mut self, ctx: &egui::Context) {
        if let Some(path) = pick_image_file(tr("import-layer-file")) {
            let result = (|| {
                let (imported, _) = open_document(&path)?;
                let cur = self.tab().editor.document.dimensions();
                let imp = imported.dimensions();
                let limit = max_texture_side(ctx);
                if (cur.0.max(imp.0).max(cur.1.max(imp.1)) as usize) > limit {
                    return Err(io::Error::other(i18n::format("canvas-limit", &[&limit])));
                }
                self.tab_mut().editor.import_document_layers(imported)
            })();
            self.finish_geometry(result);
        }
    }

    fn open_layer_properties(&mut self, layer: usize) {
        self.finish_opacity();
        self.tab_mut().editor.finish_stroke();
        if let Some(meta) = self.tab().editor.layer_meta(layer) {
            self.layer_dialog = Some(LayerPropertiesDialog::new(layer, meta));
        }
    }

    fn save_project(&mut self, ctx: &egui::Context, save_as: bool) -> bool {
        self.flush_edits();
        if !save_as && let Some(path) = self.tab().project_path.clone() {
            let result = write_pdn_atomically(&self.tab().editor.document, &path, true);
            let saved = result.is_ok();
            if saved {
                self.mark_project_saved(path, false);
            }
            self.report(result, tr("project-saved"));
            return saved;
        }
        self.prompt_save_options(
            ctx,
            tr("save-as"),
            tr("pdn-document"),
            PDN_EXT,
            "use-pdn",
            SaveFormat::pdn(),
        )
    }

    fn export(&mut self, ctx: &egui::Context) {
        self.flush_edits();
        self.prompt_save_options(
            ctx,
            tr("export-png"),
            tr("png-image"),
            "png",
            "use-png",
            SaveFormat::png(),
        );
    }

    fn report(&mut self, result: io::Result<()>, success: &str) {
        match result {
            Ok(()) if success.is_empty() => self.notice = None,
            Ok(()) => self.notice = Some((success.into(), false)),
            Err(error) => self.show_error(i18n::error_message(&error)),
        }
    }

    /// Alerts the user: shows an error in the status bar and flashes it. Every
    /// error goes through here.
    fn show_error(&mut self, message: String) {
        self.notice = Some((message, true));
        self.error_flash = Some(flash::Flash::default());
    }

    fn step_history(&mut self, redo: bool) {
        self.finish_opacity();
        self.commit_active_session();
        let result = if redo {
            self.tab_mut().editor.redo()
        } else {
            self.tab_mut().editor.undo()
        };
        self.finish_change(result);
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        use Command::*;
        self.shortcut_item(ui, New, true);
        self.shortcut_item(ui, Open, true);
        ui.menu_button(tr("open-recent"), |ui| {
            let entries = self.recent.entries.clone();
            if entries.is_empty() {
                ui.weak(tr("no-recent"));
            } else {
                for path in entries {
                    let label = display_name(&path);
                    if ui
                        .button(&label)
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        self.open_path(ui.ctx(), path);
                        ui.close();
                    }
                }
            }
            ui.separator();
            if ui
                .add_enabled(
                    !self.recent.entries.is_empty(),
                    egui::Button::new(tr("clear-recent")),
                )
                .clicked()
            {
                self.recent.clear();
                ui.close();
            }
        });
        ui.separator();
        self.shortcut_item(ui, Save, true);
        self.shortcut_item(ui, SaveAs, true);
        self.shortcut_item(ui, Export, true);
        ui.separator();
        self.shortcut_item(ui, Close, true);
        self.shortcut_item(ui, CloseAll, true);
        ui.separator();
        self.shortcut_item(ui, Exit, true);
    }

    fn save_settings(&mut self) {
        self.notice = None;
        let error = self.locale.with(|| {
            self.settings
                .save()
                .err()
                .map(|error| i18n::format("settings-save-error", &[&error]))
        });
        if let Some(message) = error {
            self.show_error(message);
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            let previous = self.settings.theme;
            let close = window_chrome::controls(ui, &mut self.settings.theme, self.fullscreen);
            if self.settings.theme != previous {
                self.save_settings();
            }
            if close {
                self.request(ui.ctx(), Action::Close);
            }
            ui.with_layout(egui::Layout::left_to_right(Align::Center), |ui| {
                let menus_width = ui.available_width() - 24.0;
                egui::ScrollArea::horizontal()
                    .id_salt("main_menu_scroll")
                    .max_width(menus_width)
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| self.main_menus(ui));
                window_chrome::drag_space(ui, self.fullscreen);
            });
        });
    }

    fn main_menus(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new()
            .style(Theme::menu_bar_style as fn(&mut egui::Style))
            .config(
                egui::containers::menu::MenuConfig::new()
                    .style(Theme::menu_popup_style as fn(&mut egui::Style)),
            )
            .ui(ui, |ui| {
                tool_icons::paint_logo(ui);
                ui.add_space(4.0);
                ui.label(
                    RichText::new(APP_NAME)
                        .strong()
                        .color(self.settings.theme.text_primary())
                        .size(13.5),
                );
                ui.add_space(8.0);
                ui.menu_button(tr("file"), |ui| self.file_menu(ui));
                self.phase2_menus(ui);
                ui.menu_button(tr("selection"), |ui| {
                    self.shortcut_item(ui, Command::SelectAll, true);
                    let selected = self.tab().editor.selection.is_some();
                    self.shortcut_item(ui, Command::Deselect, selected);
                    self.shortcut_item(ui, Command::Invert, true);
                    ui.separator();
                    self.shortcut_item(ui, Command::Fill, selected);
                    self.shortcut_item(ui, Command::Erase, selected);
                });
                ui.menu_button(tr("settings"), |ui| {
                    self.shortcut_item(ui, Command::Settings, true);
                    ui.menu_button(i18n::text("language"), |ui| {
                        for locale in i18n::Locale::all() {
                            if ui
                                .selectable_label(self.locale == locale, locale.name())
                                .clicked()
                            {
                                if self.locale.prefers_simplified_chinese()
                                    != locale.prefers_simplified_chinese()
                                {
                                    ui.ctx().set_fonts(i18n::fonts(locale));
                                    self.font_catalog = FontCatalog::for_locale(locale);
                                }
                                self.locale = locale;
                                self.notice = None;
                                let error = locale.with(|| {
                                    locale
                                        .save()
                                        .err()
                                        .map(|error| i18n::format("language-save-error", &[&error]))
                                });
                                if let Some(message) = error {
                                    self.show_error(message);
                                }
                                ui.close();
                                ui.ctx().request_repaint();
                            }
                        }
                    });
                    ui.menu_button(tr("antialiasing"), |ui| {
                        for mode in AaMode::ALL {
                            if ui
                                .selectable_label(self.settings.antialiasing == mode, mode.label())
                                .clicked()
                            {
                                if self.settings.antialiasing != mode {
                                    self.settings.antialiasing = mode;
                                    self.save_settings();
                                    self.tab_mut().editor.mark_full_refresh();
                                }
                                ui.close();
                                ui.ctx().request_repaint();
                            }
                        }
                    });
                });
                window_chrome::drag_space(ui, self.fullscreen);
            });
    }

    fn command_icon_button(
        &self,
        ui: &mut egui::Ui,
        icon: tool_icons::ActionIcon,
        enabled: bool,
        label_key: &str,
        command: Command,
    ) -> bool {
        tool_icons::icon_button(
            ui,
            self.settings.theme,
            icon,
            enabled,
            false,
            vec2(26.0, 26.0),
            &self.shortcuts.tooltip(label_key, command),
        )
        .clicked()
    }

    fn command_bar(&mut self, ui: &mut egui::Ui) {
        use tool_icons::{ActionIcon, icon_button};
        let compact = ui.available_width() < 850.0;
        let wrap_wand = matches!(self.tool, Tool::Wand | Tool::Bucket | Tool::Recolor)
            && !self.has_active_session()
            && ui.available_width() < 1120.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let btn_sz = vec2(26.0, 26.0);
            if self.command_icon_button(ui, ActionIcon::NewDoc, true, "new", Command::New) {
                self.perform(ui.ctx(), Action::New);
            }
            if self.command_icon_button(ui, ActionIcon::OpenFolder, true, "open", Command::Open) {
                self.perform(ui.ctx(), Action::Open);
            }
            if self.command_icon_button(ui, ActionIcon::SaveDisk, true, "save", Command::Save) {
                self.save_project(ui.ctx(), false);
            }
            if icon_button(
                ui,
                self.settings.theme,
                ActionIcon::ExportPng,
                true,
                false,
                btn_sz,
                tr("export-options"),
            )
            .clicked()
            {
                self.export(ui.ctx());
            }

            ui.separator();
            if self.command_icon_button(
                ui,
                ActionIcon::Undo,
                self.tab().editor.has_edits(),
                "undo",
                Command::Undo,
            ) {
                self.step_history(false);
            }
            if self.command_icon_button(
                ui,
                ActionIcon::Redo,
                self.tab().editor.can_redo(),
                "redo",
                Command::Redo,
            ) {
                self.step_history(true);
            }

            ui.separator();
            let theme = self.settings.theme;
            match self.tab().session.as_ref() {
                Some(ActiveSession::Transform(t)) => {
                    let (target, mut method, shift, scale, degrees) = (
                        t.target,
                        t.method,
                        t.affine.shift,
                        t.affine.scale,
                        t.affine.degrees,
                    );
                    ui.label(theme.muted_label(tr("transform-field")));
                    ui.label(theme.section_label(target.short_label()));
                    if target != TransformTarget::Mask {
                        let prev = method;
                        phase2::resampling_selector(ui, &mut method);
                        if method != prev {
                            let tab = self.tab_mut();
                            if let Some(transform) = tab
                                .session
                                .as_mut()
                                .and_then(ActiveSession::as_transform_mut)
                            {
                                transform.method = method;
                            }
                            tab.editor.mark_full_refresh();
                        }
                    }
                    ui.label(theme.muted_label(format!(
                        "Δ({:.0}, {:.0}) px · {:.0}%×{:.0}% · {:.1}°",
                        shift.0,
                        shift.1,
                        scale.0 * 100.0,
                        scale.1 * 100.0,
                        degrees
                    )));
                    self.session_commit_buttons(ui);
                }
                Some(ActiveSession::Gradient(g)) => {
                    let mut params = g.params;
                    ui.label(theme.section_label(tr("gradient-field")));
                    if Self::gradient_controls(ui, &mut params) {
                        self.set_gradient_params(params);
                        self.update_session(|session| {
                            if let ActiveSession::Gradient(g) = session {
                                g.params = params;
                            }
                        });
                    }
                    self.session_commit_buttons(ui);
                }
                Some(ActiveSession::LineCurve(lc)) => {
                    let mut params = lc.params;
                    ui.label(theme.section_label(tr("line-curve-field")));
                    if Self::line_curve_controls(ui, &mut params) {
                        self.set_line_curve_params(params);
                        self.update_session(|session| {
                            if let ActiveSession::LineCurve(lc) = session {
                                lc.params = params;
                            }
                        });
                    }
                    self.session_commit_buttons(ui);
                }
                Some(ActiveSession::Shape(s)) => {
                    let mut params = s.params;
                    ui.label(theme.section_label(tr("shape-field")));
                    if Self::shape_controls(ui, &mut params) {
                        self.set_shape_params(params);
                        self.update_session(|session| {
                            if let ActiveSession::Shape(s) = session {
                                s.params = params;
                            }
                        });
                    }
                    self.session_commit_buttons(ui);
                }
                Some(ActiveSession::Text(t)) => {
                    let mut text = t.text.clone();
                    let mut params = t.params.clone();
                    ui.label(theme.section_label(tr("text-field")));
                    let text_resp = ui.add(
                        egui::TextEdit::singleline(&mut text)
                            .desired_width(120.0)
                            .hint_text(tr("placeholder-text")),
                    );
                    let changed = text_resp.changed()
                        | Self::text_controls(ui, &self.font_catalog.families, &mut params);
                    if changed {
                        self.set_text_params(params.clone());
                        self.update_session(|session| {
                            if let Some(t) = session.as_text_mut() {
                                t.text = text;
                                t.params = params;
                            }
                        });
                    }
                    self.session_commit_buttons(ui);
                }
                Some(ActiveSession::CanvasResize(cr)) => {
                    let (original, mut preview_size) = (cr.original, cr.preview_size);
                    let limit = max_canvas_side(ui.ctx());
                    ui.label(theme.section_label(tr("canvas-size-field")));
                    let (wc, hc) = dimension_inputs(
                        ui,
                        &mut preview_size.0,
                        &mut preview_size.1,
                        limit,
                        false,
                    );
                    if (wc || hc)
                        && let Some(cr) = self
                            .tab_mut()
                            .session
                            .as_mut()
                            .and_then(ActiveSession::as_canvas_resize_mut)
                    {
                        cr.preview_size = preview_size;
                    }
                    ui.label(
                        RichText::new(i18n::format(
                            "canvas-resize-hint",
                            &[&original.0, &original.1],
                        ))
                        .size(11.5)
                        .color(theme.text_muted()),
                    );
                    self.session_commit_buttons(ui);
                }
                None => self.tool_options(ui, compact, wrap_wand),
            }
        });
        if wrap_wand {
            ui.horizontal(|ui| self.wrapped_tool_options(ui));
        }
    }

    fn workspace_tabs(&mut self, ui: &mut egui::Ui) {
        let mut chosen = None;
        let mut close_idx = None;
        let mut reorder: Option<(usize, usize)> = None;
        let mut add_new = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let many = self.tabs.len() > 1;
            if many {
                if ui
                    .add(egui::Button::new("◀").frame(false))
                    .on_hover_text(tr("previous-document"))
                    .clicked()
                {
                    self.cycle_tab(-1);
                }
                if ui
                    .add(egui::Button::new("▶").frame(false))
                    .on_hover_text(tr("next-document"))
                    .clicked()
                {
                    self.cycle_tab(1);
                }
            }
            egui::ScrollArea::horizontal()
                .id_salt("open_documents")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for index in 0..self.tabs.len() {
                            let active = index == self.active_tab;
                            let (title, dirty, thumb_id, thumb_size) = {
                                let tab = &self.tabs[index];
                                let title = if tab.editor.has_unsaved_edits() {
                                    format!("{} *", tab.title())
                                } else {
                                    tab.title().to_owned()
                                };
                                let (tid, tsz) = tab
                                    .tab_thumb
                                    .as_ref()
                                    .map_or((None, Vec2::ZERO), |t| (Some(t.id()), t.size_vec2()));
                                (
                                    i18n::display(&title).into_owned(),
                                    tab.editor.has_unsaved_edits(),
                                    tid,
                                    tsz,
                                )
                            };
                            let frame = if active {
                                egui::Frame::new()
                                    .fill(self.settings.theme.card_fill())
                                    .stroke(egui::Stroke::new(
                                        1.0_f32,
                                        self.settings.theme.border_color(),
                                    ))
                                    .corner_radius(egui::CornerRadius {
                                        nw: 6,
                                        ne: 6,
                                        sw: 2,
                                        se: 2,
                                    })
                                    .inner_margin(egui::Margin::symmetric(8, 3))
                            } else {
                                egui::Frame::new()
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE)
                                    .corner_radius(6)
                                    .inner_margin(egui::Margin::symmetric(8, 3))
                            };
                            let tab_resp = frame
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 5.0;
                                        if let Some(tid) = thumb_id {
                                            let aspect = (thumb_size.x / thumb_size.y.max(1.0))
                                                .clamp(0.5, 1.5);
                                            let sz = vec2(15.0 * aspect, 15.0);
                                            let (r, _) =
                                                ui.allocate_exact_size(sz, egui::Sense::hover());
                                            ui.painter().image(
                                                tid,
                                                r,
                                                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                                                Color32::WHITE,
                                            );
                                            ui.painter().rect_stroke(
                                                r,
                                                2.0,
                                                egui::Stroke::new(
                                                    0.8_f32,
                                                    self.settings.theme.border_color(),
                                                ),
                                                egui::StrokeKind::Outside,
                                            );
                                        }
                                        let label =
                                            RichText::new(title).size(12.5).color(if active {
                                                self.settings.theme.text_primary()
                                            } else {
                                                self.settings.theme.text_muted()
                                            });
                                        let label_resp = ui
                                            .add(egui::Button::new(label).frame(false))
                                            .on_hover_text(if dirty {
                                                tr("unsaved-drag-reorder")
                                            } else {
                                                tr("drag-reorder")
                                            });
                                        if label_resp.clicked() {
                                            chosen = Some(index);
                                        }
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new("×")
                                                        .size(13.0)
                                                        .color(self.settings.theme.text_muted()),
                                                )
                                                .frame(false),
                                            )
                                            .on_hover_text(tr("close-document"))
                                            .clicked()
                                        {
                                            close_idx = Some(index);
                                        }
                                        label_resp
                                    })
                                    .inner
                                })
                                .inner;
                            if let Some(pair) =
                                dnd_reorder(ui, tab_resp.rect, ("tab_dnd", index), index)
                            {
                                reorder = Some(pair);
                            }
                        }
                        if tool_icons::icon_button(
                            ui,
                            self.settings.theme,
                            tool_icons::ActionIcon::Plus,
                            true,
                            false,
                            vec2(22.0, 22.0),
                            tr("new-document"),
                        )
                        .clicked()
                        {
                            add_new = true;
                        }
                    });
                });
        });
        if add_new {
            self.perform(ui.ctx(), Action::New);
        } else if let Some((from, to)) = reorder {
            self.reorder_tab(from, to);
        } else if let Some(idx) = close_idx {
            self.switch_tab(idx);
            self.request(ui.ctx(), Action::CloseDocument);
        } else if let Some(index) = chosen {
            self.switch_tab(index);
        }
    }

    fn tool_rail(&mut self, ui: &mut egui::Ui) {
        let mut clicked_tool: Option<Tool> = None;
        let mut run_cmd: Option<Command> = None;
        let tools = [
            Tool::Select(Shape::Rectangle),
            Tool::Lasso,
            Tool::Select(Shape::Ellipse),
            Tool::Wand,
            Tool::Pan,
            Tool::Zoom,
            Tool::Bucket,
            Tool::Gradient,
            Tool::Brush,
            Tool::Eraser,
            Tool::Pencil,
            Tool::Eyedropper,
            Tool::CloneStamp,
            Tool::Recolor,
            Tool::Text,
            Tool::LineCurve,
            Tool::DrawShape,
        ];
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            if tool_icons::move_button(
                ui,
                self.settings.theme,
                self.tab()
                    .session
                    .as_ref()
                    .is_some_and(ActiveSession::is_transform),
            )
            .clicked()
            {
                run_cmd = Some(Command::MovePixels);
            }
            ui.separator();
            egui::Grid::new("tool_grid")
                .num_columns(2)
                .spacing(vec2(4.0, 4.0))
                .show(ui, |ui| {
                    for (idx, tool) in tools.into_iter().enumerate() {
                        let resp =
                            tool_icons::button(ui, self.settings.theme, tool, self.tool == tool);
                        let double_action = match tool {
                            Tool::Pan => {
                                self.shortcuts.mouse.click(&resp, Scope::ToolbarHand, true)
                            }
                            Tool::Zoom => {
                                self.shortcuts.mouse.click(&resp, Scope::ToolbarZoom, true)
                            }
                            _ => None,
                        };
                        match double_action {
                            Some(MouseAction::ZoomFit) => run_cmd = Some(Command::Fit),
                            Some(MouseAction::ZoomActual) => run_cmd = Some(Command::ActualSize),
                            _ if resp.clicked() => clicked_tool = Some(tool),
                            _ => {}
                        }
                        if idx % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
        });
        if let Some(cmd) = run_cmd {
            self.execute_command(ui.ctx(), cmd);
        } else if let Some(tool) = clicked_tool {
            self.activate_tool(tool);
        }
    }

    fn color_dropdown_popup(&mut self, ctx: &egui::Context, ribbon: colors::RibbonOutput) {
        if !self.colors_open {
            self.color_popup_rect = None;
            return;
        }
        let card_w = if self.palette.expanded { 242.0 } else { 214.0 };
        let screen = ctx.content_rect();
        let x = (ribbon.chevron_rect.right() - card_w)
            .min(screen.right() - card_w - 4.0)
            .max(4.0);
        let y = (ribbon.chevron_rect.bottom() + 6.0).min((screen.bottom() - 80.0).max(4.0));
        let theme = self.settings.theme;
        let eraser_active = self.tool == Tool::Eraser;
        let checker = self.checker.clone();
        let mut palette_action = PaletteAction::None;
        let area_resp = egui::Area::new(egui::Id::new("color_card_dropdown"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(x, y))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(theme.card_fill())
                    .stroke(egui::Stroke::new(1.0_f32, theme.border_color()))
                    .corner_radius(CARD_CORNER_RADIUS)
                    .shadow(ctx.style().visuals.popup_shadow)
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width(card_w - 20.0);
                        ui.add_enabled_ui(!eraser_active, |ui| {
                            palette_action = colors::picker(
                                ui,
                                &mut self.palette,
                                &self.shortcuts.mouse,
                                theme,
                                &checker,
                            );
                        });
                    });
            });
        let popup_rect = area_resp.response.rect;
        self.color_popup_rect = Some(popup_rect);

        if !ribbon.toggle_clicked
            && !egui::Popup::is_any_open(ctx)
            && ctx.input(|i| i.pointer.any_pressed())
            && let Some(pos) = ctx.input(|i| i.pointer.interact_pos())
        {
            let inside = popup_rect.contains(pos) || ribbon.rect.contains(pos);
            if !inside {
                self.colors_open = false;
                self.color_popup_rect = None;
                ctx.request_repaint();
            }
        }

        match palette_action {
            PaletteAction::SavePalette => {
                if let Some(mut path) = FileDialog::new()
                    .set_title(tr("save-file"))
                    .add_filter(tr("paint-net-palette"), &["txt"])
                    .set_file_name(i18n::logical_text("palette-export-name"))
                    .save_file()
                {
                    if path.extension().is_none() {
                        path.set_extension("txt");
                    }
                    let res = self.palette.save_to(&path);
                    self.report(res, tr("palette-saved"));
                }
            }
            PaletteAction::LoadPalette => {
                if let Some(path) = FileDialog::new()
                    .set_title(tr("load-file"))
                    .add_filter(tr("paint-net-palette"), &["txt"])
                    .pick_file()
                {
                    let res = self.palette.load_from(&path);
                    self.report(res, tr("palette-loaded"));
                }
            }
            PaletteAction::None => {}
        }
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_height();
        let layers_height = if self.history_open {
            (available * 0.57).round()
        } else {
            (available - 56.0).max(160.0)
        };
        self.layers(ui, layers_height);
        self.history(ui);
    }

    fn layers(&mut self, ui: &mut egui::Ui, height: f32) {
        use tool_icons::{ActionIcon, icon_button};
        let theme = self.settings.theme;
        let mut add_layer = false;
        let list_max = (height - 135.0).max(48.0);
        let mut move_pair: Option<(usize, usize)> = None;
        let mut double_clicked_layer = None;
        let mut action_cmd: Option<Command> = None;
        let mut open_props: Option<usize> = None;

        let card = self.settings.theme.styled_card(
            ui,
            tr("layers"),
            |ui| {
                if icon_button(
                    ui,
                    theme,
                    ActionIcon::NewLayer,
                    true,
                    false,
                    vec2(20.0, 20.0),
                    tr("add-layer"),
                )
                .clicked()
                {
                    add_layer = true;
                }
            },
            |ui| {
                let selected = self.tab().editor.selected;
                let meta = self.tab().editor.layer_meta(selected).unwrap();
                let mut blend = meta.blend;
                ui.horizontal(|ui| {
                    let blend_width = 96.0;
                    if blend_mode_combo(
                        ui,
                        egui::ComboBox::from_id_salt("layer_blend_mode").width(blend_width),
                        &mut blend,
                    ) {
                        self.finish_opacity();
                        let mut next = meta.clone();
                        next.blend = blend;
                        let result = self.tab_mut().editor.set_layer_meta(selected, next);
                        if result.is_ok() {
                            self.tab_mut().editor.record_layer_meta(
                                selected,
                                meta.clone(),
                                "blend-mode",
                            );
                        }
                        self.finish_geometry(result);
                    }

                    let mut opacity = meta.opacity;
                    ui.spacing_mut().slider_width = (ui.available_width()
                        - blend_width
                        - 46.0
                        - ui.spacing().item_spacing.x * 2.0)
                        .clamp(36.0, 78.0);
                    let response = ui
                        .add(opacity_slider(&mut opacity))
                        .on_hover_text(tr("layer-opacity"));
                    if response.changed() {
                        let meta_before = meta.clone();
                        let tab = self.tab_mut();
                        tab.opacity_edit.get_or_insert((selected, meta_before));
                        let result = tab.editor.set_properties(selected, (meta.visible, opacity));
                        if !ui.input(|input| input.pointer.primary_down()) {
                            self.finish_opacity();
                        }
                        self.report(result, "");
                    }
                });

                let count = self.tab().editor.document.layer_count();
                let selected = self.tab().editor.selected;
                egui::ScrollArea::vertical()
                    .id_salt("layers")
                    .max_height(list_max)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        for layer in (0..count).rev() {
                            let (visible, opacity) =
                                self.tab().editor.document.layer_properties(layer).unwrap();
                            let name = self
                                .tab()
                                .editor
                                .document
                                .layer_name(layer)
                                .unwrap()
                                .to_owned();
                            let blend_id = self
                                .tab()
                                .editor
                                .document
                                .layer_blend_id(layer)
                                .unwrap_or(0);
                            let mode = i18n::blend_name_logical(blend_id);
                            let thumb = self
                                .tab()
                                .layer_thumbs
                                .get(layer)
                                .and_then(|t| t.as_ref())
                                .map(|t| (t.id(), t.size_vec2()));
                            let active = layer == selected;
                            let pill = theme.pill_frame(active, 6, egui::Margin::symmetric(5, 3));
                            let row_resp = pill
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 5.0;
                                        let eye_icon = if visible {
                                            ActionIcon::EyeOpen
                                        } else {
                                            ActionIcon::EyeClosed
                                        };
                                        if icon_button(
                                            ui,
                                            theme,
                                            eye_icon,
                                            true,
                                            false,
                                            vec2(20.0, 20.0),
                                            tr("layer-visibility"),
                                        )
                                        .clicked()
                                        {
                                            self.toggle_layer_visibility(layer);
                                        }
                                        if let Some((tid, tsz)) = thumb {
                                            let aspect = (tsz.x / tsz.y.max(1.0)).clamp(0.5, 1.5);
                                            let sz = vec2(22.0 * aspect, 22.0);
                                            let (rect, _) =
                                                ui.allocate_exact_size(sz, egui::Sense::hover());
                                            let p = ui.painter_at(rect);
                                            paint_thumbnail(
                                                &p,
                                                &self.checker,
                                                tid,
                                                rect,
                                                2.0,
                                                Some(egui::Stroke::new(
                                                    0.8_f32,
                                                    theme.border_color(),
                                                )),
                                                10.0,
                                            );
                                        }
                                        let display = if blend_id != 0 || opacity < 255 {
                                            format!(
                                                "{name} ({mode}, {:.0}%)",
                                                f32::from(opacity) * 100.0 / 255.0
                                            )
                                        } else {
                                            name
                                        };
                                        let label_text =
                                            RichText::new(i18n::display(&display).into_owned())
                                                .size(12.5)
                                                .color(if active {
                                                    theme.pill_active_fg()
                                                } else {
                                                    theme.text_primary()
                                                });
                                        let label_resp = ui
                                            .add(
                                                egui::Button::new(label_text)
                                                    .frame(false)
                                                    .truncate(),
                                            )
                                            .on_hover_text(i18n::format(
                                                "layer-drag-hint",
                                                &[&mode],
                                            ));
                                        if self.shortcuts.mouse.click(
                                            &label_resp,
                                            Scope::LayerRow,
                                            true,
                                        ) == Some(MouseAction::LayerProperties)
                                        {
                                            double_clicked_layer = Some(layer);
                                        } else if label_resp.clicked() {
                                            self.finish_opacity();
                                            self.tab_mut().editor.selected = layer;
                                        }
                                        label_resp
                                    })
                                    .inner
                                })
                                .inner;
                            if let Some(pair) =
                                dnd_reorder(ui, row_resp.rect, ("layer_dnd", layer), layer)
                            {
                                move_pair = Some(pair);
                            }
                        }
                    });

                ui.separator();
                let count = self.tab().editor.document.layer_count();
                let selected = self.tab().editor.selected;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    let sz = vec2(24.0, 24.0);
                    if icon_button(
                        ui,
                        theme,
                        ActionIcon::NewLayer,
                        true,
                        false,
                        sz,
                        tr("add-layer"),
                    )
                    .clicked()
                    {
                        action_cmd = Some(Command::AddLayer);
                    }
                    if icon_button(
                        ui,
                        theme,
                        ActionIcon::Duplicate,
                        true,
                        false,
                        sz,
                        tr("duplicate-layer"),
                    )
                    .clicked()
                    {
                        action_cmd = Some(Command::DuplicateLayer);
                    }
                    if icon_button(
                        ui,
                        theme,
                        ActionIcon::Trash,
                        count > 1,
                        false,
                        sz,
                        tr("delete-layer"),
                    )
                    .clicked()
                    {
                        action_cmd = Some(Command::DeleteLayer);
                    }
                    if icon_button(
                        ui,
                        theme,
                        ActionIcon::MergeDown,
                        selected > 0,
                        false,
                        sz,
                        tr("merge-layer-down"),
                    )
                    .clicked()
                    {
                        action_cmd = Some(Command::MergeLayerDown);
                    }
                    for (icon, scope, enabled, label) in [
                        (
                            ActionIcon::ArrowUp,
                            Scope::LayerUpButton,
                            selected + 1 < count,
                            tr("layer-up"),
                        ),
                        (
                            ActionIcon::ArrowDown,
                            Scope::LayerDownButton,
                            selected > 0,
                            tr("layer-down"),
                        ),
                    ] {
                        let response = icon_button(ui, theme, icon, enabled, false, sz, label);
                        if let Some(command) =
                            match self.shortcuts.mouse.click(&response, scope, false) {
                                Some(MouseAction::LayerUp) => Some(Command::MoveLayerUp),
                                Some(MouseAction::LayerTop) => Some(Command::MoveLayerTop),
                                Some(MouseAction::LayerDown) => Some(Command::MoveLayerDown),
                                Some(MouseAction::LayerBottom) => Some(Command::MoveLayerBottom),
                                _ => None,
                            }
                        {
                            action_cmd = Some(command);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(
                            ui,
                            theme,
                            ActionIcon::Gear,
                            true,
                            false,
                            sz,
                            tr("layer-properties"),
                        )
                        .clicked()
                        {
                            open_props = Some(selected);
                        }
                    });
                });
            },
        );

        let focus = ui.interact(
            card.rect,
            egui::Id::new(LAYERS_ID),
            egui::Sense::focusable_noninteractive(),
        );
        let click = ui.input(|input| {
            input
                .pointer
                .interact_pos()
                .filter(|_| input.pointer.button_clicked(PointerButton::Primary))
        });
        if !self.has_modal_open()
            && !egui::Popup::is_any_open(ui.ctx())
            && click.is_some_and(|pos| {
                card.rect.contains(pos) && ui.ctx().layer_id_at(pos) == Some(ui.layer_id())
            })
            && ui
                .memory(|memory| memory.focused())
                .is_none_or(|id| egui::TextEdit::load_state(ui.ctx(), id).is_none())
        {
            focus.request_focus();
            self.layers_clicked = true;
        } else if !focus.has_focus() {
            self.layers_clicked = false;
        }

        if add_layer {
            self.execute_command(ui.ctx(), Command::AddLayer);
        } else if let Some(cmd) = action_cmd {
            self.execute_command(ui.ctx(), cmd);
        } else if let Some(layer) = open_props {
            self.open_layer_properties(layer);
        } else if let Some((from, to)) = move_pair {
            let result = self.tab_mut().editor.move_layer(from, to, "reorder-layer");
            self.finish_geometry(result);
        } else if let Some(layer) = double_clicked_layer {
            self.open_layer_properties(layer);
        }
    }

    fn history(&mut self, ui: &mut egui::Ui) {
        use tool_icons::{ActionIcon, icon_button};
        let open = self.history_open;
        let mut next_open = open;
        let theme = self.settings.theme;
        let undo_len = self.tab().editor.undo_len();
        let mut target_state: Option<usize> = None;

        self.settings.theme.styled_card(
            ui,
            tr("history"),
            |ui| {
                let icon = if open {
                    ActionIcon::ChevronDown
                } else {
                    ActionIcon::ChevronRight
                };
                if icon_button(
                    ui,
                    theme,
                    icon,
                    true,
                    false,
                    vec2(20.0, 20.0),
                    if open {
                        tr("collapse-history")
                    } else {
                        tr("expand-history")
                    },
                )
                .clicked()
                {
                    next_open = !open;
                }
            },
            |ui| {
                if !open {
                    return;
                }
                egui::ScrollArea::vertical()
                    .id_salt("history")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        let item = |ui: &mut egui::Ui, label: String, active: bool, redo: bool| {
                            let pill = theme.pill_frame(active, 5, egui::Margin::symmetric(6, 3));
                            pill.show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                let color = if active {
                                    theme.pill_active_fg()
                                } else if redo {
                                    theme.text_muted()
                                } else {
                                    theme.text_primary()
                                };
                                ui.add(
                                    egui::Button::new(RichText::new(label).size(12.0).color(color))
                                        .frame(false),
                                )
                                .clicked()
                            })
                            .inner
                        };
                        if item(
                            ui,
                            i18n::display(&format!("0. {}", i18n::logical_text("new-image")))
                                .into_owned(),
                            undo_len == 0,
                            false,
                        ) {
                            target_state = Some(0);
                        }
                        for (index, label) in self.tab().editor.history().enumerate() {
                            let state = index + 1;
                            if item(
                                ui,
                                i18n::display(&format!("{state}. {}", i18n::logical_text(label)))
                                    .into_owned(),
                                undo_len == state,
                                false,
                            ) {
                                target_state = Some(state);
                            }
                        }
                        for (offset, label) in self.tab().editor.redo_history().rev().enumerate() {
                            let state = undo_len + offset + 1;
                            if item(
                                ui,
                                i18n::display(&format!("{state}. {}", i18n::logical_text(label)))
                                    .into_owned(),
                                false,
                                true,
                            ) {
                                target_state = Some(state);
                            }
                        }
                    });
            },
        );
        self.history_open = next_open;
        if let Some(target) = target_state {
            self.finish_opacity();
            self.commit_active_session();
            let result = self.tab_mut().editor.jump_to_state(target);
            self.finish_change(result);
        }
    }

    fn combine_selector(&mut self, ui: &mut egui::Ui) {
        enum_combo(
            ui,
            "selection_combine",
            None,
            &mut self.selection_combine,
            &Combine::ALL,
            Combine::label,
        );
    }

    fn commit_selection_with(
        &mut self,
        selection: io::Result<Selection>,
        mode: Combine,
        label: &'static str,
    ) {
        match selection.and_then(|selection| self.selection_candidate(selection, mode)) {
            Ok(selection) => self.tab_mut().editor.set_selection(Some(selection), label),
            Err(error) => self.report(Err(error), ""),
        }
    }

    fn selection_candidate(&self, selection: Selection, mode: Combine) -> io::Result<Selection> {
        let editor = &self.tab().editor;
        let size = editor.document.dimensions();
        selection
            .feather_with_aa(size, self.selection_feather, self.settings.antialiasing)?
            .combine(editor.selection.as_ref(), size, mode)
    }

    fn wand(&self, seed: Point, contiguous: bool) -> io::Result<Selection> {
        let tab = self.tab();
        let size = tab.editor.document.dimensions();
        let source = tab.editor.sample_buffer(self.wand_composite)?;
        Selection::wand(
            size,
            seed,
            self.color_tolerance,
            self.alpha_match,
            contiguous,
            |(x, y)| source[y as usize * size.0 as usize + x as usize],
        )
    }

    pub fn has_active_session(&self) -> bool {
        self.tab().session.is_some()
    }

    pub fn commit_active_session(&mut self) {
        let Some(mut session) = self.tab_mut().session.take() else {
            self.tab_mut().session_creation = None;
            return;
        };
        self.tab_mut().session_creation = None;
        match session {
            ActiveSession::Transform(transform) => {
                self.commit_transform(transform);
                return;
            }
            ActiveSession::CanvasResize(cr) => {
                if cr.preview_size != cr.original {
                    let selected = self.tab().editor.selected;
                    let result =
                        geometry::canvas_size(&self.tab().editor.document, cr.preview_size, (0, 0));
                    self.install_geometry(result, selected, "canvas-size");
                } else {
                    self.tab_mut().editor.mark_full_refresh();
                }
                return;
            }
            ActiveSession::Text(ref text) if text.text.trim().is_empty() => return,
            _ => {}
        }
        let size = self.tab().editor.document.dimensions();
        let aa_mode = self.settings.antialiasing;
        let label = session.commit_label();
        let preview = {
            let tab = &mut self.tabs[self.active_tab];
            let layer = tab.editor.selected;
            let selection = tab.editor.selection.as_ref();
            let src = tab
                .editor
                .document
                .layer_pixels(layer)
                .expect("active layer");
            session.render_layer_preview(&mut self.font_catalog, src, size, selection, aa_mode)
        };
        if let Some(after) = preview {
            let result = self.tab_mut().editor.commit_layer_preview(after, label);
            self.finish_change(result);
            self.tab_mut().editor.mark_full_refresh();
        }
    }

    fn activate_tool(&mut self, tool: Tool) {
        if self.tool != tool {
            self.finish_opacity();
            self.commit_active_session();
            if matches!(
                self.tool,
                Tool::Brush
                    | Tool::Pencil
                    | Tool::Eraser
                    | Tool::CloneStamp
                    | Tool::Recolor
                    | Tool::Bucket
            ) {
                self.last_paint_tool = self.tool;
            }
            self.tab_mut().clear_gestures();
            self.tool = tool;
        }
        self.shortcuts.remember_tool(tool.shortcut());
    }

    fn clear_canvas_selection(&mut self) {
        let tab = self.tab_mut();
        tab.selection_gesture = None;
        tab.selection_path.clear();
        tab.stroke_gesture = None;
        tab.editor.set_selection(None, "deselect");
    }

    fn bucket_fill(&mut self, seed: Point, color: Pixel, contiguous: bool) {
        self.finish_opacity();
        let (primary, secondary) = if color == self.palette.secondary()
            && self.palette.primary() != self.palette.secondary()
        {
            self.palette.pair(true)
        } else {
            (color, self.palette.secondary())
        };
        let pattern = self.fill_pattern;
        let blend = self.stroke_blend;
        let tolerance = self.color_tolerance;
        let alpha_mode = self.alpha_match;
        let composite = self.bucket_composite;
        let aa_mode = self.settings.antialiasing;
        let result = self.tab_mut().editor.bucket_fill(
            seed, primary, secondary, pattern, blend, tolerance, alpha_mode, contiguous, composite,
            aa_mode,
        );
        self.finish_change(result);
    }

    fn paint_selection(&mut self, erase: bool) {
        self.finish_opacity();
        let (color, label) = if erase {
            (Pixel::default(), "erase-selection")
        } else {
            (self.palette.primary(), "fill-selection")
        };
        let result = self.tab_mut().editor.fill_selection(color, label);
        self.finish_change(result);
    }

    fn refresh_thumbnails(&mut self, ctx: &egui::Context) {
        let active_session = self.has_active_session();
        let tab = &mut self.tabs[self.active_tab];
        if !tab.thumbs_dirty || active_session {
            return;
        }
        tab.thumbs_dirty = false;
        let doc = &tab.editor.document;
        let tab_thumb = make_thumbnail_texture(ctx, "tab_thumb", doc, None, 24);
        let count = doc.layer_count();
        let mut layer_thumbs = Vec::with_capacity(count);
        for idx in 0..count {
            layer_thumbs.push(make_thumbnail_texture(
                ctx,
                &format!("layer_thumb_{idx}"),
                doc,
                Some(idx),
                28,
            ));
        }
        tab.tab_thumb = tab_thumb;
        tab.layer_thumbs = layer_thumbs;
    }

    fn refresh_image(&mut self, ctx: &egui::Context) {
        let font_catalog = &mut self.font_catalog;
        let aa_mode = self.settings.antialiasing;
        let tab = &mut self.tabs[self.active_tab];
        let dirty = tab.editor.take_dirty();
        if !dirty.full && dirty.rect.is_none() {
            return;
        }
        let (width, height) = tab.editor.document.dimensions();
        let preview_active = tab
            .session
            .as_ref()
            .is_some_and(ActiveSession::needs_layer_preview);

        if !dirty.full
            && !preview_active
            && let Some((min, max)) = dirty.rect
            && let Some(texture) = &mut tab.image
            && texture.size() == [width as usize, height as usize]
        {
            match tab.editor.document.composite_rect(min, max) {
                Ok(pixels) => {
                    let rw = (max.0 - min.0 + 1) as usize;
                    let rh = (max.1 - min.1 + 1) as usize;
                    let sub = pixels_to_color_image(rw, rh, &pixels);
                    texture.set_partial(
                        [min.0 as usize, min.1 as usize],
                        sub,
                        TextureOptions::NEAREST,
                    );
                }
                Err(error) => {
                    self.report(Err(error), "");
                }
            }
            return;
        }

        let composite = (|| {
            if !preview_active {
                return tab.editor.document.composite();
            }
            let layer = tab.editor.selected;
            let src = tab
                .editor
                .document
                .layer_pixels(layer)
                .expect("active layer");
            let sel = tab.editor.selection.as_ref();
            let mut preview = match tab.session.as_mut() {
                Some(ActiveSession::Transform(transform)) => {
                    if let Some(floating) = &transform.floating {
                        tab.editor
                            .preview_floating(floating, transform.affine, transform.method)?
                    } else {
                        tab.editor.transform_pixels(
                            transform.affine,
                            transform.method,
                            transform.target == TransformTarget::Pixels,
                            transform.clone_source,
                        )?
                    }
                }
                Some(session) => session
                    .render_layer_preview(font_catalog, src, (width, height), sel, aa_mode)
                    .unwrap_or_else(|| src.to_vec()),
                None => src.to_vec(),
            };
            tab.editor.document.swap_layer_pixels(layer, &mut preview)?;
            let composite = tab.editor.document.composite();
            tab.editor.document.swap_layer_pixels(layer, &mut preview)?;
            composite
        })();
        match composite {
            Ok(pixels) => {
                let image = pixels_to_color_image(width as usize, height as usize, &pixels);
                if let Some(texture) = &mut tab.image {
                    texture.set(image, TextureOptions::NEAREST);
                } else {
                    tab.image =
                        Some(ctx.load_texture(tr("document"), image, TextureOptions::NEAREST));
                }
            }
            Err(error) => {
                tab.image = None;
                self.report(Err(error), "");
            }
        }
    }

    fn has_modal_open(&self) -> bool {
        self.pending_close.is_some()
            || self.new_size.is_some()
            || self.size_dialog.is_some()
            || self.save_dialog.is_some()
            || self.layer_dialog.is_some()
            || self.shortcut_dialog.is_some()
    }

    fn stroke_size_target(&mut self) -> Option<(&mut u32, u32)> {
        if self.has_active_session() {
            let size = self.tab_mut().session.as_mut()?.stroke_size_target()?;
            return Some((size, MAX_OUTLINE_THICKNESS));
        }
        Some(match self.tool {
            Tool::Brush | Tool::Eraser | Tool::CloneStamp | Tool::Recolor => {
                (&mut self.diameter, MAX_BRUSH_DIAMETER)
            }
            Tool::Pencil => (&mut self.pencil_diameter, MAX_BRUSH_DIAMETER),
            Tool::LineCurve | Tool::DrawShape => (&mut self.shape_thickness, MAX_OUTLINE_THICKNESS),
            _ => return None,
        })
    }

    fn adjust_stroke_size(&mut self, delta: i32) -> bool {
        let active_session = self.has_active_session();
        let Some((size, max)) = self.stroke_size_target() else {
            return false;
        };
        let next = size.saturating_add_signed(delta).clamp(1, max);
        if *size == next {
            return false;
        }
        *size = next;
        if active_session {
            self.tab_mut().editor.mark_full_refresh();
            self.shape_thickness = next;
        }
        true
    }

    fn paint_stroke_size_cursor(
        &mut self,
        response: &egui::Response,
        painter: &egui::Painter,
        zoom: f32,
    ) {
        let ctx = &response.ctx;
        // Vector handles can cover the canvas widget while remaining on its layer.
        if ctx.input(|input| {
            self.shortcuts
                .mouse
                .wheel_chord_down(MouseAction::WheelStrokeSize, input)
        }) && ctx.rect_contains_pointer(response.layer_id, response.interact_rect)
            && let Some(center) = ctx.input(|input| input.pointer.hover_pos())
            && let Some((size, _)) = self.stroke_size_target()
        {
            let radius = *size as f32 * zoom * 0.5;
            ctx.set_cursor_icon(egui::CursorIcon::None);
            let white = egui::Stroke::new(1.0_f32, Color32::WHITE);
            let dot_radius = if radius > 1.5 {
                painter.circle_stroke(center, radius, egui::Stroke::new(2.5_f32, Color32::BLACK));
                painter.circle_stroke(center, radius, white);
                1.0
            } else {
                radius.max(0.5)
            };
            painter.circle(center, dot_radius, Color32::BLACK, white);
        }
    }

    fn canvas(&mut self, ui: &mut egui::Ui) -> f32 {
        let (width, height) = self.tab().editor.document.dimensions();
        let pixels = vec2(width as f32, height as f32);
        let available = ui.available_size();
        let padding = 24.0;
        let rulers = self.tab().view.rulers;
        let insets = if rulers { vec2(34.0, 22.0) } else { Vec2::ZERO };
        let usable = available - insets;
        let fit = ((usable.x - padding) / pixels.x)
            .min((usable.y - padding) / pixels.y)
            .clamp(0.001, 1.0);
        let overscroll = (usable * 0.35).max(Vec2::splat(padding));
        let canvas_space = |scaled: Vec2| scaled.max(usable) + overscroll * 2.0 + insets;
        let zoom_centered_on = |tab: &mut DocumentTab, next_zoom: f32, doc_point: Vec2| {
            let next = next_zoom.clamp(MIN_ZOOM, MAX_ZOOM);
            let scaled = pixels * next;
            let space = canvas_space(scaled);
            tab.zoom = Some(next);
            tab.view.scroll_to = Some((space - scaled) * 0.5 + doc_point * next - available * 0.5);
        };
        let fit_selection = std::mem::take(&mut self.tab_mut().view.fit_selection);
        if fit_selection
            && let Some((min, max)) = self
                .tab()
                .editor
                .selection
                .as_ref()
                .and_then(|s| s.bounds((width, height)))
        {
            let region = vec2((max.0 - min.0 + 1) as f32, (max.1 - min.1 + 1) as f32);
            let next = ((usable.x - padding) / region.x).min((usable.y - padding) / region.y);
            let center = vec2(min.0 as f32, min.1 as f32) + region * 0.5;
            zoom_centered_on(self.tab_mut(), next, center);
        }

        let modal_open = self.has_modal_open() || self.colors_open;
        let panel_rect = ui.available_rect_before_wrap();
        if !modal_open {
            let hover_pos = ui.input(|input| input.pointer.hover_pos());
            if let Some(pos) = hover_pos.filter(|p| panel_rect.contains(*p)) {
                let line_scroll_speed = ui
                    .ctx()
                    .options(|options| options.input_options.line_scroll_speed);
                let (scroll, magnify, size_delta) = ui.input(|input| {
                    let mut scroll = Vec2::ZERO;
                    let mut magnify = 0.0;
                    let mut size_delta = 0i32;
                    // Use the event axes before egui folds Shift+vertical wheel into horizontal scroll.
                    for event in &input.events {
                        let egui::Event::MouseWheel {
                            unit,
                            delta,
                            modifiers,
                        } = *event
                        else {
                            continue;
                        };
                        let scale = match unit {
                            egui::MouseWheelUnit::Point => 1.0,
                            egui::MouseWheelUnit::Line => line_scroll_speed,
                            egui::MouseWheelUnit::Page => input.viewport_rect().height(),
                        };
                        for (trigger, amount) in
                            [(Trigger::Wheel, delta.y), (Trigger::WheelX, delta.x)]
                        {
                            if amount == 0.0 || !amount.is_finite() {
                                continue;
                            }
                            match self.shortcuts.mouse.resolve(
                                Scope::Canvas,
                                trigger,
                                modifiers,
                                |key| input.key_down(key),
                            ) {
                                Some(MouseAction::WheelVertical) => scroll.y += amount * scale,
                                Some(MouseAction::WheelHorizontal) => scroll.x += amount * scale,
                                Some(MouseAction::WheelZoom) => magnify += amount * scale,
                                Some(MouseAction::WheelStrokeSize) => {
                                    let steps = match unit {
                                        egui::MouseWheelUnit::Line => amount.abs().round().max(1.0),
                                        _ => 1.0,
                                    };
                                    size_delta =
                                        size_delta.saturating_add((steps * amount.signum()) as i32);
                                }
                                _ => {}
                            }
                        }
                    }
                    (scroll, magnify, size_delta)
                });
                if size_delta != 0 && self.adjust_stroke_size(size_delta) {
                    ui.ctx().request_repaint();
                }
                if scroll != Vec2::ZERO || magnify != 0.0 {
                    let current = self.tab().zoom.unwrap_or(fit);
                    let next = (current * 1.0025_f32.powf(magnify).clamp(0.5, 2.0))
                        .clamp(MIN_ZOOM, MAX_ZOOM);
                    let origin = |gain: f32| {
                        let scaled = pixels * gain;
                        let space = canvas_space(scaled);
                        panel_rect.min + (space - scaled) * 0.5 + insets * 0.5
                    };
                    let tab = self.tab_mut();
                    let mut offset = tab.view.scroll_to.unwrap_or(tab.view.offset);
                    if magnify != 0.0 {
                        let document_point = (pos - origin(current) + offset) / current;
                        offset = document_point * next - (pos - origin(next));
                        tab.zoom = Some(next);
                    }
                    let target_zoom = tab.zoom.unwrap_or(fit);
                    let target_max =
                        (canvas_space(pixels * target_zoom) - available).max(Vec2::ZERO);
                    let next_offset = (offset - scroll).clamp(Vec2::ZERO, target_max);
                    if magnify != 0.0 || (next_offset - tab.view.offset).length_sq() > 1e-4 {
                        tab.view.scroll_to = Some(next_offset);
                        ui.ctx().request_repaint();
                    }
                }
            }
        }

        let zoom = self.tab().zoom.unwrap_or(fit);
        let size = pixels * zoom;
        let base_space = canvas_space(size);
        let mut space = base_space;
        if let Some(cr) = self
            .tab()
            .session
            .as_ref()
            .and_then(ActiveSession::as_canvas_resize)
        {
            let preview_px = vec2(cr.preview_size.0 as f32, cr.preview_size.1 as f32) * zoom;
            space = space.max(canvas_space(preview_px));
        }
        let max_offset = (space - available).max(Vec2::ZERO);
        if !self.tab().view.centered {
            let tab = self.tab_mut();
            tab.view.centered = true;
            tab.view.scroll_to.get_or_insert(max_offset * 0.5);
        }
        // Apply active pan movement before laying out and painting the scroll area.
        let can_pan = !modal_open && !self.has_active_session();
        let tab = self.tab_mut();
        if let Some(gesture) = tab.pan_gesture.as_mut() {
            let (position, down, released) = ui.input(|input| {
                (
                    input.pointer.latest_pos(),
                    input.pointer.button_down(gesture.button),
                    input.pointer.button_released(gesture.button),
                )
            });
            if can_pan
                && (down || released)
                && let Some((previous, current)) = gesture.last_position.zip(position)
                && current != previous
            {
                let offset = tab.view.scroll_to.unwrap_or(tab.view.offset);
                tab.view.scroll_to =
                    Some((offset - (current - previous)).clamp(Vec2::ZERO, max_offset));
            }
            gesture.last_position = position;
            if !can_pan || !down {
                tab.pan_gesture = None;
            }
        }
        let mut scroll = egui::ScrollArea::both()
            .auto_shrink([false; 2])
            .scroll_source(egui::scroll_area::ScrollSource {
                drag: false,
                mouse_wheel: false,
                ..Default::default()
            });
        if let Some(offset) = self.tab_mut().view.scroll_to.take() {
            scroll = scroll.scroll_offset(offset.clamp(Vec2::ZERO, max_offset));
        }

        let output = scroll.show(ui, |ui| {
            let (area, _) = ui.allocate_exact_size(space, egui::Sense::hover());
            let rect = Rect::from_center_size(area.min + base_space * 0.5 + insets * 0.5, size);
            let guide_margin = ui.clip_rect().min + insets;
            let canvas_clip = Rect::from_min_max(guide_margin, ui.clip_rect().max);
            let hit_rect = if rulers {
                rect.intersect(canvas_clip)
            } else {
                rect
            };
            let active_session = self.has_active_session();
            let background = ui.interact(
                canvas_clip,
                ui.id().with("canvas_background"),
                if active_session {
                    egui::Sense::hover()
                } else {
                    egui::Sense::click()
                },
            );
            let response = ui.interact(
                hit_rect,
                egui::Id::new(CANVAS_ID),
                if active_session {
                    egui::Sense::hover()
                } else {
                    egui::Sense::click_and_drag()
                },
            );
            let outer = ui.painter();
            let canvas_overlay = ui.painter_at(canvas_clip);
            let shadow_alpha = match self.settings.theme {
                Theme::Light => 14,
                Theme::Dark => 56,
            };
            outer.rect_filled(
                rect.translate(vec2(0.0, 2.0)).expand(2.0),
                4.0,
                Color32::from_black_alpha(shadow_alpha),
            );
            outer.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0_f32, self.settings.theme.border_color()),
                egui::StrokeKind::Outside,
            );
            let painter = ui.painter_at(rect);
            let viewport = CanvasViewport { rect, zoom };
            paint_checker(&painter, &self.checker, rect, pixels / 16.0);
            if let Some(image) = &self.tab().image {
                painter.image(
                    image.id(),
                    rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            let pointer_position = response
                .hover_pos()
                .map(|pointer| viewport.to_world(pointer).to_pos2());
            if self.tab().view.pointer != pointer_position {
                self.tab_mut().view.pointer = pointer_position;
                ui.ctx().request_repaint();
            }

            let mut corner_hovered = false;
            let mut suppress_corner_click = false;
            if !modal_open {
                let transform_outcome = match self.tab_mut().session.take() {
                    Some(ActiveSession::Transform(mut transform)) => {
                        let outcome = transform.handles(viewport, &self.shortcuts.mouse, ui);
                        if outcome == SessionOutcome::Commit {
                            self.commit_transform(transform);
                        } else {
                            self.tab_mut().session = Some(ActiveSession::Transform(transform));
                        }
                        Some(outcome)
                    }
                    other => {
                        self.tab_mut().session = other;
                        None
                    }
                };
                if let Some(outcome) = transform_outcome {
                    match outcome {
                        SessionOutcome::Changed => {
                            self.tab_mut().editor.mark_full_refresh();
                            ui.ctx().request_repaint();
                        }
                        SessionOutcome::Commit => {
                            ui.ctx().request_repaint();
                        }
                        SessionOutcome::None => {}
                    }
                    let dpi = self.tab().editor.document.resolution_dpi();
                    self.tab().view.guides(ui, rect, zoom, dpi);
                    return;
                }

                if let Some((orig, mut preview_size, active_drag)) = self
                    .tab()
                    .session
                    .as_ref()
                    .and_then(ActiveSession::as_canvas_resize)
                    .map(|cr| (cr.original, cr.preview_size, cr.dragging))
                {
                    let limit = max_canvas_side(ui.ctx());
                    let mut preview_rect = Rect::from_min_size(
                        rect.min,
                        vec2(preview_size.0 as f32, preview_size.1 as f32) * zoom,
                    );
                    let r_handle = pos2(preview_rect.right(), preview_rect.center().y);
                    let b_handle = pos2(preview_rect.center().x, preview_rect.bottom());

                    let drag_resp = ui.interact(
                        ui.clip_rect(),
                        ui.id().with("canvas_resize_drag"),
                        egui::Sense::click_and_drag(),
                    );
                    let handle_hit = |pos: egui::Pos2| {
                        let box_for = |c: egui::Pos2| Rect::from_center_size(c, Vec2::splat(18.0));
                        if box_for(r_handle).contains(pos) {
                            CanvasResizeDrag::RightEdge
                        } else if box_for(b_handle).contains(pos) {
                            CanvasResizeDrag::BottomEdge
                        } else {
                            CanvasResizeDrag::BottomRight
                        }
                    };
                    if active_drag.is_none()
                        && (drag_resp.drag_started()
                            || ui.input(|i| i.pointer.button_pressed(PointerButton::Primary)))
                        && let Some(ptr) = ui
                            .input(|i| i.pointer.interact_pos())
                            .filter(|p| ui.clip_rect().contains(*p))
                        && let Some(cr) = self
                            .tab_mut()
                            .session
                            .as_mut()
                            .and_then(ActiveSession::as_canvas_resize_mut)
                    {
                        cr.dragging = Some(handle_hit(ptr));
                    }
                    let active_drag = self
                        .tab()
                        .session
                        .as_ref()
                        .and_then(ActiveSession::as_canvas_resize)
                        .and_then(|cr| cr.dragging);
                    if let Some(kind) = active_drag {
                        ui.ctx().set_cursor_icon(kind.cursor());
                        let (down, released, ptr, shift) = ui.input(|i| {
                            (
                                i.pointer.button_down(PointerButton::Primary),
                                i.pointer.button_released(PointerButton::Primary)
                                    || drag_resp.drag_stopped(),
                                i.pointer.interact_pos(),
                                i.modifiers.shift,
                            )
                        });
                        if let Some(pos) = ptr {
                            let doc_px = viewport.to_world(pos);
                            let next_w =
                                (doc_px.x.round() as i64).clamp(1, i64::from(limit)) as u32;
                            let next_h =
                                (doc_px.y.round() as i64).clamp(1, i64::from(limit)) as u32;
                            match kind {
                                CanvasResizeDrag::RightEdge => preview_size.0 = next_w,
                                CanvasResizeDrag::BottomEdge => preview_size.1 = next_h,
                                CanvasResizeDrag::BottomRight => {
                                    if shift && orig.0 > 0 && orig.1 > 0 {
                                        let sx = f64::from(next_w) / f64::from(orig.0);
                                        let sy = f64::from(next_h) / f64::from(orig.1);
                                        let scale = if sx > sy { sx } else { sy };
                                        preview_size = (
                                            ((f64::from(orig.0) * scale).round() as u32)
                                                .clamp(1, limit),
                                            ((f64::from(orig.1) * scale).round() as u32)
                                                .clamp(1, limit),
                                        );
                                    } else {
                                        preview_size = (next_w, next_h);
                                    }
                                }
                            }
                            if let Some(cr) = self
                                .tab_mut()
                                .session
                                .as_mut()
                                .and_then(ActiveSession::as_canvas_resize_mut)
                            {
                                cr.preview_size = preview_size;
                            }
                            preview_rect = Rect::from_min_size(
                                rect.min,
                                vec2(preview_size.0 as f32, preview_size.1 as f32) * zoom,
                            );
                            ui.ctx().request_repaint();
                        }
                        if (!down || released)
                            && let Some(cr) = self
                                .tab_mut()
                                .session
                                .as_mut()
                                .and_then(ActiveSession::as_canvas_resize_mut)
                        {
                            cr.dragging = None;
                        }
                    } else if let Some(ptr) = ui.input(|i| i.pointer.hover_pos()) {
                        let cursor = handle_hit(ptr).cursor();
                        if ui.clip_rect().contains(ptr) {
                            ui.ctx().set_cursor_icon(cursor);
                        }
                    }

                    let overlay = ui.painter();
                    let paint_checker_strip = |strip: Rect| {
                        if strip.is_positive() {
                            paint_checker(
                                overlay,
                                &self.checker,
                                strip,
                                vec2(
                                    (strip.width() / (16.0 * zoom)).max(0.25),
                                    (strip.height() / (16.0 * zoom)).max(0.25),
                                ),
                            );
                        }
                    };
                    if preview_size.0 > width {
                        let right_strip = Rect::from_min_max(
                            pos2(rect.right(), rect.top()),
                            pos2(
                                preview_rect.right(),
                                preview_rect.bottom().min(rect.bottom()),
                            ),
                        );
                        paint_checker_strip(right_strip);
                    } else if preview_size.0 < width {
                        let trim_right = Rect::from_min_max(
                            pos2(preview_rect.right(), rect.top()),
                            rect.right_bottom(),
                        );
                        if trim_right.is_positive() {
                            overlay.rect_filled(trim_right, 0.0, Color32::from_black_alpha(120));
                        }
                    }
                    if preview_size.1 > height {
                        let bottom_strip = Rect::from_min_max(
                            pos2(rect.left(), rect.bottom()),
                            preview_rect.right_bottom(),
                        );
                        paint_checker_strip(bottom_strip);
                    } else if preview_size.1 < height {
                        let trim_bottom = Rect::from_min_max(
                            pos2(rect.left(), preview_rect.bottom()),
                            pos2(preview_rect.right().min(rect.right()), rect.bottom()),
                        );
                        if trim_bottom.is_positive() {
                            overlay.rect_filled(trim_bottom, 0.0, Color32::from_black_alpha(120));
                        }
                    }

                    paint_dashed_line(overlay, &rectangle_outline(preview_rect));
                    overlay.rect_stroke(
                        preview_rect,
                        0.0,
                        egui::Stroke::new(1.2_f32, self.settings.theme.accent()),
                        egui::StrokeKind::Outside,
                    );
                    for handle_pos in [
                        pos2(preview_rect.right(), preview_rect.center().y),
                        pos2(preview_rect.center().x, preview_rect.bottom()),
                        preview_rect.right_bottom(),
                    ] {
                        overlay.rect(
                            Rect::from_center_size(handle_pos, Vec2::splat(9.0)),
                            1.0,
                            Color32::WHITE,
                            egui::Stroke::new(1.2_f32, Color32::BLACK),
                            egui::StrokeKind::Outside,
                        );
                    }
                    let badge_pos = preview_rect.right_bottom() + vec2(8.0, 8.0);
                    let badge_galley = overlay.layout_no_wrap(
                        format!("{} × {} px", preview_size.0, preview_size.1),
                        egui::FontId::proportional(11.5),
                        Color32::WHITE,
                    );
                    let badge_rect =
                        Rect::from_min_size(badge_pos, badge_galley.size() + vec2(10.0, 6.0));
                    overlay.rect_filled(badge_rect, 4.0, Color32::from_black_alpha(190));
                    overlay.galley(badge_pos + vec2(5.0, 3.0), badge_galley, Color32::WHITE);

                    self.paint_canvas_guides(ui, &painter, rect, zoom, (width, height));
                    return;
                }

                if self.tab().session.is_some() {
                    if let Some((btn, anchor)) = self.tab().session_creation {
                        let (down, released, ptr, modifiers) = ui.input(|i| {
                            (
                                i.pointer.button_down(btn),
                                i.pointer.button_released(btn),
                                i.pointer.interact_pos(),
                                i.modifiers,
                            )
                        });
                        if let Some(pos) = ptr {
                            let cur = viewport.to_world(pos);
                            let tab = self.tab_mut();
                            match tab.session.as_mut() {
                                Some(ActiveSession::Gradient(g)) => {
                                    g.end = snap_if(anchor, cur, modifiers.shift);
                                }
                                Some(ActiveSession::LineCurve(lc)) => {
                                    lc.set_end(anchor, cur, modifiers.shift);
                                }
                                Some(ActiveSession::Shape(s)) => {
                                    s.set_creation_bounds(anchor, cur, modifiers.shift);
                                }
                                _ => {}
                            }
                            tab.editor.mark_full_refresh();
                            ui.ctx().request_repaint();
                        }
                        if !down || released {
                            self.tab_mut().session_creation = None;
                            ui.ctx().request_repaint();
                        }
                    } else {
                        let outcome = if let Some(mut session) = self.tab_mut().session.take() {
                            let outcome = match &mut session {
                                ActiveSession::Gradient(g) => g.handles(viewport, ui),
                                ActiveSession::LineCurve(lc) => lc.handles(viewport, ui),
                                ActiveSession::Shape(s) => s.handles(viewport, ui),
                                ActiveSession::Text(t) => t.handles(viewport, ui),
                                ActiveSession::Transform(_) | ActiveSession::CanvasResize(_) => {
                                    SessionOutcome::None
                                }
                            };
                            self.tab_mut().session = Some(session);
                            outcome
                        } else {
                            SessionOutcome::None
                        };
                        match outcome {
                            SessionOutcome::Commit => {
                                self.commit_active_session();
                                ui.ctx().request_repaint();
                            }
                            SessionOutcome::Changed => {
                                self.tab_mut().editor.mark_full_refresh();
                                ui.ctx().request_repaint();
                            }
                            SessionOutcome::None => {}
                        }
                    }
                    self.paint_canvas_guides(ui, &painter, rect, zoom, (width, height));
                    self.paint_stroke_size_cursor(&response, &canvas_overlay, zoom);
                    return;
                }

                let corner_zone = Rect::from_center_size(rect.right_bottom(), vec2(28.0, 28.0));
                corner_hovered = ui.input(|input| {
                    input.modifiers.ctrl
                        && input
                            .pointer
                            .hover_pos()
                            .is_some_and(|p| corner_zone.contains(p))
                });
                let trigger_corner_resize = if !self.has_active_session()
                    && ui
                        .ctx()
                        .input(|input| input.pointer.interact_pos())
                        .is_some_and(|pos| corner_zone.contains(pos))
                {
                    self.shortcuts
                        .mouse
                        .double_clicked_in(ui.ctx(), Scope::CanvasCorner)
                        .and_then(|(_, act)| {
                            (act == MouseAction::CanvasCornerResize)
                                .then(|| ui.input(|input| input.modifiers.ctrl))
                        })
                } else {
                    None
                };
                if let Some(ctrl_down) = trigger_corner_resize {
                    self.finish_opacity();
                    let tab = self.tab_mut();
                    tab.clear_gestures();
                    tab.session = Some(ActiveSession::CanvasResize(CanvasResizeSession {
                        original: (width, height),
                        preview_size: (width, height),
                        dragging: None,
                        prev_ctrl: ctrl_down,
                    }));
                    ui.ctx().request_repaint();
                    self.paint_canvas_guides(ui, &painter, rect, zoom, (width, height));
                    return;
                }
                suppress_corner_click = ui.ctx().input(|input| {
                    input.modifiers.ctrl
                        && input
                            .pointer
                            .interact_pos()
                            .is_some_and(|p| corner_zone.contains(p))
                });
                if corner_hovered {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                }

                // Give a simple click outside the selection priority over drawing
                // or creating a one-pixel selection. A drag keeps its tool behavior.
                let modifiers = ui.input(|input| input.modifiers);
                if self.tool != Tool::Eyedropper
                    && !modifiers.any()
                    && (response.clicked() || background.clicked())
                    && let Some(selection) = self.tab().editor.selection.as_ref()
                    && let Some(pos) = response
                        .interact_pointer_pos()
                        .or_else(|| background.interact_pointer_pos())
                {
                    let outside = !rect.contains(pos)
                        || viewport
                            .to_pixel(pos, (width, height))
                            .is_none_or(|pt| selection.coverage_u8(pt) == 0);
                    if outside {
                        self.clear_canvas_selection();
                        self.paint_canvas_guides(ui, &painter, rect, zoom, (width, height));
                        ui.ctx().request_repaint();
                        return;
                    }
                }

                // Resolve active canvas pan (Space+drag, Middle-drag, or Hand tool)
                let pan_trigger = if self.tab().selection_gesture.is_some() {
                    None
                } else if let Some(gesture) = &self.tab().pan_gesture {
                    Some(gesture.button)
                } else if response.hovered() || response.is_pointer_button_down_on() {
                    let allow_hand = self.tool == Tool::Pan;
                    self.shortcuts
                        .mouse
                        .active_match(ui.ctx(), true, |mouse, input, btn| {
                            let is_canvas_pan = mouse.resolve(
                                Scope::Canvas,
                                Trigger::Button(btn),
                                input.modifiers,
                                |k| input.key_down(k),
                            ) == Some(MouseAction::Pan);
                            let is_hand_pan = allow_hand
                                && mouse.resolve(
                                    Scope::Hand,
                                    Trigger::Button(btn),
                                    input.modifiers,
                                    |k| input.key_down(k),
                                ) == Some(MouseAction::Hand);
                            (is_canvas_pan || is_hand_pan).then_some(btn)
                        })
                } else {
                    None
                };
                let temp_pan_held = self.tab().selection_gesture.is_none()
                    && self.shortcuts.held(ui.ctx(), Command::TemporaryPan);

                if pan_trigger.is_some() || self.tool == Tool::Pan || temp_pan_held {
                    let tab = self.tab_mut();
                    tab.editor.finish_stroke();
                    tab.selection_gesture = None;
                    tab.selection_path.clear();
                    if tab.pan_gesture.is_none()
                        && let Some(button) = pan_trigger
                    {
                        tab.pan_gesture = Some(PanGesture {
                            button,
                            last_position: ui.input(|input| input.pointer.latest_pos()),
                        });
                    }
                    self.paint_canvas_guides(ui, &painter, rect, zoom, (width, height));
                    if response.hovered() || pan_trigger.is_some() {
                        ui.ctx().set_cursor_icon(if pan_trigger.is_some() {
                            egui::CursorIcon::Grabbing
                        } else {
                            egui::CursorIcon::Grab
                        });
                    }
                    return;
                }

                // Temporary Zoom click (Ctrl+Space+Left / Alt+Space+Left) or Zoom tool (click / marquee / scrubby)
                let temp_zoom_click = (response.hovered() || response.is_pointer_button_down_on())
                    .then(|| {
                        self.shortcuts
                            .mouse
                            .pressed_in(ui.ctx(), Scope::Canvas)
                            .and_then(|(_, act)| match act {
                                MouseAction::TempZoomIn => Some(ZOOM_IN_FACTOR),
                                MouseAction::TempZoomOut => Some(ZOOM_OUT_FACTOR),
                                _ => None,
                            })
                    })
                    .flatten();
                if let Some(factor) = temp_zoom_click {
                    if let Some(pos) = response.interact_pointer_pos() {
                        zoom_centered_on(self.tab_mut(), zoom * factor, viewport.to_world(pos));
                    } else {
                        self.tab_mut().zoom = Some((zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM));
                    }
                    ui.ctx().request_repaint();
                    self.paint_canvas_guides(ui, &painter, rect, zoom, (width, height));
                    return;
                }
                if self.tool == Tool::Zoom {
                    if response.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ZoomIn);
                    }
                    if let Some((btn, press_pos, start_zoom, is_scrubby)) = self.tab().zoom_drag {
                        let (down, released, ptr) = ui.input(|i| {
                            (
                                i.pointer.button_down(btn),
                                i.pointer.button_released(btn),
                                i.pointer.interact_pos(),
                            )
                        });
                        let dragged = ptr.is_some_and(|p| p.distance(press_pos) >= 4.0);
                        if let Some(pos) = ptr
                            && dragged
                        {
                            if is_scrubby {
                                let factor = 2.0_f32.powf((pos.x - press_pos.x) / 80.0);
                                zoom_centered_on(
                                    self.tab_mut(),
                                    start_zoom * factor,
                                    (press_pos - rect.min) / start_zoom,
                                );
                                ui.ctx().request_repaint();
                            } else {
                                let marquee = Rect::from_two_pos(press_pos, pos);
                                paint_dashed_line(&painter, &rectangle_outline(marquee));
                            }
                        }
                        if !down || released {
                            if dragged && !is_scrubby {
                                if let Some(pos) = ptr {
                                    let p0 = ((press_pos - rect.min) / start_zoom)
                                        .clamp(Vec2::ZERO, pixels);
                                    let p1 =
                                        ((pos - rect.min) / start_zoom).clamp(Vec2::ZERO, pixels);
                                    let box_w = (p1.x - p0.x).abs().max(2.0);
                                    let box_h = (p1.y - p0.y).abs().max(2.0);
                                    let next = ((usable.x - padding) / box_w)
                                        .min((usable.y - padding) / box_h);
                                    let center_doc = (p0 + p1) * 0.5;
                                    zoom_centered_on(self.tab_mut(), next, center_doc);
                                    ui.ctx().request_repaint();
                                }
                            } else if !dragged {
                                let action = self.shortcuts.mouse.at(
                                    ui.ctx(),
                                    Scope::Zoom,
                                    Trigger::Button(btn),
                                    modifiers,
                                );
                                let factor = match action {
                                    Some(MouseAction::ZoomOut) => Some(ZOOM_OUT_FACTOR),
                                    Some(MouseAction::ZoomIn) => Some(ZOOM_IN_FACTOR),
                                    _ => None,
                                };
                                if let Some(factor) = factor {
                                    zoom_centered_on(
                                        self.tab_mut(),
                                        start_zoom * factor,
                                        (press_pos - rect.min) / start_zoom,
                                    );
                                    ui.ctx().request_repaint();
                                }
                            }
                            self.tab_mut().zoom_drag = None;
                        }
                    } else if response.hovered() || response.is_pointer_button_down_on() {
                        let started = ui
                            .ctx()
                            .input(|input| input.pointer.interact_pos())
                            .and_then(|pos| {
                                let (btn, act) = self
                                    .shortcuts
                                    .mouse
                                    .pressed_in_any(ui.ctx(), &[Scope::ZoomDrag, Scope::Zoom])?;
                                matches!(
                                    act,
                                    MouseAction::ZoomScrubby
                                        | MouseAction::ZoomMarquee
                                        | MouseAction::ZoomIn
                                        | MouseAction::ZoomOut
                                )
                                .then_some((
                                    btn,
                                    pos,
                                    act == MouseAction::ZoomScrubby,
                                ))
                            });
                        if let Some((btn, pos, is_scrubby)) = started {
                            self.tab_mut().zoom_drag = Some((btn, pos, zoom, is_scrubby));
                        }
                    }
                    self.paint_canvas_guides(ui, &painter, rect, zoom, (width, height));
                    return;
                }
            }

            if corner_hovered {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
            } else if response.hovered() || response.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
            let mut preview = self.tab().editor.selection.clone();
            if !modal_open && !suppress_corner_click {
                let (origin, pointer, modifiers) = ui.input(|input| {
                    (
                        input.pointer.press_origin(),
                        input.pointer.interact_pos(),
                        input.modifiers,
                    )
                });
                let document_point =
                    |position: egui::Pos2| viewport.to_pixel(position, (width, height));
                let can_begin = response.hovered()
                    || response.is_pointer_button_down_on()
                    || response.clicked();
                let start = origin.or(pointer).and_then(document_point);

                if self.tool == Tool::Eyedropper {
                    if let Some(point) = response.interact_pointer_pos().and_then(document_point)
                        && let Some(action) =
                            self.shortcuts
                                .mouse
                                .click(&response, Scope::Eyedropper, false)
                    {
                        let sample_comp =
                            self.eyedropper_composite || action == MouseAction::EyedropperComposite;
                        let px = self.tab().editor.sample_pixel(point, sample_comp);
                        if action == MouseAction::EyedropperSecondary {
                            self.palette.set_secondary(px);
                        } else {
                            self.palette.set_primary(px);
                        }
                        if !self.eyedropper_stay {
                            let back = self.last_paint_tool;
                            self.activate_tool(back);
                        }
                        ui.ctx().request_repaint();
                    }
                } else if self.tool == Tool::Bucket {
                    if let Some(point) = response.interact_pointer_pos().and_then(document_point)
                        && let Some(action) =
                            self.shortcuts.mouse.click(&response, Scope::Bucket, false)
                    {
                        if self
                            .tab()
                            .editor
                            .selection
                            .as_ref()
                            .is_some_and(|selection| selection.coverage_u8(point) == 0)
                        {
                            self.clear_canvas_selection();
                            preview = None;
                        } else {
                            let (color, contiguous) = match action {
                                MouseAction::BucketPrimary => {
                                    (self.palette.primary(), self.bucket_contiguous)
                                }
                                MouseAction::BucketSecondary => {
                                    (self.palette.secondary(), self.bucket_contiguous)
                                }
                                MouseAction::BucketGlobalPrimary => (self.palette.primary(), false),
                                MouseAction::BucketGlobalSecondary => {
                                    (self.palette.secondary(), false)
                                }
                                _ => (self.palette.primary(), self.bucket_contiguous),
                            };
                            self.bucket_fill(point, color, contiguous);
                        }
                        ui.ctx().request_repaint();
                    }
                } else if self.tool == Tool::Wand {
                    if let Some(start) = response.interact_pointer_pos().and_then(document_point)
                        && let Some(action) =
                            self.shortcuts.mouse.click(&response, Scope::Wand, false)
                    {
                        let (mode, contiguous) = match action {
                            MouseAction::Wand => (self.selection_combine, self.wand_contiguous),
                            MouseAction::WandAdd => (Combine::Add, self.wand_contiguous),
                            MouseAction::WandSubtract => (Combine::Subtract, self.wand_contiguous),
                            MouseAction::WandXor => (Combine::Xor, self.wand_contiguous),
                            MouseAction::WandIntersect => {
                                (Combine::Intersect, self.wand_contiguous)
                            }
                            MouseAction::WandGlobal => (self.selection_combine, false),
                            MouseAction::WandGlobalAdd => (Combine::Add, false),
                            MouseAction::WandGlobalSubtract => (Combine::Subtract, false),
                            MouseAction::WandGlobalXor => (Combine::Xor, false),
                            MouseAction::WandGlobalIntersect => (Combine::Intersect, false),
                            _ => (self.selection_combine, self.wand_contiguous),
                        };
                        self.finish_opacity();
                        self.commit_selection_with(
                            self.wand(start, contiguous),
                            mode,
                            self.tool.label_key(),
                        );
                        preview = self.tab().editor.selection.clone();
                        ui.ctx().request_repaint();
                    }
                } else if matches!(self.tool, Tool::Select(_) | Tool::Lasso) {
                    if can_begin
                        && self.tab().selection_gesture.is_none()
                        && let Some(start_pt) = start
                    {
                        let pressed = self.shortcuts.mouse.pressed_in(ui.ctx(), Scope::Selection);
                        if let Some((button, action)) = pressed {
                            let (combine, constrain) = match action {
                                MouseAction::Select => (self.selection_combine, false),
                                MouseAction::SelectSquare => (self.selection_combine, true),
                                MouseAction::SelectAdd => (Combine::Add, false),
                                MouseAction::SelectSubtract => (Combine::Subtract, false),
                                MouseAction::SelectXor => (Combine::Xor, false),
                                MouseAction::SelectIntersect => (Combine::Intersect, false),
                                _ => (self.selection_combine, false),
                            };
                            let world = origin
                                .or(pointer)
                                .map_or(vec2(start_pt.0 as f32, start_pt.1 as f32), |p| {
                                    viewport.to_world(p)
                                });
                            self.finish_opacity();
                            let tab = self.tab_mut();
                            tab.editor.finish_stroke();
                            tab.selection_gesture = Some(SelectionGesture {
                                anchor: start_pt,
                                anchor_f: vec2(start_pt.0 as f32, start_pt.1 as f32),
                                last_world: world,
                                button,
                                combine,
                                constrain,
                            });
                            tab.selection_path.clear();
                            tab.selection_path.push(start_pt);
                        }
                    }

                    if let Some((btn, mode, initial_constrain)) = self
                        .tab()
                        .selection_gesture
                        .as_ref()
                        .map(|g| (g.button, g.combine, g.constrain))
                    {
                        let (down, released) = ui.input(|input| {
                            (
                                input.pointer.button_down(btn),
                                input.pointer.button_released(btn),
                            )
                        });
                        if let Some(position) = pointer {
                            let pixel = viewport.to_world(position);
                            let relocating = matches!(self.tool, Tool::Select(_))
                                && ui.ctx().input(|i| {
                                    self.shortcuts
                                        .mouse
                                        .is_gesture_down(MouseAction::SelectionRelocate, i)
                                });
                            if let Some(g) = self.tab_mut().selection_gesture.as_mut() {
                                if relocating {
                                    let delta = pixel - g.last_world;
                                    g.anchor_f.x =
                                        (g.anchor_f.x + delta.x).clamp(0.0, (width - 1) as f32);
                                    g.anchor_f.y =
                                        (g.anchor_f.y + delta.y).clamp(0.0, (height - 1) as f32);
                                    g.anchor =
                                        (g.anchor_f.x.round() as u32, g.anchor_f.y.round() as u32);
                                }
                                g.last_world = pixel;
                            }
                            let anchor = self.tab().selection_gesture.as_ref().unwrap().anchor;
                            let mut end = (
                                (pixel.x.max(0.0) as u32).min(width - 1),
                                (pixel.y.max(0.0) as u32).min(height - 1),
                            );
                            let constrain = initial_constrain
                                || modifiers.shift
                                || self.shortcuts.mouse.at(
                                    ui.ctx(),
                                    Scope::Selection,
                                    Trigger::Button(btn),
                                    modifiers,
                                ) == Some(MouseAction::SelectSquare);
                            if constrain && matches!(self.tool, Tool::Select(_)) {
                                end = constrain_square_point(anchor, end, (width, height));
                            }
                            match self.tool {
                                Tool::Select(shape) => {
                                    let candidate = Selection::new_with_aa(
                                        shape,
                                        anchor,
                                        end,
                                        self.settings.antialiasing,
                                    );
                                    if released {
                                        self.commit_selection_with(
                                            Ok(candidate),
                                            mode,
                                            self.tool.label_key(),
                                        );
                                        preview = self.tab().editor.selection.clone();
                                    } else {
                                        preview = self.selection_candidate(candidate, mode).ok();
                                    }
                                }
                                Tool::Lasso => {
                                    if self.tab().selection_path.last() != Some(&end) {
                                        self.tab_mut().selection_path.push(end);
                                    }
                                    if released
                                        || (self.selection_feather > 0
                                            && self.tab().selection_path.len() >= 3)
                                    {
                                        let selection = Selection::lasso_with_aa(
                                            (width, height),
                                            &self.tab().selection_path,
                                            self.settings.antialiasing,
                                        );
                                        if released {
                                            self.commit_selection_with(
                                                selection,
                                                mode,
                                                self.tool.label_key(),
                                            );
                                            preview = self.tab().editor.selection.clone();
                                        } else {
                                            preview = selection
                                                .and_then(|candidate| {
                                                    self.selection_candidate(candidate, mode)
                                                })
                                                .ok();
                                        }
                                    } else if self.tab().selection_path.len() >= 2 {
                                        let points = self
                                            .tab()
                                            .selection_path
                                            .iter()
                                            .map(|&(x, y)| {
                                                viewport
                                                    .to_screen(vec2(x as f32 + 0.5, y as f32 + 0.5))
                                            })
                                            .collect::<Vec<_>>();
                                        paint_dashed_line(&painter, &points);
                                    }
                                }
                                _ => unreachable!(),
                            }
                        }
                        if released {
                            ui.ctx().request_repaint();
                        }
                        if !down || released {
                            let tab = self.tab_mut();
                            tab.selection_gesture = None;
                            tab.selection_path.clear();
                        }
                    }
                } else if matches!(
                    self.tool,
                    Tool::Gradient | Tool::LineCurve | Tool::DrawShape | Tool::Text
                ) {
                    if can_begin
                        && let Some(pos) = origin.or(pointer)
                        && document_point(pos).is_some()
                    {
                        let scope = match self.tool {
                            Tool::Gradient => Scope::Gradient,
                            Tool::LineCurve => Scope::LineCurve,
                            Tool::DrawShape => Scope::ShapeTool,
                            Tool::Text => Scope::Text,
                            _ => unreachable!(),
                        };
                        let pressed = self.shortcuts.mouse.pressed_in(ui.ctx(), scope);
                        if let Some((btn, action)) = pressed {
                            self.finish_opacity();
                            let world_pt = viewport.to_world(pos);
                            let created = match (self.tool, action) {
                                (
                                    Tool::Gradient,
                                    MouseAction::GradientPrimary | MouseAction::GradientSecondary,
                                ) => Some((
                                    ActiveSession::Gradient(GradientSession::new(
                                        world_pt,
                                        world_pt,
                                        self.palette.primary(),
                                        self.palette.secondary(),
                                        action == MouseAction::GradientSecondary,
                                        self.gradient_params(),
                                    )),
                                    true,
                                )),
                                (
                                    Tool::LineCurve,
                                    MouseAction::LinePrimary | MouseAction::LineSecondary,
                                ) => {
                                    let color =
                                        self.palette.color(action == MouseAction::LineSecondary);
                                    Some((
                                        ActiveSession::LineCurve(LineCurveSession::new(
                                            world_pt,
                                            world_pt,
                                            color,
                                            self.line_curve_params(),
                                        )),
                                        true,
                                    ))
                                }
                                (
                                    Tool::DrawShape,
                                    MouseAction::ShapePrimary | MouseAction::ShapeSecondary,
                                ) => {
                                    let (outline, fill) =
                                        self.palette.pair(action == MouseAction::ShapeSecondary);
                                    Some((
                                        ActiveSession::Shape(ShapeSession::new(
                                            world_pt,
                                            world_pt,
                                            outline,
                                            fill,
                                            self.shape_params(),
                                        )),
                                        true,
                                    ))
                                }
                                (Tool::Text, MouseAction::TextPlace) => Some((
                                    ActiveSession::Text(TextSession::new(
                                        world_pt,
                                        self.palette.primary(),
                                        self.text_params(),
                                    )),
                                    false,
                                )),
                                _ => None,
                            };
                            if let Some((session, drag)) = created {
                                let tab = self.tab_mut();
                                tab.session = Some(session);
                                tab.session_creation = drag.then_some((btn, world_pt));
                                tab.editor.mark_full_refresh();
                            }
                            ui.ctx().request_repaint();
                        }
                    }
                } else {
                    self.tab_mut().selection_gesture = None;
                    if self.tool == Tool::CloneStamp
                        && let Some(anchor) = self.tab().clone_anchor
                    {
                        let src_pt = if let (Some(off), Some(pos)) =
                            (self.tab().clone_offset, pointer.and_then(document_point))
                        {
                            vec2(
                                (i64::from(pos.0) + off.0) as f32 + 0.5,
                                (i64::from(pos.1) + off.1) as f32 + 0.5,
                            )
                        } else {
                            vec2(anchor.0 as f32 + 0.5, anchor.1 as f32 + 0.5)
                        };
                        let sc = viewport.to_screen(src_pt);
                        let r = (self.diameter as f32 * 0.5 * zoom).max(4.0);
                        painter.circle_stroke(sc, r, egui::Stroke::new(1.0_f32, Color32::WHITE));
                        painter.line_segment(
                            [sc - vec2(5.0, 0.0), sc + vec2(5.0, 0.0)],
                            egui::Stroke::new(1.0_f32, Color32::BLACK),
                        );
                        painter.line_segment(
                            [sc - vec2(0.0, 5.0), sc + vec2(0.0, 5.0)],
                            egui::Stroke::new(1.0_f32, Color32::BLACK),
                        );
                    }
                    if can_begin
                        && self.tab().stroke_gesture.is_none()
                        && let Some(start_pt) = start
                    {
                        let scope = match self.tool {
                            Tool::Eraser => Scope::Eraser,
                            Tool::Pencil => Scope::Pencil,
                            Tool::Recolor => Scope::Recolor,
                            Tool::CloneStamp => Scope::CloneStamp,
                            _ => Scope::Brush,
                        };
                        let pressed = self.shortcuts.mouse.pressed_in(ui.ctx(), scope);
                        if let Some((btn, action)) = pressed {
                            match action {
                                MouseAction::BrushOptions | MouseAction::EraserOptions => {
                                    self.tool_size_popup = true;
                                }
                                MouseAction::BrushSampleColor => {
                                    let px = self.tab().editor.sample_pixel(start_pt, false);
                                    if btn == PointerButton::Secondary {
                                        self.palette.set_secondary(px);
                                    } else {
                                        self.palette.set_primary(px);
                                    }
                                }
                                MouseAction::RecolorSample => {
                                    let px = self.tab().editor.sample_pixel(start_pt, false);
                                    self.recolor_target = Some(px);
                                }
                                MouseAction::CloneSetAnchor => {
                                    let tab = self.tab_mut();
                                    tab.clone_anchor = Some(start_pt);
                                    tab.clone_offset = None;
                                    ui.ctx().request_repaint();
                                }
                                MouseAction::DrawPrimary
                                | MouseAction::DrawSecondary
                                | MouseAction::PencilPrimary
                                | MouseAction::PencilSecondary
                                | MouseAction::Erase
                                | MouseAction::RecolorPrimary
                                | MouseAction::RecolorSecondary
                                | MouseAction::ClonePaint => {
                                    let spec_and_size = match action {
                                        MouseAction::Erase => Some((
                                            StrokeSpec::Paint {
                                                primary: Pixel::default(),
                                                secondary: Pixel::default(),
                                                pattern: FillPattern::Solid,
                                                blend: StrokeBlend::Normal,
                                                hardness: self.hardness,
                                                spacing: self.spacing,
                                                is_eraser: true,
                                            },
                                            self.diameter,
                                        )),
                                        MouseAction::PencilPrimary
                                        | MouseAction::PencilSecondary => {
                                            let primary = self
                                                .palette
                                                .color(action == MouseAction::PencilSecondary);
                                            Some((
                                                StrokeSpec::Paint {
                                                    primary,
                                                    secondary: self.palette.secondary(),
                                                    pattern: FillPattern::Solid,
                                                    blend: self.stroke_blend,
                                                    hardness: 100,
                                                    spacing: 15,
                                                    is_eraser: false,
                                                },
                                                self.pencil_diameter,
                                            ))
                                        }
                                        MouseAction::DrawPrimary | MouseAction::DrawSecondary => {
                                            let (primary, secondary) = self
                                                .palette
                                                .pair(action == MouseAction::DrawSecondary);
                                            Some((
                                                StrokeSpec::Paint {
                                                    primary,
                                                    secondary,
                                                    pattern: self.fill_pattern,
                                                    blend: self.stroke_blend,
                                                    hardness: self.hardness,
                                                    spacing: self.spacing,
                                                    is_eraser: false,
                                                },
                                                self.diameter,
                                            ))
                                        }
                                        MouseAction::RecolorPrimary
                                        | MouseAction::RecolorSecondary => {
                                            let replacement = self
                                                .palette
                                                .color(action == MouseAction::RecolorSecondary);
                                            let target = self
                                                .recolor_target
                                                .unwrap_or(self.palette.secondary());
                                            Some((
                                                StrokeSpec::Recolor {
                                                    target,
                                                    replacement,
                                                    tolerance_pct: self.color_tolerance,
                                                    alpha_mode: self.alpha_match,
                                                    hardness: self.hardness,
                                                    spacing: self.spacing,
                                                },
                                                self.diameter,
                                            ))
                                        }
                                        MouseAction::ClonePaint => {
                                            if let Some(anchor) = self.tab().clone_anchor {
                                                let offset = self.tab().clone_offset.unwrap_or((
                                                    i64::from(anchor.0) - i64::from(start_pt.0),
                                                    i64::from(anchor.1) - i64::from(start_pt.1),
                                                ));
                                                self.tab_mut().clone_offset = Some(offset);
                                                let selected = self.tab().editor.selected;
                                                let source_pixels = self
                                                    .tab()
                                                    .editor
                                                    .document
                                                    .layer_pixels(selected)
                                                    .expect("active layer")
                                                    .to_vec();
                                                Some((
                                                    StrokeSpec::Clone {
                                                        source_pixels,
                                                        source_size: (width, height),
                                                        offset,
                                                        blend: self.stroke_blend,
                                                        hardness: self.hardness,
                                                        spacing: self.spacing,
                                                    },
                                                    self.diameter,
                                                ))
                                            } else {
                                                self.notice = Some((
                                                    i18n::format(
                                                        "clone-source-hint",
                                                        &[&self.shortcuts.mouse_hint(
                                                            MouseAction::CloneSetAnchor,
                                                        )],
                                                    ),
                                                    false,
                                                ));
                                                None
                                            }
                                        }
                                        _ => None,
                                    };
                                    if let Some((spec, diameter)) = spec_and_size {
                                        self.finish_opacity();
                                        let aa_mode = self.settings.antialiasing;
                                        let tab = self.tab_mut();
                                        let waiting_for_drag =
                                            tab.editor.selection.as_ref().is_some_and(
                                                |selection| selection.coverage_u8(start_pt) == 0,
                                            );
                                        tab.stroke_gesture = Some(StrokeGesture {
                                            button: btn,
                                            waiting_for_drag,
                                        });
                                        tab.editor.begin_stroke_with(spec, diameter, aa_mode);
                                        if !waiting_for_drag {
                                            tab.editor.draw_to(Some(start_pt));
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    if let Some(StrokeGesture {
                        button,
                        waiting_for_drag,
                    }) = self.tab().stroke_gesture
                    {
                        let (down, released) = ui.input(|input| {
                            (
                                input.pointer.button_down(button),
                                input.pointer.button_released(button),
                            )
                        });
                        if (down || released)
                            && (!waiting_for_drag
                                || response.dragged_by(button)
                                || response.drag_stopped_by(button))
                        {
                            let tab = self.tab_mut();
                            tab.stroke_gesture.as_mut().unwrap().waiting_for_drag = false;
                            tab.editor.draw_to(pointer.and_then(document_point));
                        }
                        if !down || released {
                            let was_drawing = self.tab().editor.has_edits();
                            self.tab_mut().editor.finish_stroke();
                            self.tab_mut().stroke_gesture = None;
                            if released {
                                if was_drawing {
                                    self.tab_mut().thumbs_dirty = true;
                                }
                                ui.ctx().request_repaint();
                            }
                        }
                    }
                }
            }
            if let Some(selection) = preview {
                paint_selection_overlay(&painter, rect, zoom, (width, height), &selection);
            }
            let dpi = self.tab().editor.document.resolution_dpi();
            self.tab().view.guides(ui, rect, zoom, dpi);
            if !modal_open {
                self.paint_stroke_size_cursor(&response, &canvas_overlay, zoom);
            }
        });
        self.tab_mut().view.offset = output.state.offset;
        zoom
    }

    fn paint_canvas_guides(
        &self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        rect: Rect,
        zoom: f32,
        size: Point,
    ) {
        let tab = self.tab();
        if let Some(selection) = tab.editor.selection.as_ref() {
            paint_selection_overlay(painter, rect, zoom, size, selection);
        }
        tab.view
            .guides(ui, rect, zoom, tab.editor.document.resolution_dpi());
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.tool_size_popup {
            let mut open = true;
            egui::Window::new(tr("stroke-size"))
                .id(egui::Id::new("stroke_size"))
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.add(
                        egui::Slider::new(&mut self.diameter, 1..=MAX_BRUSH_DIAMETER)
                            .suffix(" px")
                            .text(tr("size")),
                    );
                });
            if !open {
                self.tool_size_popup = false;
            }
        }

        if let Some(pending) = &self.pending_close
            && !pending.waiting_save_dialog
        {
            let scope = pending.scope;
            let tab_index = pending.tab_index;
            let doc_name = self.tabs[tab_index].title();
            let response = modal(ctx, "discard").show(ctx, |ui| {
                ui.heading(i18n::format("save-changes", &[&doc_name]));
                ui.horizontal(|ui| {
                    (
                        ui.button(tr("save-continue")).clicked(),
                        ui.button(tr("discard-continue")).clicked(),
                        ui.button(tr("cancel")).clicked(),
                    )
                })
                .inner
            });
            let (save, discard, cancel) = response.inner;
            if discard {
                self.pending_close = None;
                self.advance_close_flow(ctx, scope, tab_index);
            } else if save {
                self.switch_tab(tab_index);
                if self.save_project(ctx, false) {
                    self.pending_close = None;
                    self.advance_close_flow(ctx, scope, tab_index);
                } else if self.save_dialog.is_some()
                    && let Some(p) = &mut self.pending_close
                {
                    p.waiting_save_dialog = true;
                }
            } else if cancel || response.should_close() {
                self.pending_close = None;
            }
        }

        if let Some(mut dialog) = self.save_dialog.take() {
            let action = dialog.show(ctx, &self.tab().editor.document, &self.checker);
            match action {
                SaveDialogAction::Open => {
                    self.save_dialog = Some(dialog);
                }
                SaveDialogAction::Cancel => {
                    if let Some(pending) = &self.pending_close
                        && pending.waiting_save_dialog
                    {
                        self.pending_close = None;
                    }
                }
                SaveDialogAction::Save => {
                    let result = dialog.write_to_disk();
                    let ok = result.is_ok();
                    let is_project = dialog.is_project();
                    let path = dialog.path;
                    if ok && is_project {
                        self.mark_project_saved(path, true);
                    }
                    self.report(
                        result,
                        if is_project {
                            tr("project-saved")
                        } else {
                            tr("png-exported")
                        },
                    );
                    if let Some(pending) = self.pending_close.take()
                        && ok
                        && pending.waiting_save_dialog
                    {
                        self.advance_close_flow(ctx, pending.scope, pending.tab_index);
                    }
                }
            }
        }

        if let Some(mut dialog) = self.layer_dialog.take() {
            match dialog.show(ctx) {
                LayerDialogAction::Open(live_changed) => {
                    if live_changed {
                        let layer = dialog.layer;
                        let mut preview_meta = dialog.draft.clone();
                        if !is_valid_layer_name(&preview_meta.name) {
                            preview_meta.name = dialog.original.name.clone();
                        }
                        let _ = self.tab_mut().editor.set_layer_meta(layer, preview_meta);
                    }
                    self.layer_dialog = Some(dialog);
                }
                LayerDialogAction::Confirm => {
                    let layer = dialog.layer;
                    let result = self.tab_mut().editor.set_layer_meta(layer, dialog.draft);
                    if result.is_ok() {
                        self.tab_mut().editor.record_layer_meta(
                            layer,
                            dialog.original,
                            "layer-properties",
                        );
                        self.tab_mut().thumbs_dirty = true;
                    }
                    self.report(result, "");
                }
                LayerDialogAction::Cancel => {
                    let _ = self
                        .tab_mut()
                        .editor
                        .set_layer_meta(dialog.layer, dialog.original);
                }
            }
        }

        if let Some((mut width, mut height)) = self.new_size {
            let limit = max_canvas_side(ctx);
            let response = modal(ctx, "new").show(ctx, |ui| {
                ui.heading(tr("new-image"));
                ui.horizontal(|ui| {
                    dimension_inputs(ui, &mut width, &mut height, limit, true);
                });
                ui.horizontal(|ui| {
                    (
                        ui.button(tr("create")).clicked(),
                        ui.button(tr("cancel")).clicked(),
                    )
                })
                .inner
            });
            if response.inner.0 {
                match new_document(width, height) {
                    Ok(document) => self.open_in_new_tab(document, String::new(), None),
                    Err(error) => self.report(Err(error), ""),
                }
                self.new_size = None;
            } else if response.inner.1 || response.should_close() {
                self.new_size = None;
            } else {
                self.new_size = Some((width, height));
            }
        }
    }
}

fn constrain_square_point(anchor: Point, end: Point, size: Point) -> Point {
    let dx = end.0 as i64 - anchor.0 as i64;
    let dy = end.1 as i64 - anchor.1 as i64;
    let side = dx.abs().max(dy.abs());
    let sx = if dx >= 0 { 1 } else { -1 };
    let sy = if dy >= 0 { 1 } else { -1 };
    (
        (anchor.0 as i64 + sx * side).clamp(0, i64::from(size.0) - 1) as u32,
        (anchor.1 as i64 + sy * side).clamp(0, i64::from(size.1) - 1) as u32,
    )
}

fn paint_dashed_line(painter: &egui::Painter, points: &[egui::Pos2]) {
    painter.add(egui::Shape::line(
        points.to_vec(),
        egui::Stroke::new(2.0_f32, Color32::BLACK),
    ));
    painter.extend(egui::Shape::dashed_line(
        points,
        egui::Stroke::new(1.0_f32, Color32::WHITE),
        4.0,
        4.0,
    ));
}

fn paint_selection_overlay(
    painter: &egui::Painter,
    canvas: Rect,
    zoom: f32,
    size: Point,
    selection: &Selection,
) {
    let clip = painter.clip_rect();
    let from = (clip.min - canvas.min) / zoom;
    let to = (clip.max - canvas.min) / zoom;
    let start = (from.x.floor() as u32, from.y.floor() as u32);
    let end = (
        (to.x.ceil() as u32).min(size.0),
        (to.y.ceil() as u32).min(size.1),
    );
    let stride = (2.0 / zoom).ceil().max(1.0) as u32;

    match &selection.region {
        Region::Shape(shape, (min, max)) => {
            let bounds = Rect::from_min_max(
                canvas.min + zoom * vec2(min.0 as f32, min.1 as f32),
                canvas.min + zoom * vec2(max.0 as f32 + 1.0, max.1 as f32 + 1.0),
            );
            let stride = (1.0 / zoom).ceil().max(1.0) as u32;
            if selection.inverted {
                paint_selection_coverage(painter, canvas, zoom, (start, end), stride, selection);
            } else if *shape == Shape::Rectangle {
                painter.rect_filled(bounds, 0, selection_tint(255 / 3));
            } else {
                // Restrict ellipse coverage sampling to its visible bounds.
                let start = (start.0.max(min.0), start.1.max(min.1));
                let end = (end.0.min(max.0 + 1), end.1.min(max.1 + 1));
                paint_selection_coverage(painter, canvas, zoom, (start, end), stride, selection);
            }
            match shape {
                Shape::Rectangle => paint_dashed_line(painter, &rectangle_outline(bounds)),
                Shape::Ellipse => {
                    let points = (0..=96)
                        .map(|step| {
                            let angle = std::f32::consts::TAU * step as f32 / 96.0;
                            bounds.center() + Vec2::angled(angle) * (bounds.size() * 0.5)
                        })
                        .collect::<Vec<_>>();
                    paint_dashed_line(painter, &points);
                }
            }
        }
        Region::Mask(_) => {
            paint_selection_coverage(painter, canvas, zoom, (start, end), stride, selection);
            for y in (start.1..end.1).step_by(stride as usize) {
                for x in (start.0..end.0).step_by(stride as usize) {
                    let here = selection.contains((x, y));
                    let top_left = canvas.min + zoom * vec2(x as f32, y as f32);
                    let cell = vec2(
                        (size.0 - x).min(stride) as f32,
                        (size.1 - y).min(stride) as f32,
                    ) * zoom;
                    let color = if (x / stride + y / stride).is_multiple_of(2) {
                        Color32::BLACK
                    } else {
                        Color32::WHITE
                    };
                    let stroke = egui::Stroke::new(1.0_f32, color);
                    if (x >= stride && selection.contains((x - stride, y))) != here {
                        painter.line_segment([top_left, top_left + vec2(0.0, cell.y)], stroke);
                    }
                    if (y >= stride && selection.contains((x, y - stride))) != here {
                        painter.line_segment([top_left, top_left + vec2(cell.x, 0.0)], stroke);
                    }
                    if here && x.saturating_add(stride) >= size.0 {
                        let right = top_left + vec2(cell.x, 0.0);
                        painter.line_segment([right, right + vec2(0.0, cell.y)], stroke);
                    }
                    if here && y.saturating_add(stride) >= size.1 {
                        let bottom = top_left + vec2(0.0, cell.y);
                        painter.line_segment([bottom, bottom + vec2(cell.x, 0.0)], stroke);
                    }
                }
            }
        }
    }
    if selection.inverted {
        let rect = canvas.shrink(1.0);
        if rect.is_positive() {
            paint_dashed_line(painter, &rectangle_outline(rect));
        }
    }
}

fn selection_tint(opacity: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(105, 80, 230, opacity)
}

fn paint_selection_coverage(
    painter: &egui::Painter,
    canvas: Rect,
    zoom: f32,
    (start, end): (Point, Point),
    stride: u32,
    selection: &Selection,
) {
    let mut mesh = egui::Mesh::default();
    for y in (start.1..end.1).step_by(stride as usize) {
        let mut x = start.0;
        while x < end.0 {
            let opacity = selection.coverage_u8((x, y)).div_ceil(3);
            let mut next = x.saturating_add(stride);
            while next < end.0 && selection.coverage_u8((next, y)).div_ceil(3) == opacity {
                next = next.saturating_add(stride);
            }
            if opacity > 0 {
                let top_left = canvas.min + zoom * vec2(x as f32, y as f32);
                let bottom_right = canvas.min
                    + zoom
                        * vec2(
                            next.min(end.0) as f32,
                            y.saturating_add(stride).min(end.1) as f32,
                        );
                mesh.add_colored_rect(
                    Rect::from_min_max(top_left, bottom_right),
                    selection_tint(opacity),
                );
            }
            x = next;
        }
    }
    if !mesh.is_empty() {
        painter.add(egui::Shape::mesh(mesh));
    }
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
}

fn pick_image_file(title: &str) -> Option<PathBuf> {
    FileDialog::new()
        .set_title(title)
        .add_filter(tr("supported-files"), &[PDN_EXT, "png"])
        .add_filter(tr("png-image"), &["png"])
        .add_filter(tr("pdn-document"), &["pdn"])
        .pick_file()
}

fn dnd_reorder(
    ui: &mut egui::Ui,
    rect: Rect,
    id: impl std::hash::Hash,
    index: usize,
) -> Option<(usize, usize)> {
    let drag = ui.interact(rect, ui.id().with(id), egui::Sense::drag());
    drag.dnd_set_drag_payload(index);
    drag.dnd_release_payload::<usize>()
        .map(|from| (*from, index))
        .filter(|(from, to)| from != to)
}

fn new_document(width: u32, height: u32) -> io::Result<Document> {
    let mut document = Document::new(width, height)?;
    document.set_layer_name(0, i18n::logical_text("background"))?;
    document.fill_layer(0, Pixel([255, 255, 255, 255]))?;
    Ok(document)
}

fn open_document(path: &Path) -> io::Result<(Document, bool)> {
    let file = BufReader::new(File::open(path)?);
    let pdn = has_extension(path, PDN_EXT);
    let document = if pdn {
        Document::read_pdn(file)?
    } else {
        let mut document = Document::read_png(file)?;
        document.set_layer_name(0, i18n::logical_text("background"))?;
        document
    };
    Ok((document, pdn))
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

fn write_pdn_atomically(document: &Document, path: &Path, replace: bool) -> io::Result<()> {
    let mut bytes = Vec::new();
    document.write_pdn(&mut bytes)?;
    storage::atomic_write_bytes(path, &bytes, replace)
}

fn arrow_key_dir(input: &egui::InputState) -> (i64, i64) {
    let mut dx = 0_i64;
    let mut dy = 0_i64;
    if input.key_pressed(egui::Key::ArrowLeft) {
        dx -= 1;
    }
    if input.key_pressed(egui::Key::ArrowRight) {
        dx += 1;
    }
    if input.key_pressed(egui::Key::ArrowUp) {
        dy -= 1;
    }
    if input.key_pressed(egui::Key::ArrowDown) {
        dy += 1;
    }
    (dx, dy)
}

impl App {
    fn cancel_active_session(&mut self, ctx: &egui::Context) {
        let tab = self.tab_mut();
        tab.session_creation = None;
        if let Some(session) = tab.session.take() {
            if let ActiveSession::Transform(transform) = session
                && transform.floating.is_some()
            {
                let _ = tab.editor.undo();
            }
            tab.editor.mark_full_refresh();
            ctx.request_repaint();
        }
    }

    fn ui_frame(&mut self, ctx: &egui::Context) {
        self.locale.with(|| self.localized_frame(ctx));
    }

    fn localized_frame(&mut self, ctx: &egui::Context) {
        if let Some(error) = self.palette.take_save_error() {
            self.show_error(i18n::format("palette-save-error", &[&error]));
        }
        if let Some(error) = self.recent.take_save_error() {
            self.show_error(i18n::format("recent-save-error", &[&error]));
        }
        let os_fullscreen = ctx.input(|input| input.viewport().fullscreen);
        if os_fullscreen != self.observed_fullscreen {
            self.observed_fullscreen = os_fullscreen;
            if let Some(full) = os_fullscreen {
                self.fullscreen = full;
            }
        }
        if !ctx.input(|input| input.pointer.primary_down()) {
            self.finish_opacity();
        }
        if ctx.input(|input| input.viewport().close_requested()) && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request(ctx, Action::Close);
        }
        let mut handled_session_key = false;
        let escape = !self.has_modal_open()
            && !egui::Popup::is_any_open(ctx)
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if escape {
            if self.colors_open {
                self.colors_open = false;
                self.color_popup_rect = None;
                ctx.request_repaint();
            } else if self.has_active_session() {
                self.cancel_active_session(ctx);
            } else {
                self.clear_canvas_selection();
                ctx.request_repaint();
            }
            handled_session_key = true;
        }
        if !escape
            && !self.has_modal_open()
            && !ctx.wants_keyboard_input()
            && self
                .tab()
                .session
                .as_ref()
                .is_some_and(|s| s.as_text().is_some())
        {
            let mut text_changed = false;
            let mut commit_text = false;
            let mut inserted = String::new();
            let mut backspace = false;
            let (shift_enter, plain_enter) = ctx.input(|input| {
                (
                    input.key_pressed(egui::Key::Enter) && input.modifiers.shift,
                    input.key_pressed(egui::Key::Enter)
                        && !input.modifiers.shift
                        && !input.modifiers.ctrl
                        && !input.modifiers.alt,
                )
            });
            if shift_enter {
                inserted.push('\n');
            } else if plain_enter {
                commit_text = true;
            } else {
                ctx.input(|input| {
                    if !input.modifiers.ctrl && !input.modifiers.alt && !input.modifiers.command {
                        for event in &input.events {
                            if let egui::Event::Text(s) = event {
                                inserted.push_str(s);
                            }
                        }
                        if input.key_pressed(egui::Key::Backspace) {
                            backspace = true;
                        }
                    }
                });
            }
            if commit_text {
                self.commit_active_session();
                ctx.request_repaint();
            } else if let Some(text) = self
                .tab_mut()
                .session
                .as_mut()
                .and_then(ActiveSession::as_text_mut)
            {
                if backspace && text.text.pop().is_some() {
                    text_changed = true;
                }
                if !inserted.is_empty() {
                    text.text.push_str(&inserted);
                    text_changed = true;
                }
                let (dx, dy) = ctx.input(|input| {
                    if !input.modifiers.ctrl && !input.modifiers.command {
                        return (0.0_f32, 0.0_f32);
                    }
                    let (ax, ay) = arrow_key_dir(input);
                    (ax as f32 * 5.0, ay as f32 * 5.0)
                });
                if dx != 0.0 || dy != 0.0 {
                    text.nudge(dx, dy);
                    text_changed = true;
                }
                if text_changed {
                    self.tab_mut().editor.mark_full_refresh();
                    ctx.request_repaint();
                }
            }
            let has_shortcut_mod =
                ctx.input(|i| i.modifiers.ctrl || i.modifiers.alt || i.modifiers.command);
            handled_session_key = !has_shortcut_mod;
        } else if !escape
            && !self.has_modal_open()
            && !ctx.wants_keyboard_input()
            && let Some(prev_ctrl) = self
                .tab()
                .session
                .as_ref()
                .and_then(ActiveSession::as_canvas_resize)
                .map(|cr| cr.prev_ctrl)
        {
            let cur_ctrl = ctx.input(|input| input.modifiers.ctrl);
            let ctrl_just_pressed = !prev_ctrl && cur_ctrl;
            if let Some(cr) = self
                .tab_mut()
                .session
                .as_mut()
                .and_then(ActiveSession::as_canvas_resize_mut)
            {
                cr.prev_ctrl = cur_ctrl;
            }
            if ctrl_just_pressed || ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
                self.commit_active_session();
                ctx.request_repaint();
            } else {
                let limit = i64::from(max_canvas_side(ctx));
                let (dx, dy) = ctx.input(|input| {
                    let step = if input.modifiers.shift { 10 } else { 1 };
                    let (ax, ay) = arrow_key_dir(input);
                    (ax * step, ay * step)
                });
                if (dx != 0 || dy != 0)
                    && let Some(cr) = self
                        .tab_mut()
                        .session
                        .as_mut()
                        .and_then(ActiveSession::as_canvas_resize_mut)
                {
                    cr.preview_size.0 = (i64::from(cr.preview_size.0) + dx).clamp(1, limit) as u32;
                    cr.preview_size.1 = (i64::from(cr.preview_size.1) + dy).clamp(1, limit) as u32;
                    ctx.request_repaint();
                }
            }
            handled_session_key = true;
        } else if !escape
            && self.has_active_session()
            && !self.has_modal_open()
            && !ctx.wants_keyboard_input()
        {
            if ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
                self.commit_active_session();
                ctx.request_repaint();
                handled_session_key = true;
            } else {
                let (dx, dy) = ctx.input(|input| {
                    let step = if input.modifiers.ctrl || input.modifiers.command {
                        10.0_f32
                    } else {
                        1.0_f32
                    };
                    let (ax, ay) = arrow_key_dir(input);
                    (ax as f32 * step, ay as f32 * step)
                });
                if dx != 0.0 || dy != 0.0 {
                    let tab = self.tab_mut();
                    if let Some(session) = tab.session.as_mut() {
                        session.nudge(dx, dy);
                    }
                    tab.editor.mark_full_refresh();
                    ctx.request_repaint();
                    handled_session_key = true;
                }
            }
        }
        let focus = ctx.memory(|memory| memory.focused());
        let layers_focused = self.layers_clicked && focus == Some(egui::Id::new(LAYERS_ID));
        let editor_shortcuts = focus.is_none_or(|id| id == egui::Id::new(CANVAS_ID));
        if !handled_session_key
            && !self.has_modal_open()
            && (editor_shortcuts || (layers_focused && !egui::Popup::is_any_open(ctx)))
            && !ctx.input(|input| input.pointer.any_down())
        {
            for command in self.shortcuts.commands(ctx, layers_focused) {
                if let Some(transform) = self
                    .tab()
                    .session
                    .as_ref()
                    .and_then(ActiveSession::as_transform)
                    && command != Command::Deselect
                    && (!transform.affine.is_identity()
                        || !matches!(command, Command::MovePixels | Command::MoveMask))
                {
                    continue;
                }
                self.execute_command(ctx, command);
                if self.has_modal_open() {
                    break;
                }
            }
        }

        self.refresh_thumbnails(ctx);

        let workspace_frame = egui::Frame::new().fill(self.settings.theme.workspace());
        let window_radius = window_chrome::corner_radius(ctx, self.fullscreen);

        window_chrome::resize_edges(ctx, self.fullscreen);

        egui::TopBottomPanel::top("menubar")
            .frame(
                egui::Frame::new()
                    .fill(self.settings.theme.menubar_fill())
                    .corner_radius(egui::CornerRadius {
                        nw: window_radius,
                        ne: window_radius,
                        sw: 0,
                        se: 0,
                    })
                    .inner_margin(egui::Margin::symmetric(8, 3)),
            )
            .show_separator_line(false)
            .show(ctx, |ui| self.menu_bar(ui));

        egui::TopBottomPanel::top("command_bar")
            .frame(
                egui::Frame::new()
                    .fill(self.settings.theme.toolbar_fill())
                    .stroke(egui::Stroke::new(
                        1.0_f32,
                        self.settings.theme.border_color(),
                    ))
                    .inner_margin(egui::Margin::symmetric(10, 5)),
            )
            .show_separator_line(false)
            .show(ctx, |ui| self.command_bar(ui));

        let ribbon_frame = egui::Frame::new()
            .fill(self.settings.theme.toolbar_fill())
            .stroke(egui::Stroke::new(
                1.0_f32,
                self.settings.theme.border_color(),
            ))
            .inner_margin(egui::Margin::symmetric(6, 4));
        let ribbon = egui::TopBottomPanel::top("color_ribbon")
            // Include the stroke in the initial clip height and let the content size the panel.
            .default_height(colors::RIBBON_CONTENT_HEIGHT + ribbon_frame.total_margin().sum().y)
            .frame(ribbon_frame)
            .show_separator_line(false)
            .show(ctx, |ui| {
                colors::ribbon(
                    ui,
                    &mut self.palette,
                    self.colors_open,
                    &self.shortcuts.mouse,
                    self.settings.theme,
                    &self.checker,
                )
            })
            .inner;
        if ribbon.toggle_clicked {
            self.colors_open = !self.colors_open;
            if !self.colors_open {
                self.color_popup_rect = None;
            }
            ctx.request_repaint();
        }

        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(self.settings.theme.status_fill())
                    .stroke(egui::Stroke::new(
                        1.0_f32,
                        self.settings.theme.border_color(),
                    ))
                    .corner_radius(egui::CornerRadius {
                        nw: 0,
                        ne: 0,
                        sw: window_radius,
                        se: window_radius,
                    })
                    .inner_margin(egui::Margin::symmetric(10, 4)),
            )
            .show_separator_line(false)
            .show(ctx, |ui| {
                use tool_icons::{ActionIcon, icon_button};
                ui.horizontal(|ui| {
                    if let Some((notice, error)) = &self.notice {
                        let theme = self.settings.theme;
                        let mut color = if *error {
                            theme.error()
                        } else {
                            theme.accent()
                        };
                        let mut fill = Color32::TRANSPARENT;
                        if *error && let Some(flash) = &mut self.error_flash {
                            match flash.inverted(ui.ctx()) {
                                Some(true) => (color, fill) = (theme.status_fill(), color),
                                Some(false) => {}
                                None => self.error_flash = None,
                            }
                        }
                        // The frame is always present so that the text never moves.
                        egui::Frame::new()
                            .fill(fill)
                            .corner_radius(3)
                            .inner_margin(egui::Margin::symmetric(4, 0))
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(RichText::new(notice).size(12.0).color(color))
                                        .truncate(),
                                )
                                .on_hover_text(notice);
                            });
                    } else {
                        ui.label(self.settings.theme.muted_label(tr("ready")));
                    }

                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        if icon_button(
                            ui,
                            self.settings.theme,
                            ActionIcon::ZoomIn,
                            true,
                            false,
                            vec2(20.0, 20.0),
                            tr("zoom-enlarge"),
                        )
                        .clicked()
                        {
                            self.step_zoom(ZOOM_IN_FACTOR);
                        }
                        let mut scale = self.tab().display_zoom * 100.0;
                        ui.spacing_mut().slider_width = 76.0;
                        if ui
                            .add(
                                egui::Slider::new(&mut scale, 1.0..=3200.0)
                                    .logarithmic(true)
                                    .show_value(false)
                                    .clamping(egui::SliderClamping::Edits),
                            )
                            .on_hover_text(tr("continuous-zoom"))
                            .changed()
                        {
                            self.tab_mut().zoom = Some(scale / 100.0);
                        }
                        if icon_button(
                            ui,
                            self.settings.theme,
                            ActionIcon::ZoomOut,
                            true,
                            false,
                            vec2(20.0, 20.0),
                            tr("zoom-out"),
                        )
                        .clicked()
                        {
                            self.step_zoom(ZOOM_OUT_FACTOR);
                        }
                        ui.menu_button(
                            RichText::new(format!("{:.0}%", self.tab().display_zoom * 100.0))
                                .size(12.0)
                                .color(self.settings.theme.text_primary()),
                            |ui| {
                                if ui.button(tr("fit-window")).clicked() {
                                    self.tab_mut().zoom = None;
                                    ui.close();
                                }
                                if ui.button(tr("zoom-actual")).clicked() {
                                    self.tab_mut().zoom = Some(1.0);
                                    ui.close();
                                }
                                if ui.button("200%").clicked() {
                                    self.tab_mut().zoom = Some(2.0);
                                    ui.close();
                                }
                            },
                        );

                        ui.separator();
                        let tab = self.tab();
                        if let Some(coords) = tab.view.coords(&tab.editor.document) {
                            ui.label(self.settings.theme.muted_label(coords));
                            ui.separator();
                        }
                        ui.label(
                            self.settings
                                .theme
                                .muted_label(tab.view.dimensions(&tab.editor.document)),
                        );
                    });
                });
            });

        egui::SidePanel::left("tool_rail")
            .exact_width(78.0)
            .show_separator_line(false)
            .frame(workspace_frame.inner_margin(egui::Margin {
                left: CARD_GAP,
                right: 0,
                top: CARD_GAP,
                bottom: CARD_GAP,
            }))
            .show(ctx, |ui| self.tool_rail(ui));

        egui::SidePanel::right("inspector")
            .exact_width(246.0)
            .show_separator_line(false)
            .frame(workspace_frame.inner_margin(egui::Margin {
                left: 0,
                right: CARD_GAP,
                top: CARD_GAP,
                bottom: CARD_GAP,
            }))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = f32::from(CARD_GAP);
                self.inspector(ui);
            });

        egui::CentralPanel::default()
            .frame(workspace_frame.inner_margin(egui::Margin::symmetric(CARD_GAP, 4)))
            .show(ctx, |ui| {
                self.workspace_tabs(ui);
                ui.add_space(2.0);
                self.refresh_image(ctx);
                let zoom = self.canvas(ui);
                if zoom != self.tab().display_zoom {
                    self.tab_mut().display_zoom = zoom;
                    ctx.request_repaint();
                }
            });
        self.color_dropdown_popup(ctx, ribbon);
        self.dialogs(ctx);
        self.geometry_dialogs(ctx);
        if let Some(mut dialog) = self.shortcut_dialog.take()
            && !self.shortcuts.settings(ctx, &mut dialog)
        {
            self.shortcut_dialog = Some(dialog);
        }
        window_chrome::window_border(ctx, self.settings.theme, self.fullscreen);
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::TRANSPARENT.to_array()
    }

    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.ui_frame(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod perf_baseline;

    fn fixture_path(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    struct TestDriver {
        ctx: egui::Context,
        app: App,
        screen: Rect,
        time: f64,
    }

    impl TestDriver {
        fn new(doc: Document) -> Self {
            Self::with_screen(doc, 1280.0, 800.0)
        }

        fn with_screen(doc: Document, w: f32, h: f32) -> Self {
            let ctx = egui::Context::default();
            let app = App::new(&ctx, doc);
            Self {
                ctx,
                app,
                screen: Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h)),
                time: 0.0,
            }
        }

        fn run(
            &mut self,
            modifiers: egui::Modifiers,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            let input = egui::RawInput {
                screen_rect: Some(self.screen),
                modifiers,
                time: Some(self.time),
                events,
                ..Default::default()
            };
            self.time += 0.05;
            self.ctx.run(input, |ctx| self.app.ui_frame(ctx))
        }

        fn step(&mut self) -> egui::FullOutput {
            self.run(egui::Modifiers::NONE, Vec::new())
        }

        fn send_key(&mut self, key: egui::Key, modifiers: egui::Modifiers) -> egui::FullOutput {
            self.run(
                modifiers,
                vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
            )
        }

        fn press_key(&mut self, key: egui::Key) {
            for pressed in [true, false] {
                self.run(
                    egui::Modifiers::NONE,
                    vec![egui::Event::Key {
                        key,
                        physical_key: Some(key),
                        pressed,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
            }
        }

        fn click(&mut self, pos: egui::Pos2, button: PointerButton, modifiers: egui::Modifiers) {
            for pressed in [true, false] {
                self.run(
                    modifiers,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button,
                            pressed,
                            modifiers,
                        },
                    ],
                );
            }
        }

        fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
            self.run(
                egui::Modifiers::NONE,
                vec![
                    egui::Event::PointerMoved(from),
                    egui::Event::PointerButton {
                        pos: from,
                        button: PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            self.run(egui::Modifiers::NONE, vec![egui::Event::PointerMoved(to)]);
            self.run(
                egui::Modifiers::NONE,
                vec![
                    egui::Event::PointerMoved(to),
                    egui::Event::PointerButton {
                        pos: to,
                        button: PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }

    #[test]
    fn color_ribbon_background_reaches_the_workspace() -> io::Result<()> {
        for theme in [Theme::Light, Theme::Dark] {
            let ctx = egui::Context::default();
            let settings = GlobalSettings {
                theme,
                ..GlobalSettings::default()
            };
            let mut app = App::with_settings(&ctx, new_document(32, 24)?, settings);
            assert_eq!(ctx.style().visuals.dark_mode, theme == Theme::Dark);
            let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1120.0, 760.0));
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| app.ui_frame(ctx),
            );

            let backgrounds = |fill| {
                output.shapes.iter().filter_map(move |shape| {
                    let egui::Shape::Rect(rect) = &shape.shape else {
                        return None;
                    };
                    let visible = rect.rect.intersect(shape.clip_rect);
                    (rect.fill == fill && visible.x_range().contains(screen.center().x))
                        .then_some(visible)
                })
            };
            let ribbon_bottom = backgrounds(theme.toolbar_fill())
                .map(|rect| rect.bottom())
                .max_by(f32::total_cmp)
                .expect("color ribbon background");
            let workspace_top = backgrounds(theme.workspace())
                .map(|rect| rect.top())
                .min_by(f32::total_cmp)
                .expect("workspace background");
            assert_eq!(ribbon_bottom, workspace_top);
        }
        Ok(())
    }

    #[test]
    fn errors_flash_twice_in_the_status_bar() -> io::Result<()> {
        let mut driver = TestDriver::new(Document::new(64, 48)?);
        driver.step();
        driver.app.report(Ok(()), "Saved");
        assert!(driver.app.error_flash.is_none());

        // The text color and whether the error color fills its background.
        let error = driver.app.settings.theme.error();
        let look = |output: egui::FullOutput| {
            let text = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text() == "Broken" => {
                        Some(text.galley.job.sections[0].format.color)
                    }
                    _ => None,
                });
            let filled = output.shapes.iter().any(
                |clipped| matches!(&clipped.shape, egui::Shape::Rect(rect) if rect.fill == error),
            );
            (text.expect("message"), filled)
        };
        let inverted = (driver.app.settings.theme.status_fill(), true);
        let normal = (error, false);

        for _ in 0..2 {
            driver.app.report(Err(io::Error::other("Broken")), "");
            assert_eq!(look(driver.step()), inverted);
            // A file dialog can hold the next frame back for a long time.
            let start = driver.time + 60.0;
            for (after, expected) in [
                (0.0, inverted),
                (0.2, normal),
                (0.35, inverted),
                (0.5, normal),
                (5.0, normal),
            ] {
                driver.time = start + after;
                assert_eq!(look(driver.step()), expected, "after {after} s");
            }
            assert!(driver.app.error_flash.is_none());
            driver.time += 1.0;
        }
        Ok(())
    }

    #[test]
    fn new_document_has_one_opaque_white_background_layer() -> io::Result<()> {
        let document = new_document(32, 24)?;
        assert_eq!(document.layer_count(), 1);
        assert_eq!(
            document.layer_name(0),
            Some(i18n::logical_text("background"))
        );
        assert!(
            document
                .layer_pixels(0)
                .expect("background pixels")
                .iter()
                .all(|pixel| *pixel == Pixel([255, 255, 255, 255]))
        );
        Ok(())
    }

    #[test]
    fn selection_overlay_tints_only_covered_pixels() -> io::Result<()> {
        let ctx = egui::Context::default();
        let canvas = Rect::from_min_size(pos2(0.0, 0.0), vec2(24.0, 24.0));
        for mut selection in [
            Selection::new(Shape::Rectangle, (4, 4), (19, 19)),
            Selection::new(Shape::Ellipse, (4, 4), (19, 19)),
            Selection::new_with_aa(Shape::Ellipse, (4, 4), (19, 19), AaMode::Ssaa4x4),
            Selection::lasso((24, 24), &[(4, 4), (19, 4), (19, 19), (4, 19)])?,
        ] {
            for inverted in [false, true] {
                selection.inverted = inverted;
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(canvas),
                        ..Default::default()
                    },
                    |ctx| {
                        let painter = ctx.layer_painter(egui::LayerId::background());
                        paint_selection_overlay(&painter, canvas, 1.0, (24, 24), &selection);
                    },
                );
                // At 100% zoom the rectangle and ellipse tint can be sampled pixel by pixel.
                // Mask previews use 2×2 samples, so sample their grid origins.
                let step = if matches!(selection.region, Region::Mask(_)) {
                    2
                } else {
                    1
                };
                for y in (0..24).step_by(step) {
                    for x in (0..24).step_by(step) {
                        let point = pos2(x as f32 + 0.5, y as f32 + 0.5);
                        let alpha = output.shapes.iter().find_map(|shape| match &shape.shape {
                            egui::Shape::Rect(rect) if rect.rect.contains(point) => {
                                Some(rect.fill.a())
                            }
                            egui::Shape::Mesh(mesh) => {
                                mesh.vertices.as_chunks::<4>().0.iter().find_map(|quad| {
                                    Rect::from_min_max(quad[0].pos, quad[3].pos)
                                        .contains(point)
                                        .then(|| quad[0].color.a())
                                })
                            }
                            _ => None,
                        });
                        assert_eq!(
                            alpha.unwrap_or(0),
                            selection.coverage_u8((x, y)).div_ceil(3),
                            "wrong tint opacity at ({x}, {y}) for {selection:?}"
                        );
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    fn language_switch_updates_an_open_document_and_keeps_its_history_and_layer_names()
    -> io::Result<()> {
        fn collect_text(shape: &egui::Shape, output: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => output.push(text.galley.text().to_owned()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect_text(shape, output);
                    }
                }
                _ => {}
            }
        }

        let mut drv = TestDriver::new(new_document(32, 24)?);
        drv.app.tab_mut().editor.add_layer()?;
        drv.app
            .tab_mut()
            .editor
            .document
            .set_layer_name(1, "My saved layer")?;
        let shortcut = drv.app.shortcuts.hint(Command::Save);
        for locale in i18n::Locale::all() {
            drv.app.locale = locale;
            drv.ctx.set_fonts(i18n::fonts(locale));
            let output = drv.step();
            let mut rendered = Vec::new();
            for shape in output.shapes {
                collect_text(&shape.shape, &mut rendered);
            }
            let (file, untitled, history) = locale.with(|| {
                (
                    tr("file"),
                    tr("untitled"),
                    i18n::display(&format!("1. {}", i18n::logical_text("add-layer"))).into_owned(),
                )
            });
            assert!(
                rendered.iter().any(|text| text == file),
                "{}",
                locale.name()
            );
            assert!(
                rendered.iter().any(|text| text.contains(untitled)),
                "{}",
                locale.name()
            );
            assert!(rendered.contains(&history), "{}", locale.name());
            assert_eq!(
                drv.app.tab().editor.document.layer_name(1),
                Some("My saved layer")
            );
            assert_eq!(drv.app.shortcuts.hint(Command::Save), shortcut);
        }
        assert!(drv.app.tab_mut().editor.undo()?);
        assert_eq!(drv.app.tab().editor.document.layer_count(), 1);
        Ok(())
    }

    #[test]
    fn pdn_edit_undo_and_png_export_work_in_each_language() -> io::Result<()> {
        let path = fixture_path("pdn/Untitled.pdn");
        for locale in i18n::Locale::all() {
            locale.with(|| -> io::Result<()> {
                let (document, pdn) = open_document(&path)?;
                assert!(pdn);
                let mut editor = Editor::new(document);
                let color = Pixel([255, 0, 255, 255]);
                let original = editor.document.pixel(0, 0, 0);
                editor.select_all();
                assert!(editor.fill_selection(color, "fill-selection")?);
                assert_eq!(editor.document.pixel(0, 0, 0), Some(color));
                assert!(editor.undo()?);
                assert_eq!(editor.document.pixel(0, 0, 0), original);

                let mut png = Vec::new();
                editor.document.write_png(&mut png)?;
                let exported = Document::read_png(std::io::Cursor::new(png))?;
                assert_eq!(exported.composite()?, editor.document.composite()?);
                Ok(())
            })?;
        }
        Ok(())
    }

    #[test]
    fn selection_tools_commit_soft_masks_that_fill_and_move_with_partial_alpha() -> io::Result<()> {
        let ctx = egui::Context::default();
        let size = (24, 24);
        let mut app = App::new(&ctx, Document::new(size.0, size.1)?);
        let color = Pixel([90, 60, 30, 255]);
        app.selection_feather = 100;
        let candidates = [
            Selection::new(Shape::Rectangle, (7, 7), (14, 14)),
            Selection::new(Shape::Ellipse, (7, 7), (14, 14)),
            Selection::lasso(size, &[(7, 7), (14, 7), (14, 14), (7, 14)])?,
            Selection::wand(
                size,
                (7, 7),
                0,
                AlphaMatchMode::Premultiplied,
                true,
                |(x, y)| {
                    if (7..=14).contains(&x) && (7..=14).contains(&y) {
                        color
                    } else {
                        Pixel::default()
                    }
                },
            )?,
        ];
        for candidate in candidates {
            let point = (10, 15);
            assert_eq!(candidate.coverage_u8(point), 0);
            let preview = app.selection_candidate(candidate.clone(), Combine::Replace)?;
            app.commit_selection_with(Ok(candidate), Combine::Replace, "Seleção");
            let editor = &mut app.tab_mut().editor;
            assert_eq!(editor.selection.as_ref(), Some(&preview));
            let coverage = preview.coverage_u8(point);
            assert!((1..255).contains(&coverage));
            assert!(editor.fill_selection(color, "Preencher seleção")?);
            assert_eq!(
                editor.document.pixel(0, point.0, point.1).unwrap().0[3],
                coverage
            );
            assert!(editor.undo()?);
            let mut translate = geometry::Affine::centered(size);
            translate.shift = (1.0, 2.0);
            editor.transform_mask(translate)?;
            assert_eq!(
                editor
                    .selection
                    .as_ref()
                    .unwrap()
                    .coverage_u8((point.0 + 1, point.1 + 2)),
                coverage,
            );
            assert!(editor.undo()?);
            assert_eq!(editor.selection.as_ref(), Some(&preview));
        }
        Ok(())
    }

    #[test]
    fn click_outside_or_escape_deselects_and_bucket_click_fills() -> io::Result<()> {
        let mut drv = TestDriver::new(Document::new(100, 100)?);
        let center = pos2(556.0, 440.0);
        let click = |drv: &mut TestDriver| {
            drv.run(
                egui::Modifiers::NONE,
                vec![egui::Event::PointerMoved(center)],
            );
            drv.click(center, PointerButton::Primary, egui::Modifiers::NONE);
        };
        let selection = Selection::new(Shape::Rectangle, (0, 0), (4, 4));
        for tool in [
            Tool::Pan,
            Tool::Brush,
            Tool::Wand,
            Tool::Select(Shape::Rectangle),
            Tool::Bucket,
        ] {
            drv.app.activate_tool(tool);
            drv.app
                .tab_mut()
                .editor
                .set_selection(Some(selection.clone()), "Seleção");
            click(&mut drv);
            assert!(drv.app.tab().editor.selection.is_none(), "{}", tool.label());
            assert!(
                !drv.app.tab().editor.has_unsaved_edits(),
                "{}",
                tool.label()
            );
        }

        drv.app
            .tab_mut()
            .editor
            .set_selection(Some(selection), "Seleção");
        drv.send_key(egui::Key::Escape, egui::Modifiers::NONE);
        assert!(drv.app.tab().editor.selection.is_none());
        drv.app.activate_tool(Tool::Bucket);
        click(&mut drv);
        assert_eq!(
            drv.app.tab().editor.document.pixel(0, 0, 0),
            Some(drv.app.palette.primary())
        );
        Ok(())
    }

    #[test]
    fn pasted_document_keeps_source_history_and_selection_in_its_tab() -> io::Result<()> {
        let context = egui::Context::default();
        let mut app = App::new(&context, Document::new(8, 6)?);
        app.tab_mut().editor.select_all();
        let selection = app.tab().editor.selection.clone();
        app.tab_mut().editor.add_layer()?;
        app.open_in_new_tab(Document::new(2, 3)?, "Colagem".into(), None);
        app.tab_mut().editor.mark_new_content();
        assert_eq!(app.tab().editor.document.dimensions(), (2, 3));
        assert!(app.tab().editor.has_unsaved_edits());
        app.switch_tab(0);
        assert_eq!(app.tab().editor.document.dimensions(), (8, 6));
        assert_eq!(app.tab().editor.selection, selection);
        assert!(app.tab_mut().editor.undo()?);
        assert_eq!(app.tab().editor.document.layer_count(), 1);
        app.switch_tab(1);
        assert_eq!(app.tab().editor.document.dimensions(), (2, 3));
        app.close_tab_at(1);
        assert_eq!(app.tab().editor.document.dimensions(), (8, 6));
        assert_eq!(app.tabs.len(), 1);
        Ok(())
    }

    #[test]
    fn new_and_open_tabs_preserve_existing_document_and_transform() -> io::Result<()> {
        let context = egui::Context::default();
        let mut app = App::new(&context, Document::new(8, 8)?);
        app.tab_mut().editor.add_layer()?;
        app.start_transform(TransformTarget::Layer);
        assert!(
            app.tab()
                .session
                .as_ref()
                .is_some_and(ActiveSession::is_transform)
        );
        app.open_in_new_tab(Document::new(4, 4)?, "Novo.pdn".into(), None);
        assert_eq!(app.tabs.len(), 2);
        app.switch_tab(0);
        assert!(
            app.tab()
                .session
                .as_ref()
                .is_some_and(ActiveSession::is_transform)
        );
        assert_eq!(app.tab().editor.document.layer_count(), 2);
        app.reorder_tab(0, 1);
        assert_eq!(app.active_tab, 1);
        assert_eq!(app.tabs[0].name, "Novo.pdn");
        Ok(())
    }

    #[test]
    fn floating_paste_commits_after_shift_and_constrains_square() -> io::Result<()> {
        let context = egui::Context::default();
        let mut app = App::new(&context, Document::new(8, 8)?);
        let red = Pixel([220, 40, 40, 255]);
        app.clipboard.put(clipboard::Image {
            size: (2, 2),
            origin: (1, 1),
            pixels: vec![red; 4],
        });
        app.paste_from_clipboard(false);
        assert!(
            app.tab()
                .session
                .as_ref()
                .is_some_and(ActiveSession::is_transform)
        );
        app.tab_mut()
            .session
            .as_mut()
            .and_then(ActiveSession::as_transform_mut)
            .unwrap()
            .affine
            .shift = (3.0, 2.0);
        app.commit_active_session();
        assert_eq!(app.tab().editor.document.pixel(0, 4, 3), Some(red));
        assert_eq!(
            app.tab().editor.document.pixel(0, 1, 1),
            Some(Pixel::default())
        );
        assert_eq!(constrain_square_point((2, 2), (6, 4), (8, 8)), (6, 6));
        Ok(())
    }

    #[test]
    fn selection_keyboard_events_fill_and_delete() -> io::Result<()> {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, Document::new(4, 4)?);
        app.tab_mut().editor.set_selection(
            Some(Selection::new(Shape::Rectangle, (0, 0), (1, 1))),
            "Seleção retangular",
        );
        for (key, expected) in [
            (egui::Key::Backspace, app.palette.primary()),
            (egui::Key::Delete, Pixel::default()),
        ] {
            let input = egui::RawInput {
                events: vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                for command in app.shortcuts.commands(ctx, false) {
                    app.execute_command(ctx, command);
                }
            });
            assert_eq!(app.tab().editor.document.pixel(0, 0, 0), Some(expected));
            assert_eq!(
                app.tab().editor.document.pixel(0, 2, 2),
                Some(Pixel::default())
            );
        }
        Ok(())
    }

    #[test]
    fn pdn_save_replaces_only_when_allowed_and_keeps_last_valid_file() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("teste.pdn");
        let mut document = Document::new(2, 2)?;
        write_pdn_atomically(&document, &path, false)?;
        let original = std::fs::read(&path)?;
        document.set_pixel(0, 1, 1, Pixel([0, 100, 0, 255]))?;
        assert!(write_pdn_atomically(&document, &path, false).is_err());
        assert_eq!(std::fs::read(&path)?, original);
        write_pdn_atomically(&document, &path, true)?;
        let saved = std::fs::read(&path)?;
        assert_eq!(
            Document::read_pdn(saved.as_slice())?.pixel(0, 1, 1),
            document.pixel(0, 1, 1)
        );
        document.add_layer("Nome\0inválido")?;
        assert!(write_pdn_atomically(&document, &path, true).is_err());
        assert_eq!(std::fs::read(&path)?, saved);
        assert_eq!(std::fs::read_dir(directory.path())?.count(), 1);
        Ok(())
    }

    #[test]
    fn save_options_dialog_allows_overwrite_and_encodes_png_depths() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("saida.png");
        std::fs::write(&path, b"old")?;
        let ctx = egui::Context::default();
        let mut doc = Document::new(2, 2)?;
        doc.set_pixel(0, 0, 0, Pixel([200, 100, 50, 128]))?;
        let mut dialog = SaveOptionsDialog::new(path.clone(), SaveFormat::png(), false, &doc, &ctx);
        dialog.format = dialogs::SaveFormat::Png {
            depth: openpaintr::PngDepth::Rgb24White,
            compression: openpaintr::PngCompression::High,
        };
        let mut bytes = Vec::new();
        doc.write_png_with_options(
            &mut bytes,
            openpaintr::PngDepth::Rgb24White,
            openpaintr::PngCompression::High,
        )?;
        assert!(!bytes.is_empty());
        dialog.write_to_disk()?;
        assert!(std::fs::read(&path)?.starts_with(b"\x89PNG"));
        Ok(())
    }

    #[test]
    fn layout_renders_both_themes_and_collapsed_cards() -> io::Result<()> {
        let mut drv = TestDriver::new(Document::new(32, 24)?);
        for theme in [Theme::Light, Theme::Dark] {
            drv.app.settings.theme = theme;
            theme.apply(&drv.ctx);
            drv.app.colors_open = false;
            drv.app.history_open = false;
            drv.step();
            drv.app.colors_open = true;
            drv.app.history_open = true;
            drv.step();
        }
        Ok(())
    }

    #[test]
    fn transform_keyboard_arrows_nudge_and_enter_commits() -> io::Result<()> {
        let mut drv = TestDriver::new(Document::new(20, 20)?);
        let red = Pixel([220, 40, 40, 255]);
        drv.app.tab_mut().editor.document.set_pixel(0, 2, 2, red)?;
        drv.app.tab_mut().editor.set_selection(
            Some(Selection::new(Shape::Rectangle, (2, 2), (2, 2))),
            "Seleção retangular",
        );
        drv.app.execute_command(&drv.ctx, Command::MovePixels);
        assert!(
            drv.app
                .tab()
                .session
                .as_ref()
                .is_some_and(ActiveSession::is_transform)
        );

        drv.send_key(egui::Key::ArrowRight, egui::Modifiers::NONE);
        drv.send_key(egui::Key::ArrowDown, egui::Modifiers::CTRL);
        assert_eq!(
            drv.app
                .tab()
                .session
                .as_ref()
                .and_then(ActiveSession::as_transform)
                .unwrap()
                .affine
                .shift,
            (1.0, 10.0)
        );
        drv.send_key(egui::Key::Enter, egui::Modifiers::NONE);
        assert!(drv.app.tab().session.is_none());
        assert_eq!(drv.app.tab().editor.document.pixel(0, 3, 12), Some(red));
        assert_eq!(
            drv.app.tab().editor.document.pixel(0, 2, 2),
            Some(Pixel::default())
        );

        drv.app.tab_mut().editor.set_selection(
            Some(Selection::new(Shape::Rectangle, (3, 12), (3, 12))),
            "Seleção retangular",
        );
        drv.app.execute_command(&drv.ctx, Command::MovePixels);
        drv.send_key(egui::Key::ArrowRight, egui::Modifiers::NONE);
        drv.send_key(egui::Key::Escape, egui::Modifiers::NONE);
        assert!(drv.app.tab().session.is_none());
        assert_eq!(drv.app.tab().editor.document.pixel(0, 3, 12), Some(red));

        // Primary mouse click on the canvas commits the transform
        drv.app.execute_command(&drv.ctx, Command::MovePixels);
        drv.app
            .tab_mut()
            .session
            .as_mut()
            .and_then(ActiveSession::as_transform_mut)
            .unwrap()
            .nudge(2.0, 0.0);
        let center = pos2(640.0, 400.0);
        drv.run(
            egui::Modifiers::NONE,
            vec![egui::Event::PointerMoved(center)],
        );
        drv.click(center, PointerButton::Primary, egui::Modifiers::NONE);
        assert!(drv.app.tab().session.is_none());
        assert_eq!(drv.app.tab().editor.document.pixel(0, 5, 12), Some(red));
        assert_eq!(
            drv.app.tab().editor.document.pixel(0, 3, 12),
            Some(Pixel::default())
        );

        // Escape cancels floating paste and also reverts the paste selection
        drv.app.tab_mut().editor.set_selection(None, "Limpar");
        drv.app.clipboard.put(clipboard::Image {
            size: (2, 2),
            origin: (0, 0),
            pixels: vec![Pixel([0, 200, 0, 255]); 4],
        });
        drv.app.paste_from_clipboard(false);
        assert!(
            drv.app
                .tab()
                .session
                .as_ref()
                .is_some_and(ActiveSession::is_transform)
        );
        assert!(drv.app.tab().editor.selection.is_some());
        drv.send_key(egui::Key::Escape, egui::Modifiers::NONE);
        assert!(drv.app.tab().session.is_none());
        assert!(drv.app.tab().editor.selection.is_none());
        assert_eq!(
            drv.app.tab().editor.document.pixel(0, 0, 0),
            Some(Pixel::default())
        );
        Ok(())
    }

    #[test]
    fn tolerance_is_shared_across_wand_bucket_and_recolor_with_alpha_modes() -> io::Result<()> {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, Document::new(8, 8)?);
        let trans_red = Pixel([255, 0, 0, 0]);
        let trans_blue = Pixel([0, 0, 255, 0]);
        let soft_blue = Pixel([40, 80, 200, 255]);
        let near_soft_blue = Pixel([58, 94, 212, 255]);
        app.tab_mut()
            .editor
            .document
            .set_pixel(0, 0, 0, trans_red)?;
        app.tab_mut()
            .editor
            .document
            .set_pixel(0, 1, 0, trans_blue)?;
        app.tab_mut()
            .editor
            .document
            .set_pixel(0, 3, 3, soft_blue)?;
        app.tab_mut()
            .editor
            .document
            .set_pixel(0, 5, 5, near_soft_blue)?;

        // 0% tolerance distinguishes Straight vs Premultiplied on zero-alpha pixels
        app.color_tolerance = 0;
        app.alpha_match = AlphaMatchMode::Premultiplied;
        let wand_premul = app.wand((0, 0), true)?;
        assert!(wand_premul.contains((1, 0)));

        app.alpha_match = AlphaMatchMode::Straight;
        let wand_straight = app.wand((0, 0), true)?;
        assert!(!wand_straight.contains((1, 0)));

        // Shared tolerance governs global bucket fill and composite sampling
        app.color_tolerance = 20;
        app.palette.set_primary(Pixel([20, 200, 60, 255]));
        app.bucket_fill((3, 3), app.palette.primary(), false);
        assert_eq!(
            app.tab().editor.document.pixel(0, 5, 5),
            Some(app.palette.primary())
        );
        assert!(app.tab_mut().editor.undo()?);

        // Shared tolerance governs Recolor stroke and preserves shading
        let tol = app.color_tolerance;
        let alpha_mode = app.alpha_match;
        let aa_mode = app.settings.antialiasing;
        app.tab_mut().editor.begin_stroke_with(
            StrokeSpec::Recolor {
                target: soft_blue,
                replacement: Pixel([200, 80, 40, 255]),
                tolerance_pct: tol,
                alpha_mode,
                hardness: 100,
                spacing: 15,
            },
            4,
            aa_mode,
        );
        app.tab_mut().editor.draw_to(Some((5, 5)));
        app.tab_mut().editor.finish_stroke();
        let recolored = app.tab().editor.document.pixel(0, 5, 5).unwrap();
        assert_ne!(recolored, near_soft_blue);
        assert_eq!(recolored.0[0], 218);
        assert!(app.tab_mut().editor.undo()?);
        assert_eq!(
            app.tab().editor.document.pixel(0, 5, 5),
            Some(near_soft_blue)
        );
        Ok(())
    }

    #[test]
    fn eyedropper_samples_layer_or_composite_and_clone_stamp_copies_with_undo() -> io::Result<()> {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, Document::new(16, 16)?);
        let green = Pixel([30, 180, 70, 255]);
        let yellow = Pixel([240, 220, 40, 255]);
        app.tab_mut().editor.document.fill_layer(0, green)?;
        app.tab_mut().editor.add_layer()?;
        app.tab_mut().editor.document.set_pixel(1, 2, 2, yellow)?;

        // Composite vs layer sampling
        let comp = app.tab().editor.document.composite()?;
        assert_eq!(comp[10 * 16 + 10], green);
        assert_eq!(
            app.tab().editor.document.pixel(1, 10, 10),
            Some(Pixel::default())
        );

        // Clone stamp from layer 1 pixel (2, 2) to (8, 8)
        let src = app.tab().editor.document.layer_pixels(1).unwrap().to_vec();
        let aa_mode = app.settings.antialiasing;
        app.tab_mut().editor.begin_stroke_with(
            StrokeSpec::Clone {
                source_pixels: src,
                source_size: (16, 16),
                offset: (-6, -6),
                blend: StrokeBlend::Normal,
                hardness: 100,
                spacing: 15,
            },
            3,
            aa_mode,
        );
        app.tab_mut().editor.draw_to(Some((8, 8)));
        app.tab_mut().editor.finish_stroke();
        assert_eq!(app.tab().editor.document.pixel(1, 8, 8), Some(yellow));
        assert!(app.tab_mut().editor.undo()?);
        assert_eq!(
            app.tab().editor.document.pixel(1, 8, 8),
            Some(Pixel::default())
        );
        Ok(())
    }

    #[test]
    fn tool_params_share_blend_pattern_and_outline_thickness() -> io::Result<()> {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, Document::new(8, 8)?);
        assert_eq!(app.gradient_params(), GradientParams::default());
        assert_eq!(app.line_curve_params(), LineCurveParams::default());
        assert_eq!(app.shape_params(), ShapeParams::default());
        assert_eq!(app.text_params(), TextParams::default());

        app.set_shape_params(ShapeParams {
            thickness: 7,
            pattern: FillPattern::ALL[1],
            blend: StrokeBlend::Overwrite,
            corner_radius: 3.0,
            ..app.shape_params()
        });
        assert_eq!(app.line_curve_params().thickness, 7);
        assert_eq!(app.line_curve_params().blend, StrokeBlend::Overwrite);
        assert_eq!(app.gradient_params().blend, StrokeBlend::Overwrite);
        assert_eq!(app.text_params().blend, StrokeBlend::Overwrite);
        assert_eq!(app.fill_pattern, FillPattern::ALL[1]);
        assert_eq!(app.shape_params().corner_radius, 3.0);

        app.set_line_curve_params(LineCurveParams {
            thickness: 4,
            blend: StrokeBlend::Normal,
            ..app.line_curve_params()
        });
        assert_eq!(app.shape_params().thickness, 4);
        assert_eq!(app.shape_params().blend, StrokeBlend::Normal);
        assert_eq!(app.shape_params().corner_radius, 3.0);
        Ok(())
    }

    #[test]
    fn interactive_gradient_line_shape_freeform_and_text_commit_cancel_undo() -> io::Result<()> {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, Document::new(64, 48)?);
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0));
        let send_events = |app: &mut App, events| {
            let input = egui::RawInput {
                screen_rect: Some(rect),
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| app.ui_frame(ctx));
        };
        let key_evt = |key: egui::Key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };

        // Gradient: Esc cancels, Enter commits
        app.activate_tool(Tool::Gradient);
        let grad_params = GradientParams {
            shape: GradientShape::Linear,
            repeat: GradientRepeat::None,
            channel_mode: GradientChannelMode::Colors,
            dither: false,
            blend: StrokeBlend::Overwrite,
        };
        app.tab_mut().session = Some(ActiveSession::Gradient(GradientSession::new(
            vec2(4.0, 4.0),
            vec2(50.0, 4.0),
            Pixel([200, 40, 40, 255]),
            Pixel([40, 40, 200, 255]),
            false,
            grad_params,
        )));
        send_events(&mut app, vec![key_evt(egui::Key::Escape)]);
        assert!(app.tab().session.is_none());
        assert_eq!(
            app.tab().editor.document.pixel(0, 10, 4),
            Some(Pixel::default())
        );

        app.tab_mut().session = Some(ActiveSession::Gradient(GradientSession::new(
            vec2(4.0, 4.0),
            vec2(50.0, 4.0),
            Pixel([200, 40, 40, 255]),
            Pixel([40, 40, 200, 255]),
            false,
            grad_params,
        )));
        send_events(&mut app, vec![key_evt(egui::Key::Enter)]);
        assert!(app.tab().session.is_none());
        assert_ne!(
            app.tab().editor.document.pixel(0, 25, 20),
            Some(Pixel::default())
        );
        assert!(app.tab_mut().editor.undo()?);

        // Line / Curve commits SSAA pixels + undo
        app.activate_tool(Tool::LineCurve);
        app.tab_mut().session = Some(ActiveSession::LineCurve(LineCurveSession::new(
            vec2(6.0, 10.0),
            vec2(54.0, 38.0),
            Pixel([20, 20, 20, 255]),
            LineCurveParams {
                kind: CurveKind::CubicBezier,
                thickness: 3,
                dash: DashPreset::Solid,
                start_cap: LineCapStyle::Flat,
                end_cap: LineCapStyle::ArrowFilled,
                blend: StrokeBlend::Normal,
            },
        )));
        send_events(&mut app, vec![key_evt(egui::Key::Enter)]);
        assert!(app.tab().session.is_none());
        assert!(app.tab_mut().editor.undo()?);

        // Shape Freeform commits + undo
        let mut freeform = ShapeSession::new(
            vec2(8.0, 8.0),
            vec2(40.0, 40.0),
            Pixel([10, 10, 10, 255]),
            Pixel([180, 90, 220, 255]),
            ShapeParams {
                kind: ShapeKind::Freeform,
                fill_mode: ShapeFillMode::Both,
                thickness: 2,
                corner_radius: 8.0,
                pattern: FillPattern::Solid,
                blend: StrokeBlend::Normal,
            },
        );
        freeform.set_creation_bounds(vec2(8.0, 8.0), vec2(40.0, 12.0), false);
        freeform.set_creation_bounds(vec2(8.0, 8.0), vec2(26.0, 40.0), false);
        app.tab_mut().session = Some(ActiveSession::Shape(freeform));
        send_events(&mut app, vec![key_evt(egui::Key::Enter)]);
        assert!(app.tab().session.is_none());
        assert_ne!(
            app.tab().editor.document.pixel(0, 20, 20),
            Some(Pixel::default())
        );
        assert!(app.tab_mut().editor.undo()?);

        // Text: Event::Text("Oi") + Enter commits SSAA glyphs without triggering tool shortcuts
        app.activate_tool(Tool::Text);
        app.tab_mut().session = Some(ActiveSession::Text(TextSession::new(
            vec2(4.0, 4.0),
            Pixel([25, 25, 25, 255]),
            TextParams {
                family: text_tool::DEFAULT_FAMILY.into(),
                size_px: 28.0,
                bold: false,
                italic: false,
                underline: false,
                strikethrough: false,
                align: TextAlign::Left,
                blend: StrokeBlend::Normal,
            },
        )));
        send_events(&mut app, vec![egui::Event::Text("bt".into())]);
        assert_eq!(app.tool, Tool::Text);
        assert_eq!(
            app.tab()
                .session
                .as_ref()
                .and_then(ActiveSession::as_text)
                .unwrap()
                .text,
            "bt"
        );
        send_events(&mut app, vec![key_evt(egui::Key::Enter)]);
        assert!(app.tab().session.is_none());
        let any_ink = (0..48)
            .flat_map(|y| (0..64).map(move |x| (x, y)))
            .any(|(x, y)| app.tab().editor.document.pixel(0, x, y).unwrap().0[3] > 0);
        assert!(any_ink);
        assert!(app.tab_mut().editor.undo()?);
        Ok(())
    }

    #[test]
    fn wheel_gesture_adjusts_pixel_widths_and_the_active_vector_preview() -> io::Result<()> {
        let mut drv = TestDriver::new(Document::new(64, 48)?);
        drv.app.tab_mut().zoom = Some(2.0);
        let no_modifiers = egui::Modifiers::NONE;
        drv.step();
        drv.step();
        let pointer = drv
            .ctx
            .read_response(egui::Id::new(CANVAS_ID))
            .unwrap()
            .rect
            .center();
        let wheel = |drv: &mut TestDriver, lines, modifiers| {
            drv.run(
                modifiers,
                vec![egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: vec2(0.0, lines),
                    modifiers,
                }],
            )
        };
        let assert_preview = |output: egui::FullOutput, radius, fill| {
            assert_eq!(output.platform_output.cursor_icon, egui::CursorIcon::None);
            assert!(output.shapes.iter().any(|shape| matches!(
                &shape.shape,
                egui::Shape::Circle(circle) if circle.center == pointer
                    && circle.radius == radius
                    && circle.fill == fill
                    && circle.stroke.color == Color32::WHITE
            )));
        };
        let crosshair = |output: egui::FullOutput| {
            assert_eq!(
                output.platform_output.cursor_icon,
                egui::CursorIcon::Crosshair
            );
        };
        let outline = Color32::TRANSPARENT;
        let resize = egui::Modifiers::CTRL | egui::Modifiers::SHIFT;
        drv.app.activate_tool(Tool::Brush);
        crosshair(drv.run(no_modifiers, vec![egui::Event::PointerMoved(pointer)]));
        assert_preview(drv.run(resize, vec![]), 12.0, outline);
        for tool in [Tool::Brush, Tool::Eraser, Tool::CloneStamp, Tool::Recolor] {
            drv.app.activate_tool(tool);
            drv.app.diameter = 12;
            assert_preview(wheel(&mut drv, 1.0, resize), 13.0, outline);
            assert_eq!(drv.app.diameter, 13, "{}", tool.label());
            assert_preview(wheel(&mut drv, -2.0, resize), 11.0, outline);
            assert_eq!(drv.app.diameter, 11, "{}", tool.label());
        }
        crosshair(drv.run(egui::Modifiers::CTRL, vec![]));

        drv.app.activate_tool(Tool::Pencil);
        assert_preview(wheel(&mut drv, -1.0, resize), 1.0, Color32::BLACK);
        assert_eq!(drv.app.pencil_diameter, 1);
        assert_preview(wheel(&mut drv, 4.0, resize), 5.0, outline);
        assert_eq!(drv.app.pencil_diameter, 5);
        assert_eq!(drv.app.diameter, 11);

        drv.app.activate_tool(Tool::LineCurve);
        drv.app.shape_thickness = 9;
        drv.app.tab_mut().session = Some(ActiveSession::LineCurve(LineCurveSession::new(
            vec2(10.0, 18.0),
            vec2(40.0, 18.0),
            drv.app.palette.primary(),
            LineCurveParams {
                kind: CurveKind::CubicBezier,
                thickness: 2,
                dash: DashPreset::Solid,
                start_cap: LineCapStyle::Flat,
                end_cap: LineCapStyle::Flat,
                blend: StrokeBlend::Normal,
            },
        )));
        let zoom = drv.app.tab().zoom;
        assert_preview(wheel(&mut drv, 8.0, resize), 10.0, outline);
        assert_eq!(drv.app.shape_thickness, 10);
        assert_eq!(
            *drv.app
                .tab_mut()
                .session
                .as_mut()
                .unwrap()
                .stroke_size_target()
                .unwrap(),
            10
        );
        assert_eq!(
            drv.app.tab().editor.document.pixel(0, 20, 21),
            Some(Pixel::default())
        );
        assert_eq!(drv.app.tab().zoom, zoom);
        drv.app.commit_active_session();
        assert_ne!(
            drv.app.tab().editor.document.pixel(0, 20, 21),
            Some(Pixel::default())
        );

        drv.app.activate_tool(Tool::DrawShape);
        drv.app.tab_mut().session = Some(ActiveSession::Shape(ShapeSession::new(
            vec2(10.0, 10.0),
            vec2(40.0, 40.0),
            drv.app.palette.primary(),
            drv.app.palette.secondary(),
            ShapeParams {
                kind: ShapeKind::Freeform,
                fill_mode: ShapeFillMode::Both,
                thickness: 10,
                corner_radius: 0.0,
                pattern: FillPattern::Solid,
                blend: StrokeBlend::Normal,
            },
        )));
        assert_preview(
            wheel(&mut drv, 100.0, resize),
            MAX_OUTLINE_THICKNESS as f32,
            outline,
        );
        assert_eq!(drv.app.shape_thickness, MAX_OUTLINE_THICKNESS);
        assert_eq!(
            *drv.app
                .tab_mut()
                .session
                .as_mut()
                .unwrap()
                .stroke_size_target()
                .unwrap(),
            MAX_OUTLINE_THICKNESS
        );

        drv.app.activate_tool(Tool::Bucket);
        drv.run(resize, vec![]);
        crosshair(wheel(&mut drv, 1.0, resize));
        assert_eq!(drv.app.diameter, 11);
        assert_eq!(drv.app.tab().zoom, zoom);
        wheel(&mut drv, 1.0, egui::Modifiers::CTRL);
        assert!(drv.app.tab().zoom > zoom);

        drv.app.activate_tool(Tool::Brush);
        drv.app.tab_mut().zoom = zoom;
        drv.app.shortcuts.mouse =
            mouse_shortcuts::MouseBindings::compile(&std::collections::BTreeMap::from([(
                MouseAction::WheelStrokeSize.id().into(),
                vec!["Alt+WheelX".into()],
            )]))
            .map_err(io::Error::other)?;
        crosshair(drv.run(resize, vec![]));
        assert_preview(drv.run(egui::Modifiers::ALT, vec![]), 11.0, outline);
        crosshair(drv.step());
        let outside_canvas = drv.run(
            egui::Modifiers::ALT,
            vec![egui::Event::PointerMoved(pos2(20.0, pointer.y))],
        );
        assert_ne!(
            outside_canvas.platform_output.cursor_icon,
            egui::CursorIcon::None
        );
        Ok(())
    }

    #[test]
    fn zoom_marquee_and_selection_relocate_mouse_bindings_exist_in_both_profiles() {
        let shortcuts = Shortcuts::default();
        assert_eq!(
            MouseAction::ZoomMarquee.defaults(shortcuts::Base::PaintNet),
            &["Left"]
        );
        assert_eq!(
            MouseAction::ZoomScrubby.defaults(shortcuts::Base::Adobe),
            &["Left"]
        );
        assert_eq!(
            MouseAction::SelectionRelocate.defaults(shortcuts::Base::PaintNet),
            &["Right"]
        );
        assert_eq!(
            MouseAction::SelectionRelocate.defaults(shortcuts::Base::Adobe),
            &["Space+Left"]
        );
        let _ = shortcuts.hint(Command::Text);
    }

    #[test]
    fn wslg_prefers_x11_and_fullscreen_keeps_the_window_borderless() {
        assert_eq!(linux_backend_choice(None, true, true), Some(true));
        assert_eq!(linux_backend_choice(None, true, false), None);
        assert_eq!(
            linux_backend_choice(Some("wayland"), true, true),
            Some(false)
        );
        assert_eq!(linux_backend_choice(Some("x11"), false, false), Some(true));
        assert_eq!(linux_backend_choice(None, false, true), None);

        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, Document::new(16, 16).unwrap());
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let run_frame = |app: &mut App, events, fullscreen: Option<bool>| {
            let mut input = egui::RawInput {
                screen_rect: Some(rect),
                events,
                ..Default::default()
            };
            input
                .viewports
                .entry(egui::ViewportId::ROOT)
                .or_default()
                .fullscreen = fullscreen;
            ctx.run(input, |ctx| app.ui_frame(ctx))
        };
        // Warm up initial layout pass before sending shortcut events.
        let _ = run_frame(&mut app, Vec::new(), Some(false));
        let output = run_frame(
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::F11,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            Some(false),
        );
        assert!(app.fullscreen);
        assert!(
            output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .contains(&egui::ViewportCommand::Fullscreen(true))
        );
        assert!(
            !output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .iter()
                .any(|command| matches!(command, egui::ViewportCommand::Decorations(_)))
        );

        // Intermediate frame before OS ack must keep fullscreen & decorations hidden.
        let _ = run_frame(&mut app, Vec::new(), Some(false));
        assert!(app.fullscreen);

        // The native fullscreen state can change without the app requesting it.
        let _ = run_frame(&mut app, Vec::new(), Some(true));
        let exit_out = run_frame(&mut app, Vec::new(), Some(false));
        assert!(!app.fullscreen);
        assert!(
            !exit_out.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .iter()
                .any(|command| matches!(command, egui::ViewportCommand::Decorations(_)))
        );
    }

    #[test]
    fn color_ribbon_switches_swatches_and_paints_with_their_opacity() -> io::Result<()> {
        let mut drv = TestDriver::new(Document::new(100, 100)?);
        let pointer_button = |pos, button, pressed| egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let left = PointerButton::Primary;
        let right = PointerButton::Secondary;
        let custom = Pixel([200, 135, 80, 173]);
        drv.app.palette.set_primary(custom);
        drv.app.activate_tool(Tool::Brush);
        drv.app.colors_open = true;
        drv.step();
        drv.step();
        let ribbon =
            egui::containers::panel::PanelState::load(&drv.ctx, egui::Id::new("color_ribbon"))
                .unwrap()
                .rect;
        let at = |x| pos2(ribbon.left() + x, ribbon.center().y);
        let opacity_start = at(80.0);
        let opacity_midpoint = at(133.0);
        let opacity_drag_end = at(191.0);

        drv.click(at(40.0), left, egui::Modifiers::NONE);
        assert_eq!(drv.app.palette.primary(), Pixel([0, 0, 0, 255]));
        drv.click(opacity_midpoint, left, egui::Modifiers::NONE);
        let half_black = Pixel([0, 0, 0, 128]);
        assert_eq!(drv.app.palette.primary(), half_black);
        assert!(drv.app.colors_open);
        drv.click(at(64.0), left, egui::Modifiers::NONE);
        assert_eq!(drv.app.palette.primary(), Pixel([255, 255, 255, 255]));
        drv.click(opacity_start, left, egui::Modifiers::NONE);
        assert_eq!(drv.app.palette.primary(), Pixel([255, 255, 255, 0]));
        drv.run(
            egui::Modifiers::NONE,
            vec![pointer_button(opacity_start, left, true)],
        );
        drv.run(
            egui::Modifiers::NONE,
            vec![egui::Event::PointerMoved(opacity_drag_end)],
        );
        assert_eq!(drv.app.palette.primary(), Pixel([255, 255, 255, 255]));
        drv.run(
            egui::Modifiers::NONE,
            vec![pointer_button(opacity_drag_end, left, false)],
        );
        drv.click(opacity_start, left, egui::Modifiers::NONE);
        drv.click(at(40.0), right, egui::Modifiers::NONE);
        assert_eq!(drv.app.palette.secondary(), half_black);
        assert_eq!(drv.app.palette.primary(), Pixel([255, 255, 255, 0]));
        drv.click(at(16.0), left, egui::Modifiers::NONE);
        assert_eq!(drv.app.palette.primary(), custom);
        drv.click(at(64.0), left, egui::Modifiers::NONE);
        assert_eq!(drv.app.palette.primary(), Pixel([255, 255, 255, 0]));

        let hue_start = ribbon.left() + 201.0;
        let hue_end = ribbon.right() - 47.0;
        let green = egui::lerp(hue_start..=hue_end, 1.0 / 3.0);
        drv.click(pos2(green, ribbon.center().y), left, egui::Modifiers::NONE);
        let Pixel([r, g, b, a]) = drv.app.palette.primary();
        assert!(g > r && g > b);
        assert_eq!(a, custom.0[3]);
        assert!(drv.app.colors_open);

        drv.click(at(40.0), left, egui::Modifiers::NONE);
        assert_eq!(drv.app.palette.primary(), half_black);
        drv.app.colors_open = false;
        drv.step();
        let canvas = drv
            .ctx
            .read_response(egui::Id::new(CANVAS_ID))
            .unwrap()
            .rect;
        drv.click(canvas.center(), left, egui::Modifiers::NONE);
        assert_eq!(
            drv.app.tab().editor.document.pixel(0, 50, 50),
            Some(half_black)
        );
        Ok(())
    }

    #[test]
    fn color_dropdown_closes_on_escape_or_outside_click_without_shifting_canvas() {
        let mut drv = TestDriver::new(Document::new(32, 24).unwrap());
        drv.app.colors_open = false;
        drv.step();
        drv.step();
        let closed_central = drv.ctx.available_rect();

        drv.app.colors_open = true;
        drv.step();
        let open_central = drv.ctx.available_rect();
        assert_eq!(closed_central, open_central);
        assert!(drv.app.color_popup_rect.is_some());

        // ESC closes the Color Card dropdown
        drv.send_key(egui::Key::Escape, egui::Modifiers::NONE);
        assert!(!drv.app.colors_open);
        assert!(drv.app.color_popup_rect.is_none());

        // Pressing outside closes an open Color Card dropdown
        drv.app.colors_open = true;
        drv.step();
        assert!(drv.app.color_popup_rect.is_some());
        drv.run(
            egui::Modifiers::NONE,
            vec![
                egui::Event::PointerMoved(pos2(300.0, 400.0)),
                egui::Event::PointerButton {
                    pos: pos2(300.0, 400.0),
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert!(!drv.app.colors_open);
    }

    #[test]
    fn ctrl_double_click_bottom_right_enters_canvas_resize_and_commits_or_cancels() -> io::Result<()>
    {
        let mut doc = Document::new(100, 80)?;
        let red = Pixel([200, 40, 40, 255]);
        doc.fill_layer(0, red)?;
        let mut drv = TestDriver::new(doc);

        drv.step();
        drv.step();
        let canvas_rect = drv
            .ctx
            .read_response(egui::Id::new(CANVAS_ID))
            .unwrap()
            .rect;
        let corner = canvas_rect.right_bottom();

        // Ctrl + Double-Click on bottom-right corner enters interactive canvas resize mode
        for _ in 0..2 {
            drv.click(corner, PointerButton::Primary, egui::Modifiers::CTRL);
        }
        assert!(
            drv.app
                .tab()
                .session
                .as_ref()
                .is_some_and(|s| s.as_canvas_resize().is_some())
        );
        assert_eq!(
            drv.app
                .tab()
                .session
                .as_ref()
                .and_then(ActiveSession::as_canvas_resize)
                .unwrap()
                .preview_size,
            (100, 80)
        );

        // Dragging bottom-right corner outward updates preview_size; Esc cancels
        let expanded = canvas_rect.min + vec2(130.0, 100.0);
        drv.drag(corner, expanded);
        assert_eq!(
            drv.app
                .tab()
                .session
                .as_ref()
                .and_then(ActiveSession::as_canvas_resize)
                .unwrap()
                .preview_size,
            (130, 100)
        );
        drv.send_key(egui::Key::Escape, egui::Modifiers::NONE);
        assert!(drv.app.tab().session.is_none());
        assert_eq!(drv.app.tab().editor.document.dimensions(), (100, 80));

        // Let egui's multi-click streak reset before starting a fresh Ctrl + Double-Click
        for _ in 0..10 {
            drv.step();
        }

        // Re-enter via Ctrl + Double-Click, shrink canvas, and confirm with Ctrl rising edge
        for _ in 0..2 {
            drv.click(corner, PointerButton::Primary, egui::Modifiers::CTRL);
        }
        // Release initial Ctrl: must remain in resize mode
        drv.step();
        assert!(
            drv.app
                .tab()
                .session
                .as_ref()
                .is_some_and(|s| s.as_canvas_resize().is_some())
        );

        let shrunk = canvas_rect.min + vec2(60.0, 60.0);
        drv.drag(corner, shrunk);
        assert_eq!(
            drv.app
                .tab()
                .session
                .as_ref()
                .and_then(ActiveSession::as_canvas_resize)
                .unwrap()
                .preview_size,
            (60, 60)
        );
        // Pressing Ctrl confirms the new canvas size and pushes one Undo step
        drv.run(egui::Modifiers::CTRL, Vec::new());
        assert!(drv.app.tab().session.is_none());
        assert_eq!(drv.app.tab().editor.document.dimensions(), (60, 60));
        assert_eq!(drv.app.tab().editor.document.pixel(0, 10, 10), Some(red));
        assert!(drv.app.tab_mut().editor.undo()?);
        assert_eq!(drv.app.tab().editor.document.dimensions(), (100, 80));

        // Enter key also confirms a canvas resize
        drv.app.tab_mut().session = Some(ActiveSession::CanvasResize(CanvasResizeSession {
            original: (100, 80),
            preview_size: (110, 90),
            dragging: None,
            prev_ctrl: false,
        }));
        drv.send_key(egui::Key::Enter, egui::Modifiers::NONE);
        assert!(drv.app.tab().session.is_none());
        assert_eq!(drv.app.tab().editor.document.dimensions(), (110, 90));
        Ok(())
    }

    #[test]
    fn editable_persistence_atomic_save_as_preserves_original_and_handles_failure_or_cancel()
    -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let orig_path = dir.path().join("orig.pdn");
        let copy_path = dir.path().join("copy.pdn");

        let mut initial = Document::new(8, 8)?;
        initial.set_layer_name(0, "Base")?;
        initial.fill_layer(0, Pixel([10, 20, 30, 255]))?;
        let top = initial.add_layer("Camada 2")?;
        initial.set_layer_blend_mode(top, 1)?;
        initial.set_layer_properties(top, true, 180)?;
        initial.set_pixel(top, 3, 4, Pixel([200, 100, 50, 220]))?;
        write_pdn_atomically(&initial, &orig_path, false)?;

        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, Document::new(4, 4)?);
        app.open_path(&ctx, orig_path.clone());
        assert_eq!(app.tab().project_path.as_deref(), Some(orig_path.as_path()));
        assert!(!app.tab().editor.has_unsaved_edits());

        // Modify layer 1 and perform atomic Save
        app.tab_mut().editor.selected = 1;
        app.tab_mut()
            .editor
            .begin_stroke(Pixel([120, 220, 40, 255]), 1);
        app.tab_mut().editor.draw_to(Some((1, 1)));
        app.tab_mut().editor.finish_stroke();
        assert!(app.tab().editor.has_unsaved_edits());
        assert!(app.save_project(&ctx, false));
        assert!(!app.tab().editor.has_unsaved_edits());

        // Saved revision tracks across undo/redo
        assert!(app.tab_mut().editor.undo()?);
        assert!(app.tab().editor.has_unsaved_edits());
        assert!(app.tab_mut().editor.redo()?);
        assert!(!app.tab().editor.has_unsaved_edits());

        // Further edit, test Cancel vs Save As preserving original on disk
        let orig_bytes_before_copy = std::fs::read(&orig_path)?;
        app.tab_mut()
            .editor
            .begin_stroke(Pixel([250, 10, 10, 255]), 1);
        app.tab_mut().editor.draw_to(Some((6, 6)));
        app.tab_mut().editor.finish_stroke();
        let edited_pixel = app.tab().editor.document.pixel(1, 6, 6);
        assert!(app.tab().editor.has_unsaved_edits());

        // Simulate Save As dialog opened & canceled: state and disk must remain unchanged
        let _canceled_dialog = SaveOptionsDialog::new(
            copy_path.clone(),
            SaveFormat::pdn(),
            false,
            &app.tab().editor.document,
            &ctx,
        );
        assert!(!copy_path.exists());
        assert_eq!(app.tab().project_path.as_deref(), Some(orig_path.as_path()));
        assert!(app.tab().editor.has_unsaved_edits());

        // Complete Save As to copy_path: original orig.pdn stays byte-identical
        let save_as_dialog = SaveOptionsDialog::new(
            copy_path.clone(),
            SaveFormat::pdn(),
            false,
            &app.tab().editor.document,
            &ctx,
        );
        save_as_dialog.write_to_disk()?;
        app.tab_mut().name = display_name(&copy_path);
        app.tab_mut().project_path = Some(copy_path.clone());
        app.tab_mut().editor.mark_saved();
        assert_eq!(std::fs::read(&orig_path)?, orig_bytes_before_copy);
        let reopened_copy = Document::read_pdn(File::open(&copy_path)?)?;
        assert_eq!(reopened_copy.pixel(1, 6, 6), edited_pixel);
        assert_ne!(reopened_copy.pixel(1, 6, 6), initial.pixel(1, 6, 6));

        // Simulate write failure into missing directory: in-memory edits & last valid file survive
        let copy_bytes_before_fail = std::fs::read(&copy_path)?;
        let before_fail_px = app.tab().editor.document.pixel(1, 2, 2);
        app.tab_mut()
            .editor
            .begin_stroke(Pixel([15, 25, 240, 255]), 1);
        app.tab_mut().editor.draw_to(Some((2, 2)));
        app.tab_mut().editor.finish_stroke();
        let after_fail_px = app.tab().editor.document.pixel(1, 2, 2);
        assert_ne!(after_fail_px, before_fail_px);
        let failed_target = dir.path().join("missing_dir").join("fail.pdn");
        app.tab_mut().project_path = Some(failed_target);
        assert!(!app.save_project(&ctx, false));
        assert!(app.notice.as_ref().is_some_and(|(_, is_err)| *is_err));
        assert!(app.tab().editor.has_unsaved_edits());
        assert_eq!(app.tab().editor.document.pixel(1, 2, 2), after_fail_px);
        assert_eq!(std::fs::read(&copy_path)?, copy_bytes_before_fail);
        Ok(())
    }

    #[test]
    fn phase1_five_workflows_end_to_end() -> io::Result<()> {
        let fixture_path = fixture_path("pdn/pdn52_roundtrip_layers.pdn");
        let dir = tempfile::tempdir()?;
        let export_png = dir.path().join("workflow_export.png");
        let saved_pdn = dir.path().join("workflow_resumed.pdn");

        let mut drv = TestDriver::new(Document::new(4, 4)?);

        // 1. Open and understand a layered project (locate layer + hidden layer)
        drv.app.open_path(&drv.ctx, fixture_path);
        assert_eq!(drv.app.tab().editor.document.dimensions(), (96, 64));
        assert_eq!(drv.app.tab().editor.document.layer_count(), 4);
        assert_eq!(
            drv.app.tab().editor.document.layer_name(1),
            Some("Sombra 50")
        );
        assert_eq!(
            drv.app.tab().editor.document.layer_name(2),
            Some("Camada oculta")
        );
        assert_eq!(
            drv.app.tab().editor.document.layer_properties(2),
            Some((false, 255))
        );

        // Acceptance check: reorder, duplicate, merge down, then undo all three
        drv.app.tab_mut().editor.move_layer(1, 3, "move-layer")?;
        drv.app.tab_mut().editor.duplicate_layer(3)?;
        drv.app.tab_mut().editor.merge_layer_down(4)?;
        assert_eq!(drv.app.tab().editor.document.layer_count(), 4);
        for _ in 0..3 {
            assert!(drv.app.tab_mut().editor.undo()?);
        }
        assert_eq!(
            drv.app.tab().editor.document.layer_name(1),
            Some("Sombra 50")
        );
        assert_eq!(
            drv.app.tab().editor.document.layer_name(2),
            Some("Camada oculta")
        );

        // 2. Edit a layer: change opacity, draw a stroke, undo ONLY the stroke
        let before_meta = drv.app.tab().editor.layer_meta(1).unwrap();
        let px_before_stroke = drv.app.tab().editor.document.pixel(1, 20, 20);
        drv.app.tab_mut().editor.selected = 1;
        drv.app
            .tab_mut()
            .editor
            .set_properties(1, (before_meta.visible, 180))?;
        drv.app
            .tab_mut()
            .editor
            .record_properties(1, (before_meta.visible, before_meta.opacity));
        drv.app
            .tab_mut()
            .editor
            .begin_stroke(Pixel([250, 20, 20, 255]), 1);
        drv.app.tab_mut().editor.draw_to(Some((20, 20)));
        drv.app.tab_mut().editor.finish_stroke();
        assert_ne!(
            drv.app.tab().editor.document.pixel(1, 20, 20),
            px_before_stroke
        );
        drv.app.step_history(false);
        assert_eq!(
            drv.app.tab().editor.document.pixel(1, 20, 20),
            px_before_stroke
        );
        assert_eq!(
            drv.app.tab().editor.document.layer_properties(1),
            Some((true, 180))
        );

        // 3. Navigate and edit a detail via zoom + scroll, then return to overview
        drv.app.tab_mut().zoom = Some(4.0);
        drv.app.tab_mut().view.scroll_to = Some(vec2(40.0, 30.0));
        drv.step();
        drv.app
            .tab_mut()
            .editor
            .begin_stroke(Pixel([10, 230, 40, 255]), 1);
        drv.app.tab_mut().editor.draw_to(Some((35, 35)));
        drv.app.tab_mut().editor.finish_stroke();
        let detail_pixel = drv.app.tab().editor.document.pixel(1, 35, 35);
        drv.app.tab_mut().zoom = None;
        drv.step();

        // 4. Export PNG and verify compositing + transparency + hidden-layer exclusion
        let expected_composite = drv.app.tab().editor.document.composite()?;
        SaveOptionsDialog::new(
            export_png.clone(),
            SaveFormat::png(),
            false,
            &drv.app.tab().editor.document,
            &drv.ctx,
        )
        .write_to_disk()?;
        let exported = Document::read_png(BufReader::new(File::open(&export_png)?))?;
        assert_eq!(exported.composite()?, expected_composite);

        // 5. Save, close, reopen, verify all properties, and continue editing
        drv.app.tab_mut().project_path = Some(saved_pdn.clone());
        assert!(drv.app.save_project(&drv.ctx, false));
        drv.app.close_tab_at(0);
        drv.app.open_path(&drv.ctx, saved_pdn);
        assert_eq!(drv.app.tab().editor.document.dimensions(), (96, 64));
        assert_eq!(drv.app.tab().editor.document.layer_count(), 4);
        assert_eq!(
            drv.app.tab().editor.document.layer_name(1),
            Some("Sombra 50")
        );
        assert_eq!(
            drv.app.tab().editor.document.layer_properties(1),
            Some((true, 180))
        );
        assert_eq!(
            drv.app.tab().editor.document.layer_properties(2),
            Some((false, 255))
        );
        assert_eq!(drv.app.tab().editor.document.pixel(1, 35, 35), detail_pixel);

        drv.app.tab_mut().editor.selected = 0;
        let before_resume_px = drv.app.tab().editor.document.pixel(0, 2, 2);
        drv.app
            .tab_mut()
            .editor
            .begin_stroke(Pixel([99, 88, 77, 255]), 1);
        drv.app.tab_mut().editor.draw_to(Some((2, 2)));
        drv.app.tab_mut().editor.finish_stroke();
        assert_ne!(
            drv.app.tab().editor.document.pixel(0, 2, 2),
            before_resume_px
        );
        assert!(drv.app.tab_mut().editor.undo()?);
        assert_eq!(
            drv.app.tab().editor.document.pixel(0, 2, 2),
            before_resume_px
        );
        Ok(())
    }

    /// The phase 1 documents: `layers` uniform layers with mixed blend modes.
    fn build_workload_pdn(width: u32, height: u32, layers: usize) -> io::Result<Vec<u8>> {
        let mut doc = Document::new(width, height)?;
        doc.fill_layer(0, Pixel([48, 84, 140, 255]))?;
        for idx in 1..layers {
            let l = doc.add_layer(format!("Camada {idx}"))?;
            doc.set_layer_blend_mode(l, (idx % 4) as u8)?;
            doc.set_layer_properties(l, idx != 2, if idx % 2 == 0 { 255 } else { 168 })?;
            let fill = Pixel([
                ((idx * 53) % 256) as u8,
                ((idx * 97) % 256) as u8,
                ((idx * 37) % 256) as u8,
                192,
            ]);
            doc.fill_layer(l, fill)?;
        }
        let mut bytes = Vec::new();
        doc.write_pdn(&mut bytes)?;
        Ok(bytes)
    }

    /// Median and maximum.
    fn stats(samples: &[f64]) -> (f64, f64) {
        let mut sorted = samples.to_vec();
        sorted.sort_by(f64::total_cmp);
        let mid = sorted.len() / 2;
        let median = if sorted.len().is_multiple_of(2) {
            (sorted[mid - 1] + sorted[mid]) * 0.5
        } else {
            sorted[mid]
        };
        (median, *sorted.last().unwrap_or(&0.0))
    }

    fn vm_hwm_line() -> String {
        std::fs::read_to_string("/proc/self/status")
            .unwrap_or_default()
            .lines()
            .find(|line| line.starts_with("VmHWM:"))
            .map_or_else(|| "n/a".into(), |line| line.trim().into())
    }

    /// Frame times in milliseconds, one inner vector per timed run.
    #[derive(Default)]
    struct Phase1Samples {
        open: Vec<Vec<f64>>,
        stroke: Vec<Vec<f64>>,
        nav: Vec<Vec<f64>>,
        export: Vec<Vec<f64>>,
        stroke_undo_bytes: Vec<usize>,
    }

    fn run_means(runs: &[Vec<f64>]) -> Vec<f64> {
        runs.iter()
            .map(|frames| frames.iter().sum::<f64>() / frames.len() as f64)
            .collect()
    }

    fn measure_phase1_workload(
        width: u32,
        height: u32,
        layers: usize,
        warmups: usize,
        runs: usize,
    ) -> io::Result<Phase1Samples> {
        let pdn_bytes = build_workload_pdn(width, height, layers)?;
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0));
        let ctx = egui::Context::default();
        let mut samples = Phase1Samples::default();

        for iter in 0..(warmups + runs) {
            let t0 = std::time::Instant::now();
            let doc = Document::read_pdn(pdn_bytes.as_slice())?;
            let mut app = App::new(&ctx, doc);
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| app.ui_frame(ctx),
            );
            let open = vec![t0.elapsed().as_secs_f64() * 1000.0];

            let mut stroke = Vec::new();
            for step in 0..8_u32 {
                let t1 = std::time::Instant::now();
                app.tab_mut()
                    .editor
                    .begin_stroke(Pixel([220, 40, 40, 255]), 24);
                app.tab_mut()
                    .editor
                    .draw_to(Some((40 + step * 8, 40 + step * 4)));
                app.tab_mut().editor.finish_stroke();
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        ..Default::default()
                    },
                    |ctx| app.ui_frame(ctx),
                );
                stroke.push(t1.elapsed().as_secs_f64() * 1000.0);
                if iter >= warmups {
                    samples
                        .stroke_undo_bytes
                        .extend(app.tab().editor.last_undo_entry_bytes());
                }
            }

            let mut nav = Vec::new();
            for step in 0..10_u32 {
                let t2 = std::time::Instant::now();
                app.tab_mut().zoom = Some(1.0 + step as f32 * 0.2);
                app.tab_mut().view.scroll_to = Some(vec2(step as f32 * 16.0, step as f32 * 12.0));
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        ..Default::default()
                    },
                    |ctx| app.ui_frame(ctx),
                );
                nav.push(t2.elapsed().as_secs_f64() * 1000.0);
            }

            let t3 = std::time::Instant::now();
            let mut png_out = Vec::new();
            app.tab().editor.document.write_png(&mut png_out)?;
            let export = vec![t3.elapsed().as_secs_f64() * 1000.0];

            if iter >= warmups {
                samples.open.push(open);
                samples.stroke.push(stroke);
                samples.nav.push(nav);
                samples.export.push(export);
            }
        }
        Ok(samples)
    }

    #[test]
    #[ignore]
    fn phase1_interactive_performance_baseline() -> io::Result<()> {
        for (name, width, height, layers) in [("typical", 800, 600, 4), ("large", 2560, 1440, 8)] {
            let samples = measure_phase1_workload(width, height, layers, 2, 10)?;
            let (o_med, o_max) = stats(&run_means(&samples.open));
            let (s_med, s_max) = stats(&run_means(&samples.stroke));
            let (n_med, n_max) = stats(&run_means(&samples.nav));
            let (e_med, e_max) = stats(&run_means(&samples.export));
            let vm_hwm = vm_hwm_line();
            println!(
                "BENCH {name} ({width}x{height} x{layers}): open={o_med:.2}/{o_max:.2} ms | stroke={s_med:.2}/{s_max:.2} ms | nav={n_med:.2}/{n_max:.2} ms | export={e_med:.2}/{e_max:.2} ms | {vm_hwm}"
            );
        }
        Ok(())
    }

    #[test]
    fn hand_pan_and_wheel_clamp_without_flicker_and_allow_overscroll() -> io::Result<()> {
        let ctx = egui::Context::default();
        let doc = Document::new(640, 480)?;
        let mut app = App::new(&ctx, doc);
        app.tool = Tool::Pan;
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0));
        let mut time = 0.0_f64;
        let mut run = |app: &mut App, events| {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                time: Some(time),
                events,
                ..Default::default()
            };
            time += 0.016;
            ctx.run(input, |ctx| app.ui_frame(ctx))
        };

        let _ = run(&mut app, Vec::new());
        let texture_id = app.tab().image.as_ref().unwrap().id();
        let painted_canvas = |output: egui::FullOutput| {
            output
                .shapes
                .into_iter()
                .find_map(|shape| match shape.shape {
                    egui::Shape::Mesh(mesh) if mesh.texture_id == texture_id => {
                        Some(mesh.calc_bounds())
                    }
                    _ => None,
                })
                .expect("painted canvas")
        };
        let initial_rect = painted_canvas(run(&mut app, Vec::new()));
        let start = initial_rect.center();
        let _ = run(
            &mut app,
            vec![egui::Event::PointerMoved(start - vec2(80.0, 30.0))],
        );

        // Dragging with Hand tool pans even when the 640x480 canvas fits inside the viewport.
        let pressed = run(
            &mut app,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert_eq!(painted_canvas(pressed), initial_rect);
        assert_eq!(painted_canvas(run(&mut app, Vec::new())), initial_rect);
        for delta in [vec2(10.0, 6.0), vec2(25.0, 18.0), vec2(40.0, 30.0)] {
            let rect = painted_canvas(run(
                &mut app,
                vec![egui::Event::PointerMoved(start + delta)],
            ));
            assert_eq!(rect, initial_rect.translate(delta));
            assert_eq!(painted_canvas(run(&mut app, Vec::new())), rect);
        }
        let dragged = start + vec2(40.0, 30.0);
        let _ = run(
            &mut app,
            vec![egui::Event::PointerButton {
                pos: dragged,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let _ = run(&mut app, Vec::new());
        let panned_rect = ctx.read_response(egui::Id::new(CANVAS_ID)).unwrap().rect;
        assert!(
            (panned_rect.min.x - (initial_rect.min.x + 40.0)).abs() < 1.0
                && (panned_rect.min.y - (initial_rect.min.y + 30.0)).abs() < 1.0,
            "expected canvas to pan with overscroll margin: before={:?}, after={:?}",
            initial_rect.min,
            panned_rect.min
        );

        // Scrolling far past the max_offset limit clamps cleanly and never flickers on further input.
        let _ = run(
            &mut app,
            vec![
                egui::Event::PointerMoved(panned_rect.center()),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: vec2(-10_000.0, -10_000.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let _ = run(&mut app, Vec::new());
        let clamped_offset = app.tab().view.offset;
        let clamped_rect = ctx.read_response(egui::Id::new(CANVAS_ID)).unwrap().rect;
        // Repeatedly scrolling past the boundary must not overshoot or oscillate on any frame.
        for _ in 0..3 {
            let _ = run(
                &mut app,
                vec![
                    egui::Event::PointerMoved(clamped_rect.center()),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: vec2(-200.0, -200.0),
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            assert_eq!(app.tab().view.offset, clamped_offset);
            assert_eq!(
                ctx.read_response(egui::Id::new(CANVAS_ID)).unwrap().rect,
                clamped_rect
            );
            let _ = run(&mut app, Vec::new());
            assert_eq!(app.tab().view.offset, clamped_offset);
            assert_eq!(
                ctx.read_response(egui::Id::new(CANVAS_ID)).unwrap().rect,
                clamped_rect
            );
        }
        Ok(())
    }

    #[test]
    fn inspector_cards_stay_within_right_panel_bounds() -> io::Result<()> {
        let mut drv = TestDriver::with_screen(Document::new(640, 480)?, 1120.0, 760.0);
        drv.step();
        let output = drv.step();
        let inspector_left = drv.screen.right() - 246.0;
        let max_allowed_right = drv.screen.right() - f32::from(CARD_GAP) + 2.0;
        for clipped in &output.shapes {
            let bounds = clipped.shape.visual_bounding_rect();
            // Skip the SidePanel's full-width background rect (874..1120) and inspect the cards inside.
            if bounds.is_positive()
                && bounds.left() >= inspector_left
                && (bounds.width() < 245.0 || bounds.left() > inspector_left + 1.0)
                && bounds.top() > 100.0
                && bounds.bottom() < drv.screen.bottom() - 30.0
            {
                assert!(
                    bounds.right() <= max_allowed_right,
                    "inspector shape overflows right panel bound: {bounds:?} > {max_allowed_right}"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn layer_card_shortcuts_require_a_click_and_keep_canvas_delete() -> io::Result<()> {
        let mut doc = new_document(32, 24)?;
        doc.add_layer("Top")?;
        let mut drv = TestDriver::with_screen(doc, 1120.0, 760.0);

        drv.step();
        drv.step();
        drv.press_key(egui::Key::Insert);
        assert_eq!(drv.app.tab().editor.document.layer_count(), 2);
        let card = drv
            .ctx
            .read_response(egui::Id::new(LAYERS_ID))
            .unwrap()
            .rect;
        drv.click(
            card.left_top() + vec2(12.0, 12.0),
            PointerButton::Primary,
            egui::Modifiers::NONE,
        );
        assert!(drv.app.layers_clicked);
        drv.press_key(egui::Key::Insert);
        assert_eq!(drv.app.tab().editor.document.layer_count(), 3);
        assert_eq!(drv.app.tab().editor.selected, 1);
        assert_eq!(drv.app.tab().editor.document.layer_name(2), Some("Top"));
        assert_eq!(
            drv.app.tab().editor.document.pixel(1, 0, 0),
            Some(Pixel::default())
        );
        drv.press_key(egui::Key::Delete);
        assert_eq!(drv.app.tab().editor.document.layer_count(), 2);
        assert_eq!(drv.app.tab().editor.document.layer_name(1), Some("Top"));
        assert!(drv.app.tab_mut().editor.undo()?);
        assert_eq!(drv.app.tab().editor.document.layer_count(), 3);
        assert!(drv.app.tab_mut().editor.undo()?);
        assert_eq!(drv.app.tab().editor.document.layer_count(), 2);

        drv.app.tool = Tool::Pan;
        let canvas = drv
            .ctx
            .read_response(egui::Id::new(CANVAS_ID))
            .unwrap()
            .rect;
        drv.click(
            canvas.center(),
            PointerButton::Primary,
            egui::Modifiers::NONE,
        );
        assert!(!drv.app.layers_clicked);
        drv.app.tab_mut().editor.select_all();
        drv.press_key(egui::Key::Delete);
        assert_eq!(drv.app.tab().editor.document.layer_count(), 2);
        assert_eq!(
            drv.app.tab().editor.document.pixel(0, 0, 0),
            Some(Pixel::default())
        );

        drv.click(
            card.left_top() + vec2(12.0, 12.0),
            PointerButton::Primary,
            egui::Modifiers::NONE,
        );
        drv.press_key(egui::Key::Delete);
        drv.press_key(egui::Key::Delete);
        assert_eq!(drv.app.tab().editor.document.layer_count(), 1);
        Ok(())
    }

    #[test]
    fn dialogs_and_window_use_the_layers_card_radius() -> io::Result<()> {
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1120.0, 760.0));
        let card_radius = egui::CornerRadius::same(CARD_CORNER_RADIUS);
        for theme in [Theme::Light, Theme::Dark] {
            for modal_open in [false, true] {
                let ctx = egui::Context::default();
                let mut app = App::with_settings(
                    &ctx,
                    new_document(32, 24)?,
                    GlobalSettings {
                        theme,
                        ..Default::default()
                    },
                );
                app.tool_size_popup = !modal_open;
                app.new_size = modal_open.then_some((32, 24));
                let mut draw = |time| {
                    ctx.run(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            time: Some(time),
                            ..Default::default()
                        },
                        |ctx| app.ui_frame(ctx),
                    )
                };
                let _ = draw(0.0);
                let output = draw(1.0);
                let has_rect = |predicate: &dyn Fn(&egui::epaint::RectShape) -> bool| {
                    output.shapes.iter().any(|clipped| match &clipped.shape {
                        egui::Shape::Rect(rect) => predicate(rect),
                        egui::Shape::Vec(shapes) => shapes.iter().any(
                            |shape| matches!(shape, egui::Shape::Rect(rect) if predicate(rect)),
                        ),
                        _ => false,
                    })
                };
                assert!(has_rect(&|rect| rect.rect == screen
                    && rect.corner_radius == card_radius
                    && rect.stroke.color == theme.border_color()));
                assert!(has_rect(&|rect| rect.rect.right() < screen.right() - 246.0
                    && rect.corner_radius == card_radius
                    && rect.fill == theme.card_fill()
                    && rect.stroke.width > 0.0));
                if modal_open {
                    assert!(has_rect(&|rect| rect.rect == screen
                        && rect.corner_radius == card_radius
                        && rect.fill == Color32::from_black_alpha(100)));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn window_corner_radius_matches_windowed_and_fullscreen_states() {
        let ctx = egui::Context::default();
        assert_eq!(
            window_chrome::corner_radius(&ctx, false),
            CARD_CORNER_RADIUS
        );
        assert_eq!(window_chrome::corner_radius(&ctx, true), 0);
    }
}
