mod app_settings;
mod bucket_edges;
mod clipboard;
mod colors;
mod dialogs;
mod editor;
mod feather;
mod geometry;
mod gradient_tool;
mod i18n;
mod icon_atlas;
mod mouse_shortcuts;
mod phase2;
mod selection;
mod shortcuts;
mod stroke_raster;
mod text_tool;
mod theme;
mod tolerance;
mod tool_icons;
mod transform;
mod vector_tools;
mod view;
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
use openpaintr::{Document, Pixel};
use rfd::FileDialog;

use app_settings::{AaMode, GlobalSettings};
use colors::{ColorPalette, PaletteAction};
use dialogs::{
    LayerDialogAction, LayerPropertiesDialog, RecentFiles, SaveDialogAction, SaveOptionsDialog,
    atomic_write_bytes, make_thumbnail_texture, modal,
};
use editor::{Editor, LayerMeta, StrokeSpec};
use gradient_tool::{
    GradientChannelMode, GradientRepeat, GradientSession, GradientShape, SessionOutcome, snap_if,
};
use i18n::text as tr;
use mouse_shortcuts::{BUTTONS, MouseAction, Scope, Trigger};
use phase2::SizeDialog;
use selection::{Combine, Point, Region, Selection, Shape};
use shortcuts::{Command, ShortcutDialog, Shortcuts};
use text_tool::{FontCatalog, TextAlign, TextSession};
use theme::{CARD_CORNER_RADIUS, CARD_GAP, Theme};
use tolerance::{AlphaMatchMode, FillPattern, StrokeBlend};
use transform::{Transform, TransformOutcome, TransformTarget};
use vector_tools::{
    CurveKind, DashPreset, LineCapStyle, LineCurveSession, ShapeFamily, ShapeFillMode, ShapeKind,
    ShapeSession,
};
use view::View;

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
            app.locale = locale;
            creation.egui_ctx.set_fonts(i18n::fonts(locale));
            app.font_catalog = FontCatalog::for_locale(locale);
            app.locale.with(|| {
                app.shortcuts = Shortcuts::from_executable();
                if let Some(error) = &app.shortcuts.storage_error {
                    app.notice = Some((i18n::format("shortcuts-error", &[error]), true));
                }
                app.recent = RecentFiles::from_executable();
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum CanvasResizeDrag {
    BottomRight,
    RightEdge,
    BottomEdge,
}

#[derive(Clone)]
struct CanvasResizeSession {
    original: Point,
    preview_size: Point,
    dragging: Option<CanvasResizeDrag>,
    prev_ctrl: bool,
}

struct DocumentTab {
    editor: Editor,
    name: String,
    project_path: Option<PathBuf>,
    zoom: Option<f32>,
    display_zoom: f32,
    view: View,
    transform: Option<Transform>,
    gradient: Option<GradientSession>,
    line_curve: Option<LineCurveSession>,
    shape_session: Option<ShapeSession>,
    text_session: Option<TextSession>,
    canvas_resize: Option<CanvasResizeSession>,
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
            transform: None,
            gradient: None,
            line_curve: None,
            shape_session: None,
            text_session: None,
            canvas_resize: None,
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
    gradient_shape: GradientShape,
    gradient_repeat: GradientRepeat,
    gradient_mode: GradientChannelMode,
    gradient_dither: bool,
    curve_kind: CurveKind,
    shape_thickness: u32,
    dash_preset: DashPreset,
    line_start_cap: LineCapStyle,
    line_end_cap: LineCapStyle,
    shape_kind: ShapeKind,
    shape_fill_mode: ShapeFillMode,
    shape_corner_radius: f32,
    font_catalog: FontCatalog,
    settings: GlobalSettings,
    text_family: String,
    text_size: f32,
    text_bold: bool,
    text_italic: bool,
    text_underline: bool,
    text_strikethrough: bool,
    text_align: TextAlign,
    checker: TextureHandle,
    notice: Option<(String, bool)>,
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
            gradient_shape: GradientShape::Linear,
            gradient_repeat: GradientRepeat::None,
            gradient_mode: GradientChannelMode::Colors,
            gradient_dither: false,
            curve_kind: CurveKind::CubicBezier,
            shape_thickness: 2,
            dash_preset: DashPreset::Solid,
            line_start_cap: LineCapStyle::Flat,
            line_end_cap: LineCapStyle::Round,
            shape_kind: ShapeKind::Rectangle,
            shape_fill_mode: ShapeFillMode::Both,
            shape_corner_radius: 8.0,
            font_catalog: FontCatalog::new(),
            settings,
            text_family: "Padrão (Sans)".to_owned(),
            text_size: 28.0,
            text_bold: false,
            text_italic: false,
            text_underline: false,
            text_strikethrough: false,
            text_align: TextAlign::Left,
            checker,
            notice: None,
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

    fn request(&mut self, ctx: &egui::Context, action: Action) {
        self.finish_opacity();
        self.tab_mut().editor.finish_stroke();
        self.commit_active_session();
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
                } else if scope == CloseScope::All {
                    self.tabs.truncate(1);
                    self.active_tab = 0;
                    self.close_tab_at(0);
                } else {
                    self.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
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
                        self.tabs.truncate(1);
                        self.active_tab = 0;
                        self.close_tab_at(0);
                        return;
                    }
                } else {
                    self.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
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
                if let Some(path) = FileDialog::new()
                    .set_title(tr("open"))
                    .add_filter(tr("supported-files"), &[PDN_EXT, "png"])
                    .add_filter(tr("png-image"), &["png"])
                    .add_filter(tr("pdn-document"), &["pdn"])
                    .pick_file()
                {
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
            let limit = ctx.input(|input| input.max_texture_side);
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
        if let Some(path) = FileDialog::new()
            .set_title(tr("import-layer-file"))
            .add_filter(tr("supported-files"), &[PDN_EXT, "png"])
            .add_filter(tr("png-image"), &["png"])
            .add_filter(tr("pdn-document"), &["pdn"])
            .pick_file()
        {
            let result = (|| {
                let (imported, _) = open_document(&path)?;
                let cur = self.tab().editor.document.dimensions();
                let imp = imported.dimensions();
                let limit = ctx.input(|input| input.max_texture_side);
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
        self.finish_opacity();
        self.tab_mut().editor.finish_stroke();
        self.commit_active_session();
        if !save_as && let Some(path) = self.tab().project_path.clone() {
            let result = (|| {
                let mut bytes = Vec::new();
                self.tab().editor.document.write_pdn(&mut bytes)?;
                atomic_write_bytes(&path, &bytes, true)
            })();
            let saved = result.is_ok();
            if saved {
                self.tab_mut().name = display_name(&path);
                self.tab_mut().editor.mark_saved();
                self.recent.push(path);
            }
            self.report(result, tr("project-saved"));
            return saved;
        }

        let base = Path::new(self.tab().title())
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_else(|| self.tab().title())
            .to_owned();
        let Some(mut path) = FileDialog::new()
            .set_title(tr("save-as"))
            .add_filter(tr("pdn-document"), &[PDN_EXT])
            .set_file_name(format!("{base}.{PDN_EXT}"))
            .save_file()
        else {
            return false;
        };
        let extension_added = path.extension().is_none();
        if extension_added {
            path.set_extension(PDN_EXT);
        }
        if !has_extension(&path, PDN_EXT) {
            self.report(
                Err(io::Error::new(io::ErrorKind::InvalidInput, tr("use-pdn"))),
                "",
            );
            return false;
        }
        let needs_confirm = extension_added && path.exists();
        self.save_dialog = Some(SaveOptionsDialog::new_pdn(
            path,
            needs_confirm,
            &self.tab().editor.document,
            ctx,
        ));
        false
    }

    fn export(&mut self, ctx: &egui::Context) {
        self.finish_opacity();
        self.tab_mut().editor.finish_stroke();
        self.commit_active_session();
        let base = Path::new(self.tab().title())
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_else(|| self.tab().title())
            .to_owned();
        if let Some(mut path) = FileDialog::new()
            .set_title(tr("export-png"))
            .add_filter(tr("png-image"), &["png"])
            .set_file_name(format!("{base}.png"))
            .save_file()
        {
            let extension_added = path.extension().is_none();
            if extension_added {
                path.set_extension("png");
            }
            if !has_extension(&path, "png") {
                self.report(
                    Err(io::Error::new(io::ErrorKind::InvalidInput, tr("use-png"))),
                    "",
                );
                return;
            }
            let needs_confirm = extension_added && path.exists();
            self.save_dialog = Some(SaveOptionsDialog::new_png(
                path,
                needs_confirm,
                &self.tab().editor.document,
                ctx,
            ));
        }
    }

    fn report(&mut self, result: io::Result<()>, success: &str) {
        self.notice = match result {
            Ok(()) if success.is_empty() => None,
            Ok(()) => Some((success.into(), false)),
            Err(error) => Some((i18n::error_message(&error), true)),
        };
    }

    fn step_history(&mut self, redo: bool) {
        self.finish_opacity();
        self.commit_active_session();
        let result = if redo {
            self.tab_mut().editor.redo()
        } else {
            self.tab_mut().editor.undo()
        };
        match result {
            Ok(changed) => {
                if changed {
                    self.tab_mut().thumbs_dirty = true;
                }
            }
            Err(error) => self.report(Err(error), ""),
        }
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
        self.notice = self.locale.with(|| {
            self.settings
                .save()
                .err()
                .map(|error| (i18n::format("settings-save-error", &[&error]), true))
        });
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
                    if ui
                        .add(
                            egui::Button::new(tr("shortcuts"))
                                .shortcut_text(self.shortcuts.hint(Command::Settings)),
                        )
                        .clicked()
                    {
                        self.shortcut_dialog = Some(self.shortcuts.dialog());
                        ui.close();
                    }
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
                                self.notice = locale.with(|| {
                                    locale.save().err().map(|error| {
                                        (i18n::format("language-save-error", &[&error]), true)
                                    })
                                });
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

    fn command_bar(&mut self, ui: &mut egui::Ui) {
        use tool_icons::{ActionIcon, icon_button};
        let compact = ui.available_width() < 850.0;
        let wrap_wand = matches!(self.tool, Tool::Wand | Tool::Bucket | Tool::Recolor)
            && !self.has_active_session()
            && ui.available_width() < 1120.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let btn_sz = vec2(26.0, 26.0);
            if icon_button(
                ui,
                self.settings.theme,
                ActionIcon::NewDoc,
                true,
                false,
                btn_sz,
                &i18n::format(
                    "shortcut-with-hint",
                    &[
                        &i18n::logical_text("new"),
                        &self.shortcuts.hint(Command::New),
                    ],
                ),
            )
            .clicked()
            {
                self.perform(ui.ctx(), Action::New);
            }
            if icon_button(
                ui,
                self.settings.theme,
                ActionIcon::OpenFolder,
                true,
                false,
                btn_sz,
                &i18n::format(
                    "shortcut-with-hint",
                    &[
                        &i18n::logical_text("open"),
                        &self.shortcuts.hint(Command::Open),
                    ],
                ),
            )
            .clicked()
            {
                self.perform(ui.ctx(), Action::Open);
            }
            if icon_button(
                ui,
                self.settings.theme,
                ActionIcon::SaveDisk,
                true,
                false,
                btn_sz,
                &i18n::format(
                    "shortcut-with-hint",
                    &[
                        &i18n::logical_text("save"),
                        &self.shortcuts.hint(Command::Save),
                    ],
                ),
            )
            .clicked()
            {
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
            if icon_button(
                ui,
                self.settings.theme,
                ActionIcon::Undo,
                self.tab().editor.has_edits(),
                false,
                btn_sz,
                &i18n::format(
                    "shortcut-with-hint",
                    &[
                        &i18n::logical_text("undo"),
                        &self.shortcuts.hint(Command::Undo),
                    ],
                ),
            )
            .clicked()
            {
                self.step_history(false);
            }
            if icon_button(
                ui,
                self.settings.theme,
                ActionIcon::Redo,
                self.tab().editor.can_redo(),
                false,
                btn_sz,
                &i18n::format(
                    "shortcut-with-hint",
                    &[
                        &i18n::logical_text("redo"),
                        &self.shortcuts.hint(Command::Redo),
                    ],
                ),
            )
            .clicked()
            {
                self.step_history(true);
            }

            ui.separator();
            if self.tab().transform.is_some() {
                let mut method = self.tab().transform.as_ref().unwrap().method;
                let (target, shift, scale, degrees) = {
                    let t = self.tab().transform.as_ref().unwrap();
                    (t.target, t.affine.shift, t.affine.scale, t.affine.degrees)
                };
                ui.label(
                    RichText::new(tr("transform-field"))
                        .size(12.0)
                        .color(self.settings.theme.text_muted()),
                );
                ui.label(
                    RichText::new(target.short_label())
                        .strong()
                        .size(12.5)
                        .color(self.settings.theme.text_primary()),
                );
                if target != TransformTarget::Mask {
                    let prev = method;
                    phase2::resampling_selector(ui, &mut method);
                    if method != prev {
                        let tab = self.tab_mut();
                        if let Some(transform) = tab.transform.as_mut() {
                            transform.method = method;
                        }
                        tab.editor.mark_full_refresh();
                    }
                }
                ui.label(
                    RichText::new(format!(
                        "Δ({:.0}, {:.0}) px · {:.0}%×{:.0}% · {:.1}°",
                        shift.0,
                        shift.1,
                        scale.0 * 100.0,
                        scale.1 * 100.0,
                        degrees
                    ))
                    .size(12.0)
                    .color(self.settings.theme.text_muted()),
                );
                self.session_commit_buttons(ui);
            } else if let Some((mut shape, mut repeat, mut mode, mut dither, mut blend)) = self
                .tab()
                .gradient
                .as_ref()
                .map(|g| (g.shape, g.repeat, g.channel_mode, g.dither, g.blend))
            {
                ui.label(
                    RichText::new(tr("gradient-field"))
                        .strong()
                        .size(12.5)
                        .color(self.settings.theme.text_primary()),
                );
                let changed = Self::gradient_controls(
                    ui,
                    &mut shape,
                    &mut repeat,
                    &mut mode,
                    &mut dither,
                    &mut blend,
                );
                if changed {
                    self.gradient_shape = shape;
                    self.gradient_repeat = repeat;
                    self.gradient_mode = mode;
                    self.gradient_dither = dither;
                    self.stroke_blend = blend;
                    let tab = self.tab_mut();
                    if let Some(g) = tab.gradient.as_mut() {
                        g.shape = shape;
                        g.repeat = repeat;
                        g.channel_mode = mode;
                        g.dither = dither;
                        g.blend = blend;
                    }
                    tab.editor.mark_full_refresh();
                }
                self.session_commit_buttons(ui);
            } else if let Some((
                mut kind,
                mut thickness,
                mut dash,
                mut start_cap,
                mut end_cap,
                mut blend,
            )) = self.tab().line_curve.as_ref().map(|lc| {
                (
                    lc.kind,
                    lc.thickness,
                    lc.dash,
                    lc.start_cap,
                    lc.end_cap,
                    lc.blend,
                )
            }) {
                ui.label(
                    RichText::new(tr("line-curve-field"))
                        .strong()
                        .size(12.5)
                        .color(self.settings.theme.text_primary()),
                );
                let changed = Self::line_curve_controls(
                    ui,
                    &mut kind,
                    &mut thickness,
                    &mut dash,
                    &mut start_cap,
                    &mut end_cap,
                    &mut blend,
                );
                if changed {
                    self.curve_kind = kind;
                    self.shape_thickness = thickness;
                    self.dash_preset = dash;
                    self.line_start_cap = start_cap;
                    self.line_end_cap = end_cap;
                    self.stroke_blend = blend;
                    let tab = self.tab_mut();
                    if let Some(lc) = tab.line_curve.as_mut() {
                        lc.kind = kind;
                        lc.thickness = thickness;
                        lc.dash = dash;
                        lc.start_cap = start_cap;
                        lc.end_cap = end_cap;
                        lc.blend = blend;
                    }
                    tab.editor.mark_full_refresh();
                }
                self.session_commit_buttons(ui);
            } else if let Some((
                mut kind,
                mut fill_mode,
                mut thickness,
                mut radius,
                mut pattern,
                mut blend,
            )) = self.tab().shape_session.as_ref().map(|s| {
                (
                    s.kind,
                    s.fill_mode,
                    s.thickness,
                    s.corner_radius,
                    s.pattern,
                    s.blend,
                )
            }) {
                ui.label(
                    RichText::new(tr("shape-field"))
                        .strong()
                        .size(12.5)
                        .color(self.settings.theme.text_primary()),
                );
                let changed = Self::shape_controls(
                    ui,
                    &mut kind,
                    &mut fill_mode,
                    &mut thickness,
                    &mut radius,
                    &mut pattern,
                    &mut blend,
                );
                if changed {
                    self.shape_kind = kind;
                    self.shape_fill_mode = fill_mode;
                    self.shape_thickness = thickness;
                    self.shape_corner_radius = radius;
                    self.fill_pattern = pattern;
                    self.stroke_blend = blend;
                    let tab = self.tab_mut();
                    if let Some(s) = tab.shape_session.as_mut() {
                        s.kind = kind;
                        s.fill_mode = fill_mode;
                        s.thickness = thickness;
                        s.corner_radius = radius;
                        s.pattern = pattern;
                        s.blend = blend;
                    }
                    tab.editor.mark_full_refresh();
                }
                self.session_commit_buttons(ui);
            } else if let Some((
                mut text,
                mut family,
                mut size_px,
                mut bold,
                mut italic,
                mut underline,
                mut strike,
                mut align,
                mut blend,
            )) = self.tab().text_session.as_ref().map(|t| {
                (
                    t.text.clone(),
                    t.family.clone(),
                    t.size_px,
                    t.bold,
                    t.italic,
                    t.underline,
                    t.strikethrough,
                    t.align,
                    t.blend,
                )
            }) {
                ui.label(
                    RichText::new(tr("text-field"))
                        .strong()
                        .size(12.5)
                        .color(self.settings.theme.text_primary()),
                );
                let text_resp = ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .desired_width(120.0)
                        .hint_text(tr("placeholder-text")),
                );
                let changed = text_resp.changed()
                    | Self::text_controls(
                        ui,
                        &self.font_catalog.families,
                        &mut family,
                        &mut size_px,
                        &mut bold,
                        &mut italic,
                        &mut underline,
                        &mut strike,
                        &mut align,
                        &mut blend,
                    );
                if changed {
                    self.text_family = family.clone();
                    self.text_size = size_px;
                    self.text_bold = bold;
                    self.text_italic = italic;
                    self.text_underline = underline;
                    self.text_strikethrough = strike;
                    self.text_align = align;
                    self.stroke_blend = blend;
                    let tab = self.tab_mut();
                    if let Some(t) = tab.text_session.as_mut() {
                        t.text = text;
                        t.family = family;
                        t.size_px = size_px;
                        t.bold = bold;
                        t.italic = italic;
                        t.underline = underline;
                        t.strikethrough = strike;
                        t.align = align;
                        t.blend = blend;
                    }
                    tab.editor.mark_full_refresh();
                }
                self.session_commit_buttons(ui);
            } else if let Some((original, mut preview_size)) = self
                .tab()
                .canvas_resize
                .as_ref()
                .map(|c| (c.original, c.preview_size))
            {
                let limit = ui.ctx().input(|i| i.max_texture_side).clamp(1, 8192) as u32;
                ui.label(
                    RichText::new(tr("canvas-size-field"))
                        .strong()
                        .size(12.5)
                        .color(self.settings.theme.text_primary()),
                );
                let mut changed = false;
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut preview_size.0)
                            .range(1..=limit)
                            .suffix(" px"),
                    )
                    .changed();
                ui.label("×");
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut preview_size.1)
                            .range(1..=limit)
                            .suffix(" px"),
                    )
                    .changed();
                if changed && let Some(cr) = self.tab_mut().canvas_resize.as_mut() {
                    cr.preview_size = preview_size;
                }
                ui.label(
                    RichText::new(i18n::format(
                        "canvas-resize-hint",
                        &[&original.0, &original.1],
                    ))
                    .size(11.5)
                    .color(self.settings.theme.text_muted()),
                );
                self.session_commit_buttons(ui);
            } else {
                ui.label(
                    RichText::new(tr("tool-field"))
                        .size(12.0)
                        .color(self.settings.theme.text_muted()),
                );
                tool_icons::paint_small_tool(ui, self.tool);
                ui.label(
                    RichText::new(self.tool.label())
                        .strong()
                        .size(12.5)
                        .color(self.settings.theme.text_primary()),
                );

                ui.spacing_mut().slider_width = if compact { 50.0 } else { 80.0 };
                match self.tool {
                    Tool::Brush | Tool::Eraser | Tool::CloneStamp => {
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(tr("size-field"))
                                .size(12.0)
                                .color(self.settings.theme.text_muted()),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.diameter, 1..=MAX_BRUSH_DIAMETER)
                                .suffix(" px"),
                        );
                        ui.label(
                            RichText::new(tr("hardness-field"))
                                .size(12.0)
                                .color(self.settings.theme.text_muted()),
                        );
                        ui.add(egui::Slider::new(&mut self.hardness, 0..=100).suffix("%"));
                        ui.label(
                            RichText::new(tr("spacing-field"))
                                .size(12.0)
                                .color(self.settings.theme.text_muted()),
                        );
                        ui.add(egui::Slider::new(&mut self.spacing, 1..=200).suffix("%"));
                        if self.tool == Tool::Brush {
                            Self::pattern_selector(ui, "brush_pattern", &mut self.fill_pattern);
                        }
                        if self.tool != Tool::Eraser {
                            Self::blend_selector(ui, "stroke_blend", &mut self.stroke_blend);
                        }
                        if self.tool == Tool::CloneStamp {
                            let hint = self.tab().clone_anchor.map_or_else(
                                || {
                                    i18n::format(
                                        "clone-source-unset",
                                        &[&self.shortcuts.mouse_hint(MouseAction::CloneSetAnchor)],
                                    )
                                },
                                |(x, y)| i18n::format("clone-source", &[&x, &y]),
                            );
                            ui.label(
                                RichText::new(hint)
                                    .size(11.5)
                                    .color(self.settings.theme.text_muted()),
                            );
                        }
                    }
                    Tool::Pencil => {
                        ui.add_space(6.0);
                        ui.label(tr("size-field"));
                        ui.add(
                            egui::Slider::new(&mut self.pencil_diameter, 1..=MAX_BRUSH_DIAMETER)
                                .suffix(" px"),
                        );
                        Self::blend_selector(ui, "pencil_blend", &mut self.stroke_blend);
                    }
                    Tool::Bucket => {
                        if !wrap_wand {
                            ui.add_space(4.0);
                            self.bucket_options(ui);
                        }
                    }
                    Tool::Recolor => {
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(tr("short-size-field"))
                                .size(12.0)
                                .color(self.settings.theme.text_muted()),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.diameter, 1..=MAX_BRUSH_DIAMETER)
                                .suffix(" px"),
                        );
                        ui.label(
                            RichText::new(tr("short-hardness-field"))
                                .size(12.0)
                                .color(self.settings.theme.text_muted()),
                        );
                        ui.add(egui::Slider::new(&mut self.hardness, 0..=100).suffix("%"));
                        let target = self.recolor_target.unwrap_or(self.palette.secondary);
                        let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                        ui.painter().rect_filled(
                            r,
                            3.0,
                            Color32::from_rgba_unmultiplied(
                                target.0[0],
                                target.0[1],
                                target.0[2],
                                target.0[3],
                            ),
                        );
                        ui.painter().rect_stroke(
                            r,
                            3.0,
                            egui::Stroke::new(1.0_f32, self.settings.theme.border_color()),
                            egui::StrokeKind::Outside,
                        );
                        if !wrap_wand {
                            self.tolerance_options(ui, "recolor_alpha");
                        }
                    }
                    Tool::Eyedropper => {
                        ui.add_space(6.0);
                        ui.checkbox(&mut self.eyedropper_composite, tr("sample-composite"));
                        ui.checkbox(&mut self.eyedropper_stay, tr("stay-eyedropper"));
                    }
                    Tool::Gradient => {
                        Self::gradient_controls(
                            ui,
                            &mut self.gradient_shape,
                            &mut self.gradient_repeat,
                            &mut self.gradient_mode,
                            &mut self.gradient_dither,
                            &mut self.stroke_blend,
                        );
                    }
                    Tool::LineCurve => {
                        Self::line_curve_controls(
                            ui,
                            &mut self.curve_kind,
                            &mut self.shape_thickness,
                            &mut self.dash_preset,
                            &mut self.line_start_cap,
                            &mut self.line_end_cap,
                            &mut self.stroke_blend,
                        );
                    }
                    Tool::DrawShape => {
                        Self::shape_controls(
                            ui,
                            &mut self.shape_kind,
                            &mut self.shape_fill_mode,
                            &mut self.shape_thickness,
                            &mut self.shape_corner_radius,
                            &mut self.fill_pattern,
                            &mut self.stroke_blend,
                        );
                    }
                    Tool::Text => {
                        Self::text_controls(
                            ui,
                            &self.font_catalog.families,
                            &mut self.text_family,
                            &mut self.text_size,
                            &mut self.text_bold,
                            &mut self.text_italic,
                            &mut self.text_underline,
                            &mut self.text_strikethrough,
                            &mut self.text_align,
                            &mut self.stroke_blend,
                        );
                    }
                    Tool::Select(_) | Tool::Lasso | Tool::Wand => {
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(tr("mode-field"))
                                .size(12.0)
                                .color(self.settings.theme.text_muted()),
                        );
                        self.combine_selector(ui);
                        ui.label(tr("feather-field"));
                        ui.add(egui::Slider::new(&mut self.selection_feather, 0..=100).suffix("%"))
                            .on_hover_text(tr("feather-help"));
                        if self.tool == Tool::Wand && !wrap_wand {
                            ui.add_space(4.0);
                            self.wand_options(ui);
                        }
                    }
                    Tool::Pan | Tool::Zoom => {}
                }
            }

            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let p = self.palette.primary;
                let color = Color32::from_rgba_unmultiplied(p.0[0], p.0[1], p.0[2], p.0[3]);
                let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 4.0, color);
                ui.painter().rect_stroke(
                    rect,
                    4.0,
                    egui::Stroke::new(1.0_f32, self.settings.theme.border_color()),
                    egui::StrokeKind::Outside,
                );
                ui.label(
                    RichText::new(tr("color-field"))
                        .size(11.5)
                        .color(self.settings.theme.text_muted()),
                );
            });
        });
        if wrap_wand {
            ui.horizontal(|ui| match self.tool {
                Tool::Wand => self.wand_options(ui),
                Tool::Bucket => self.bucket_options(ui),
                Tool::Recolor => self.tolerance_options(ui, "recolor_alpha_wrap"),
                _ => {}
            });
        }
    }

    fn session_commit_buttons(&mut self, ui: &mut egui::Ui) {
        if ui
            .button(tr("apply"))
            .on_hover_text(tr("confirm-edits"))
            .clicked()
        {
            self.commit_active_session();
        }
        if ui
            .button(tr("cancel"))
            .on_hover_text(tr("cancel-edits"))
            .clicked()
        {
            self.cancel_active_session(ui.ctx());
        }
    }

    fn tolerance_options(&mut self, ui: &mut egui::Ui, id_salt: &str) {
        ui.label(
            RichText::new(tr("tolerance-field"))
                .size(12.0)
                .color(self.settings.theme.text_muted()),
        );
        ui.add(egui::Slider::new(&mut self.color_tolerance, 0..=100).suffix("%"));
        egui::ComboBox::from_id_salt(id_salt)
            .width(125.0)
            .selected_text(self.alpha_match.label())
            .show_ui(ui, |ui| {
                for mode in AlphaMatchMode::ALL {
                    ui.selectable_value(&mut self.alpha_match, mode, mode.label());
                }
            });
    }

    fn wand_options(&mut self, ui: &mut egui::Ui) {
        self.tolerance_options(ui, "wand_alpha");
        ui.checkbox(&mut self.wand_contiguous, tr("contiguous-feminine"));
        ui.checkbox(&mut self.wand_composite, tr("sample-composite"));
    }

    fn bucket_options(&mut self, ui: &mut egui::Ui) {
        self.tolerance_options(ui, "bucket_alpha");
        Self::pattern_selector(ui, "bucket_pattern", &mut self.fill_pattern);
        Self::blend_selector(ui, "bucket_blend", &mut self.stroke_blend);
        ui.checkbox(&mut self.bucket_contiguous, tr("contiguous"));
        ui.checkbox(&mut self.bucket_composite, tr("sample-composite"));
    }

    fn pattern_selector(ui: &mut egui::Ui, id_salt: &str, pattern: &mut FillPattern) {
        egui::ComboBox::from_id_salt(id_salt)
            .width(110.0)
            .selected_text(pattern.label())
            .show_ui(ui, |ui| {
                for p in FillPattern::ALL {
                    ui.selectable_value(pattern, p, p.label());
                }
            });
    }

    fn blend_selector(ui: &mut egui::Ui, id_salt: &str, blend: &mut StrokeBlend) {
        egui::ComboBox::from_id_salt(id_salt)
            .width(90.0)
            .selected_text(blend.label())
            .show_ui(ui, |ui| {
                for b in StrokeBlend::ALL {
                    ui.selectable_value(blend, b, b.label());
                }
            });
    }

    fn gradient_controls(
        ui: &mut egui::Ui,
        shape: &mut GradientShape,
        repeat: &mut GradientRepeat,
        mode: &mut GradientChannelMode,
        dither: &mut bool,
        blend: &mut StrokeBlend,
    ) -> bool {
        let mut changed = false;
        let prev_shape = *shape;
        egui::ComboBox::from_id_salt("grad_shape")
            .width(115.0)
            .selected_text(shape.label())
            .show_ui(ui, |ui| {
                for s in GradientShape::ALL {
                    ui.selectable_value(shape, s, s.label());
                }
            });
        changed |= *shape != prev_shape;
        let prev_rep = *repeat;
        egui::ComboBox::from_id_salt("grad_repeat")
            .width(96.0)
            .selected_text(repeat.label())
            .show_ui(ui, |ui| {
                for r in GradientRepeat::ALL {
                    ui.selectable_value(repeat, r, r.label());
                }
            });
        changed |= *repeat != prev_rep;
        let prev_mode = *mode;
        egui::ComboBox::from_id_salt("grad_channel")
            .width(98.0)
            .selected_text(mode.label())
            .show_ui(ui, |ui| {
                for m in GradientChannelMode::ALL {
                    ui.selectable_value(mode, m, m.label());
                }
            });
        changed |= *mode != prev_mode;
        changed |= ui.checkbox(dither, tr("dither")).changed();
        let prev_blend = *blend;
        Self::blend_selector(ui, "grad_blend", blend);
        changed | (*blend != prev_blend)
    }

    fn line_curve_controls(
        ui: &mut egui::Ui,
        kind: &mut CurveKind,
        thickness: &mut u32,
        dash: &mut DashPreset,
        start_cap: &mut LineCapStyle,
        end_cap: &mut LineCapStyle,
        blend: &mut StrokeBlend,
    ) -> bool {
        let mut changed = false;
        let prev_k = *kind;
        egui::ComboBox::from_id_salt("curve_kind")
            .width(100.0)
            .selected_text(kind.label())
            .show_ui(ui, |ui| {
                for k in CurveKind::ALL {
                    ui.selectable_value(kind, k, k.label());
                }
            });
        changed |= *kind != prev_k;
        changed |= ui
            .add(egui::Slider::new(thickness, 1..=MAX_OUTLINE_THICKNESS).suffix(" px"))
            .changed();
        let prev_d = *dash;
        egui::ComboBox::from_id_salt("dash_preset")
            .width(105.0)
            .selected_text(dash.label())
            .show_ui(ui, |ui| {
                for d in DashPreset::ALL {
                    ui.selectable_value(dash, d, d.label());
                }
            });
        changed |= *dash != prev_d;
        let prev_sc = *start_cap;
        egui::ComboBox::from_id_salt("start_cap")
            .width(96.0)
            .selected_text(start_cap.label())
            .show_ui(ui, |ui| {
                for c in LineCapStyle::ALL {
                    ui.selectable_value(start_cap, c, c.label());
                }
            });
        changed |= *start_cap != prev_sc;
        let prev_ec = *end_cap;
        egui::ComboBox::from_id_salt("end_cap")
            .width(96.0)
            .selected_text(end_cap.label())
            .show_ui(ui, |ui| {
                for c in LineCapStyle::ALL {
                    ui.selectable_value(end_cap, c, c.label());
                }
            });
        changed |= *end_cap != prev_ec;
        let prev_b = *blend;
        Self::blend_selector(ui, "line_blend", blend);
        changed | (*blend != prev_b)
    }

    fn shape_controls(
        ui: &mut egui::Ui,
        kind: &mut ShapeKind,
        fill_mode: &mut ShapeFillMode,
        thickness: &mut u32,
        radius: &mut f32,
        pattern: &mut FillPattern,
        blend: &mut StrokeBlend,
    ) -> bool {
        let mut changed = false;
        let prev_k = *kind;
        tool_icons::paint_shape_icon(ui, *kind);
        egui::ComboBox::from_id_salt("shape_kind")
            .width(132.0)
            .selected_text(kind.label())
            .show_ui(ui, |ui| {
                for fam in ShapeFamily::ALL {
                    ui.label(RichText::new(fam.label()).strong().size(11.0));
                    for k in ShapeKind::ALL.iter().copied().filter(|k| k.family() == fam) {
                        ui.selectable_value(kind, k, k.label());
                    }
                    ui.separator();
                }
            });
        changed |= *kind != prev_k;
        let prev_m = *fill_mode;
        egui::ComboBox::from_id_salt("shape_fill_mode")
            .width(120.0)
            .selected_text(fill_mode.label())
            .show_ui(ui, |ui| {
                for m in ShapeFillMode::ALL {
                    ui.selectable_value(fill_mode, m, m.label());
                }
            });
        changed |= *fill_mode != prev_m;
        changed |= ui
            .add(egui::Slider::new(thickness, 1..=MAX_OUTLINE_THICKNESS).suffix(" px"))
            .changed();
        if *kind == ShapeKind::RoundedRect {
            changed |= ui
                .add(egui::DragValue::new(radius).range(0.0..=120.0).prefix("R "))
                .changed();
        }
        let prev_p = *pattern;
        Self::pattern_selector(ui, "shape_pattern", pattern);
        changed |= *pattern != prev_p;
        let prev_b = *blend;
        Self::blend_selector(ui, "shape_blend", blend);
        changed | (*blend != prev_b)
    }

    #[allow(clippy::too_many_arguments)]
    fn text_controls(
        ui: &mut egui::Ui,
        families: &[String],
        family: &mut String,
        size_px: &mut f32,
        bold: &mut bool,
        italic: &mut bool,
        underline: &mut bool,
        strike: &mut bool,
        align: &mut TextAlign,
        blend: &mut StrokeBlend,
    ) -> bool {
        let mut changed = false;
        let prev_f = family.clone();
        egui::ComboBox::from_id_salt("text_family")
            .width(115.0)
            .selected_text(text_tool::family_label(family))
            .show_ui(ui, |ui| {
                for fam in families {
                    ui.selectable_value(family, fam.clone(), text_tool::family_label(fam));
                }
            });
        changed |= *family != prev_f;
        changed |= ui
            .add(
                egui::DragValue::new(size_px)
                    .range(6.0..=240.0)
                    .suffix(" px"),
            )
            .changed();
        for (flag, label, tooltip) in [
            (bold, "B", "text-bold"),
            (italic, "I", "text-italic"),
            (underline, "U", "text-underline"),
            (strike, "S", "text-strikethrough"),
        ] {
            if ui
                .selectable_label(*flag, label)
                .on_hover_text(tr(tooltip))
                .clicked()
            {
                *flag = !*flag;
                changed = true;
            }
        }
        let prev_a = *align;
        egui::ComboBox::from_id_salt("text_align")
            .width(82.0)
            .selected_text(align.label())
            .show_ui(ui, |ui| {
                for a in TextAlign::ALL {
                    ui.selectable_value(align, a, a.label());
                }
            });
        changed |= *align != prev_a;
        let prev_b = *blend;
        Self::blend_selector(ui, "text_blend", blend);
        changed | (*blend != prev_b)
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
                .id_salt("documentos_abertos")
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
                            let drag_resp = ui.interact(
                                tab_resp.rect,
                                ui.id().with(("tab_dnd", index)),
                                egui::Sense::drag(),
                            );
                            drag_resp.dnd_set_drag_payload(index);
                            if let Some(from) = drag_resp.dnd_release_payload::<usize>() {
                                reorder = Some((*from, index));
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
            if tool_icons::move_button(ui, self.settings.theme, self.tab().transform.is_some())
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
            let inside = popup_rect.contains(pos)
                || ribbon.chevron_rect.contains(pos)
                || ribbon.bar_rect.contains(pos)
                || ribbon.preview_rect.contains(pos);
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
                    .set_file_name("paleta.txt")
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
                    egui::ComboBox::from_id_salt("layer_blend_mode")
                        .width(blend_width)
                        .selected_text(i18n::blend_name(blend))
                        .show_ui(ui, |ui| {
                            for mode in 0..openpaintr::BLEND_MODES.len() as u8 {
                                ui.selectable_value(&mut blend, mode, i18n::blend_name(mode));
                            }
                        });
                    if blend != meta.blend {
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
                        .add(
                            egui::Slider::new(&mut opacity, 0..=255)
                                .custom_formatter(|value, _| {
                                    format!("{:.0}%", value * 100.0 / 255.0)
                                })
                                .custom_parser(|text| {
                                    text.trim_end_matches('%')
                                        .parse::<f64>()
                                        .ok()
                                        .map(|value| value * 255.0 / 100.0)
                                }),
                        )
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
                            let pill = egui::Frame::new()
                                .fill(if active {
                                    theme.pill_active_fill()
                                } else {
                                    Color32::TRANSPARENT
                                })
                                .corner_radius(6)
                                .inner_margin(egui::Margin::symmetric(5, 3));
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
                                            self.finish_opacity();
                                            let meta_before =
                                                self.tab().editor.layer_meta(layer).unwrap();
                                            let result = self
                                                .tab_mut()
                                                .editor
                                                .set_properties(layer, (!visible, opacity));
                                            if result.is_ok() {
                                                self.tab_mut().editor.record_layer_meta(
                                                    layer,
                                                    meta_before,
                                                    "visibility",
                                                );
                                                self.tab_mut().thumbs_dirty = true;
                                            }
                                            self.report(result, "");
                                        }
                                        if let Some((tid, tsz)) = thumb {
                                            let aspect = (tsz.x / tsz.y.max(1.0)).clamp(0.5, 1.5);
                                            let sz = vec2(22.0 * aspect, 22.0);
                                            let (rect, _) =
                                                ui.allocate_exact_size(sz, egui::Sense::hover());
                                            let p = ui.painter_at(rect);
                                            p.image(
                                                self.checker.id(),
                                                rect,
                                                Rect::from_min_max(
                                                    pos2(0.0, 0.0),
                                                    pos2(sz.x / 10.0, sz.y / 10.0),
                                                ),
                                                Color32::WHITE,
                                            );
                                            p.image(
                                                tid,
                                                rect,
                                                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                                                Color32::WHITE,
                                            );
                                            p.rect_stroke(
                                                rect,
                                                2.0,
                                                egui::Stroke::new(0.8_f32, theme.border_color()),
                                                egui::StrokeKind::Outside,
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
                            let drag = ui.interact(
                                row_resp.rect,
                                ui.id().with(("layer_dnd", layer)),
                                egui::Sense::drag(),
                            );
                            drag.dnd_set_drag_payload(layer);
                            if let Some(from) = drag.dnd_release_payload::<usize>()
                                && *from != layer
                            {
                                move_pair = Some((*from, layer));
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
                            let pill = egui::Frame::new()
                                .fill(if active {
                                    theme.pill_active_fill()
                                } else {
                                    Color32::TRANSPARENT
                                })
                                .corner_radius(5)
                                .inner_margin(egui::Margin::symmetric(6, 3));
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
            match self.tab_mut().editor.jump_to_state(target) {
                Ok(changed) => {
                    if changed {
                        self.tab_mut().thumbs_dirty = true;
                    }
                }
                Err(error) => self.report(Err(error), ""),
            }
        }
    }

    fn combine_selector(&mut self, ui: &mut egui::Ui) {
        egui::ComboBox::from_id_salt("selection_combine")
            .selected_text(self.selection_combine.label())
            .show_ui(ui, |ui| {
                for mode in Combine::ALL {
                    ui.selectable_value(&mut self.selection_combine, mode, mode.label());
                }
            });
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
        let composite = self
            .wand_composite
            .then(|| tab.editor.document.composite())
            .transpose()?;
        Selection::wand(
            size,
            seed,
            self.color_tolerance,
            self.alpha_match,
            contiguous,
            |(x, y)| {
                if let Some(composite) = &composite {
                    composite[y as usize * size.0 as usize + x as usize]
                } else {
                    tab.editor
                        .document
                        .pixel(tab.editor.selected, x, y)
                        .unwrap_or_default()
                }
            },
        )
    }

    pub fn has_active_session(&self) -> bool {
        let tab = self.tab();
        tab.transform.is_some()
            || tab.gradient.is_some()
            || tab.line_curve.is_some()
            || tab.shape_session.is_some()
            || tab.text_session.is_some()
            || tab.canvas_resize.is_some()
    }

    pub fn commit_active_session(&mut self) {
        self.commit_active_transform();
        if let Some(cr) = self.tab_mut().canvas_resize.take() {
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
        let size = self.tab().editor.document.dimensions();
        let aa_mode = self.settings.antialiasing;
        let (preview, label) = {
            let tab = &mut self.tabs[self.active_tab];
            tab.session_creation = None;
            let layer = tab.editor.selected;
            let selection = tab.editor.selection.as_ref();
            let src = tab
                .editor
                .document
                .layer_pixels(layer)
                .expect("camada ativa");
            if let Some(grad) = tab.gradient.take() {
                (Some(grad.render_preview(src, size, selection)), "gradient")
            } else if let Some(lc) = tab.line_curve.take() {
                (
                    Some(lc.render_preview(src, size, selection, aa_mode)),
                    "line-curve",
                )
            } else if let Some(shape) = tab.shape_session.take() {
                (
                    Some(shape.render_preview(src, size, selection, aa_mode)),
                    "shape",
                )
            } else if let Some(mut text) = tab.text_session.take() {
                let empty = text.text.trim().is_empty();
                let buf = (!empty)
                    .then(|| text.render_preview(&mut self.font_catalog, src, size, selection));
                (buf, "text")
            } else {
                (None, "")
            }
        };
        if let Some(after) = preview {
            match self.tab_mut().editor.commit_layer_preview(after, label) {
                Ok(changed) => self.tab_mut().thumbs_dirty |= changed,
                Err(error) => self.report(Err(error), ""),
            }
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
            let tab = self.tab_mut();
            tab.editor.finish_stroke();
            tab.selection_gesture = None;
            tab.selection_path.clear();
            tab.stroke_gesture = None;
            tab.zoom_drag = None;
            tab.pan_gesture = None;
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
        let (primary, secondary) =
            if color == self.palette.secondary && self.palette.primary != self.palette.secondary {
                (self.palette.secondary, self.palette.primary)
            } else {
                (color, self.palette.secondary)
            };
        let pattern = self.fill_pattern;
        let blend = self.stroke_blend;
        let tolerance = self.color_tolerance;
        let alpha_mode = self.alpha_match;
        let composite = self.bucket_composite;
        let aa_mode = self.settings.antialiasing;
        match self.tab_mut().editor.bucket_fill(
            seed, primary, secondary, pattern, blend, tolerance, alpha_mode, contiguous, composite,
            aa_mode,
        ) {
            Ok(changed) => self.tab_mut().thumbs_dirty |= changed,
            Err(error) => self.report(Err(error), ""),
        }
    }

    fn paint_selection(&mut self, erase: bool) {
        self.finish_opacity();
        let (color, label) = if erase {
            (Pixel::default(), "erase-selection")
        } else {
            (self.palette.primary, "fill-selection")
        };
        match self.tab_mut().editor.fill_selection(color, label) {
            Ok(changed) => {
                if changed {
                    self.tab_mut().thumbs_dirty = true;
                }
            }
            Err(error) => self.report(Err(error), ""),
        }
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
        let transform_preview = tab.transform.as_ref().is_some_and(|t| {
            t.floating.is_some()
                || (t.target != TransformTarget::Mask
                    && (!t.affine.is_identity() || t.clone_source))
        });
        let preview_active = transform_preview
            || tab.gradient.is_some()
            || tab.line_curve.is_some()
            || tab.shape_session.is_some()
            || tab.text_session.is_some();

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
                    let colors = pixels
                        .into_iter()
                        .map(|Pixel([r, g, b, a])| Color32::from_rgba_unmultiplied(r, g, b, a))
                        .collect();
                    let sub = ColorImage::new([rw, rh], colors);
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
                .expect("camada ativa");
            let sel = tab.editor.selection.as_ref();
            let mut preview = if transform_preview {
                let transform = tab.transform.as_ref().unwrap();
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
            } else if let Some(grad) = &tab.gradient {
                grad.render_preview(src, (width, height), sel)
            } else if let Some(lc) = &tab.line_curve {
                lc.render_preview(src, (width, height), sel, aa_mode)
            } else if let Some(shape) = &tab.shape_session {
                shape.render_preview(src, (width, height), sel, aa_mode)
            } else if let Some(text) = tab.text_session.as_mut() {
                text.render_preview(font_catalog, src, (width, height), sel)
            } else {
                src.to_vec()
            };
            tab.editor.document.swap_layer_pixels(layer, &mut preview)?;
            let composite = tab.editor.document.composite();
            tab.editor.document.swap_layer_pixels(layer, &mut preview)?;
            composite
        })();
        match composite {
            Ok(pixels) => {
                let colors = pixels
                    .into_iter()
                    .map(|Pixel([r, g, b, a])| Color32::from_rgba_unmultiplied(r, g, b, a))
                    .collect();
                let image = ColorImage::new([width as usize, height as usize], colors);
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

    fn adjust_stroke_size(&mut self, delta: i32) -> bool {
        let adjust = |size: u32, max| size.saturating_add_signed(delta).clamp(1, max);
        if self.has_active_session() {
            let tab = self.tab_mut();
            let size = if let Some(line) = &mut tab.line_curve {
                &mut line.thickness
            } else if let Some(shape) = &mut tab.shape_session {
                &mut shape.thickness
            } else {
                return false;
            };
            let next = adjust(*size, MAX_OUTLINE_THICKNESS);
            if *size == next {
                return false;
            }
            *size = next;
            tab.editor.mark_full_refresh();
            self.shape_thickness = next;
            return true;
        }
        let (size, max) = match self.tool {
            Tool::Brush | Tool::Eraser | Tool::CloneStamp | Tool::Recolor => {
                (&mut self.diameter, MAX_BRUSH_DIAMETER)
            }
            Tool::Pencil => (&mut self.pencil_diameter, MAX_BRUSH_DIAMETER),
            Tool::LineCurve | Tool::DrawShape => (&mut self.shape_thickness, MAX_OUTLINE_THICKNESS),
            _ => return false,
        };
        let next = adjust(*size, max);
        let changed = *size != next;
        *size = next;
        changed
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
            let zoom = ((usable.x - padding) / region.x)
                .min((usable.y - padding) / region.y)
                .clamp(0.01, 32.0);
            let tab = self.tab_mut();
            tab.zoom = Some(zoom);
            let scaled = pixels * zoom;
            let space = canvas_space(scaled);
            let center = vec2(min.0 as f32, min.1 as f32) + region * 0.5;
            tab.view.scroll_to = Some((space - scaled) * 0.5 + center * zoom - available * 0.5);
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
                    let next =
                        (current * 1.0025_f32.powf(magnify).clamp(0.5, 2.0)).clamp(0.01, 32.0);
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
        if let Some(cr) = &self.tab().canvas_resize {
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
            let hit_rect = if rulers {
                rect.intersect(Rect::from_min_max(guide_margin, ui.clip_rect().max))
            } else {
                rect
            };
            let active_session = self.has_active_session();
            let background = ui.interact(
                Rect::from_min_max(guide_margin, ui.clip_rect().max),
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
            painter.image(
                self.checker.id(),
                rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(pixels.x / 16.0, pixels.y / 16.0)),
                Color32::WHITE,
            );
            if let Some(image) = &self.tab().image {
                painter.image(
                    image.id(),
                    rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            let pointer_position = response.hover_pos().map(|pointer| {
                let pixel = (pointer - rect.min) / zoom;
                pos2(pixel.x, pixel.y)
            });
            if self.tab().view.pointer != pointer_position {
                self.tab_mut().view.pointer = pointer_position;
                ui.ctx().request_repaint();
            }

            let mut corner_hovered = false;
            let mut suppress_corner_click = false;
            if !modal_open {
                let transform_outcome = if let Some(mut transform) = self.tab_mut().transform.take()
                {
                    let outcome = transform.handles(rect, zoom, &self.shortcuts.mouse, ui);
                    if outcome == TransformOutcome::Commit {
                        self.commit_transform(transform);
                    } else {
                        self.tab_mut().transform = Some(transform);
                    }
                    Some(outcome)
                } else {
                    None
                };
                if let Some(outcome) = transform_outcome {
                    match outcome {
                        TransformOutcome::Changed => {
                            self.tab_mut().editor.mark_full_refresh();
                            ui.ctx().request_repaint();
                        }
                        TransformOutcome::Commit => {
                            ui.ctx().request_repaint();
                        }
                        TransformOutcome::None => {}
                    }
                    let dpi = self.tab().editor.document.resolution_dpi();
                    self.tab().view.guides(ui, rect, zoom, dpi);
                    return;
                }

                if self.tab().canvas_resize.is_some() {
                    let limit = ui.ctx().input(|i| i.max_texture_side).clamp(1, 8192) as u32;
                    let orig = self.tab().canvas_resize.as_ref().unwrap().original;
                    let mut preview_size = self.tab().canvas_resize.as_ref().unwrap().preview_size;
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
                    if self
                        .tab()
                        .canvas_resize
                        .as_ref()
                        .unwrap()
                        .dragging
                        .is_none()
                        && (drag_resp.drag_started()
                            || ui.input(|i| i.pointer.button_pressed(PointerButton::Primary)))
                        && let Some(ptr) = ui
                            .input(|i| i.pointer.interact_pos())
                            .filter(|p| ui.clip_rect().contains(*p))
                        && let Some(cr) = self.tab_mut().canvas_resize.as_mut()
                    {
                        cr.dragging = Some(handle_hit(ptr));
                    }
                    let active_drag = self.tab().canvas_resize.as_ref().unwrap().dragging;
                    if let Some(kind) = active_drag {
                        ui.ctx().set_cursor_icon(match kind {
                            CanvasResizeDrag::RightEdge => egui::CursorIcon::ResizeHorizontal,
                            CanvasResizeDrag::BottomEdge => egui::CursorIcon::ResizeVertical,
                            CanvasResizeDrag::BottomRight => egui::CursorIcon::ResizeNwSe,
                        });
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
                            let doc_px = (pos - rect.min) / zoom;
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
                            if let Some(cr) = self.tab_mut().canvas_resize.as_mut() {
                                cr.preview_size = preview_size;
                            }
                            preview_rect = Rect::from_min_size(
                                rect.min,
                                vec2(preview_size.0 as f32, preview_size.1 as f32) * zoom,
                            );
                            ui.ctx().request_repaint();
                        }
                        if (!down || released)
                            && let Some(cr) = self.tab_mut().canvas_resize.as_mut()
                        {
                            cr.dragging = None;
                        }
                    } else if let Some(ptr) = ui.input(|i| i.pointer.hover_pos()) {
                        let cursor = match handle_hit(ptr) {
                            CanvasResizeDrag::RightEdge => egui::CursorIcon::ResizeHorizontal,
                            CanvasResizeDrag::BottomEdge => egui::CursorIcon::ResizeVertical,
                            CanvasResizeDrag::BottomRight => egui::CursorIcon::ResizeNwSe,
                        };
                        if ui.clip_rect().contains(ptr) {
                            ui.ctx().set_cursor_icon(cursor);
                        }
                    }

                    let overlay = ui.painter();
                    let paint_checker_strip = |strip: Rect| {
                        if strip.is_positive() {
                            overlay.image(
                                self.checker.id(),
                                strip,
                                Rect::from_min_max(
                                    pos2(0.0, 0.0),
                                    pos2(
                                        (strip.width() / (16.0 * zoom)).max(0.25),
                                        (strip.height() / (16.0 * zoom)).max(0.25),
                                    ),
                                ),
                                Color32::WHITE,
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

                if self.tab().gradient.is_some()
                    || self.tab().line_curve.is_some()
                    || self.tab().shape_session.is_some()
                    || self.tab().text_session.is_some()
                {
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
                            let cur =
                                vec2((pos.x - rect.min.x) / zoom, (pos.y - rect.min.y) / zoom);
                            let tab = self.tab_mut();
                            if let Some(g) = tab.gradient.as_mut() {
                                g.end = snap_if(anchor, cur, modifiers.shift);
                            } else if let Some(lc) = tab.line_curve.as_mut() {
                                lc.set_end(anchor, cur, modifiers.shift);
                            } else if let Some(s) = tab.shape_session.as_mut() {
                                s.set_creation_bounds(anchor, cur, modifiers.shift);
                            }
                            tab.editor.mark_full_refresh();
                            ui.ctx().request_repaint();
                        }
                        if !down || released {
                            self.tab_mut().session_creation = None;
                            ui.ctx().request_repaint();
                        }
                    } else {
                        let outcome = if let Some(mut g) = self.tab_mut().gradient.take() {
                            let o = g.handles(rect, zoom, ui);
                            self.tab_mut().gradient = Some(g);
                            o
                        } else if let Some(mut lc) = self.tab_mut().line_curve.take() {
                            let o = lc.handles(rect, zoom, ui);
                            self.tab_mut().line_curve = Some(lc);
                            o
                        } else if let Some(mut s) = self.tab_mut().shape_session.take() {
                            let o = s.handles(rect, zoom, ui);
                            self.tab_mut().shape_session = Some(s);
                            o
                        } else if let Some(mut t) = self.tab_mut().text_session.take() {
                            let o = t.handles(rect, zoom, ui);
                            self.tab_mut().text_session = Some(t);
                            o
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
                let trigger_corner_resize = if !self.has_active_session() {
                    ui.ctx().input(|input| {
                        let pos = input.pointer.interact_pos()?;
                        if !corner_zone.contains(pos) {
                            return None;
                        }
                        BUTTONS.into_iter().find_map(|btn| {
                            (input.pointer.button_double_clicked(btn)
                                && self.shortcuts.mouse.resolve(
                                    Scope::CanvasCorner,
                                    Trigger::Double(btn),
                                    input.modifiers,
                                    |k| input.key_down(k),
                                ) == Some(MouseAction::CanvasCornerResize))
                            .then_some(input.modifiers.ctrl)
                        })
                    })
                } else {
                    None
                };
                if let Some(ctrl_down) = trigger_corner_resize {
                    self.finish_opacity();
                    let tab = self.tab_mut();
                    tab.editor.finish_stroke();
                    tab.selection_gesture = None;
                    tab.selection_path.clear();
                    tab.stroke_gesture = None;
                    tab.zoom_drag = None;
                    tab.pan_gesture = None;
                    tab.canvas_resize = Some(CanvasResizeSession {
                        original: (width, height),
                        preview_size: (width, height),
                        dragging: None,
                        prev_ctrl: ctrl_down,
                    });
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
                    let pixel = (pos - rect.min) / zoom;
                    let outside = !rect.contains(pos)
                        || pixel.x >= pixels.x
                        || pixel.y >= pixels.y
                        || selection.coverage_u8((pixel.x as u32, pixel.y as u32)) == 0;
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
                } else {
                    ui.ctx().input(|input| {
                        if let Some(gesture) = &self.tab().pan_gesture {
                            return Some(gesture.button);
                        }
                        if response.hovered() || response.is_pointer_button_down_on() {
                            for btn in BUTTONS {
                                if input.pointer.button_pressed(btn)
                                    || input.pointer.button_down(btn)
                                {
                                    let is_canvas_pan = self.shortcuts.mouse.resolve(
                                        Scope::Canvas,
                                        Trigger::Button(btn),
                                        input.modifiers,
                                        |k| input.key_down(k),
                                    ) == Some(MouseAction::Pan);
                                    let is_hand_pan = self.tool == Tool::Pan
                                        && self.shortcuts.mouse.resolve(
                                            Scope::Hand,
                                            Trigger::Button(btn),
                                            input.modifiers,
                                            |k| input.key_down(k),
                                        ) == Some(MouseAction::Hand);
                                    if is_canvas_pan || is_hand_pan {
                                        return Some(btn);
                                    }
                                }
                            }
                        }
                        None
                    })
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
                let temp_zoom_click = ui.ctx().input(|input| {
                    if !(response.hovered() || response.is_pointer_button_down_on()) {
                        return None;
                    }
                    for btn in BUTTONS {
                        if input.pointer.button_pressed(btn) {
                            match self.shortcuts.mouse.resolve(
                                Scope::Canvas,
                                Trigger::Button(btn),
                                input.modifiers,
                                |k| input.key_down(k),
                            ) {
                                Some(MouseAction::TempZoomIn) => return Some(1.25_f32),
                                Some(MouseAction::TempZoomOut) => return Some(0.8_f32),
                                _ => {}
                            }
                        }
                    }
                    None
                });
                if let Some(factor) = temp_zoom_click {
                    let next = (zoom * factor).clamp(0.01, 32.0);
                    let tab = self.tab_mut();
                    tab.zoom = Some(next);
                    if let Some(pos) = response.interact_pointer_pos() {
                        let scaled = pixels * next;
                        let space = canvas_space(scaled);
                        tab.view.scroll_to = Some(
                            (space - scaled) * 0.5 + (pos - rect.min) / zoom * next
                                - available * 0.5,
                        );
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
                                let next = (start_zoom * factor).clamp(0.01, 32.0);
                                let scaled = pixels * next;
                                let space = canvas_space(scaled);
                                let tab = self.tab_mut();
                                tab.zoom = Some(next);
                                tab.view.scroll_to = Some(
                                    (space - scaled) * 0.5
                                        + (press_pos - rect.min) / start_zoom * next
                                        - available * 0.5,
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
                                        .min((usable.y - padding) / box_h)
                                        .clamp(0.01, 32.0);
                                    let center_doc = (p0 + p1) * 0.5;
                                    let scaled = pixels * next;
                                    let space = canvas_space(scaled);
                                    let tab = self.tab_mut();
                                    tab.zoom = Some(next);
                                    tab.view.scroll_to = Some(
                                        (space - scaled) * 0.5 + center_doc * next
                                            - available * 0.5,
                                    );
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
                                    Some(MouseAction::ZoomOut) => Some(0.8_f32),
                                    Some(MouseAction::ZoomIn) => Some(1.25_f32),
                                    _ => None,
                                };
                                if let Some(factor) = factor {
                                    let next = (start_zoom * factor).clamp(0.01, 32.0);
                                    let scaled = pixels * next;
                                    let space = canvas_space(scaled);
                                    let tab = self.tab_mut();
                                    tab.zoom = Some(next);
                                    tab.view.scroll_to = Some(
                                        (space - scaled) * 0.5
                                            + (press_pos - rect.min) / start_zoom * next
                                            - available * 0.5,
                                    );
                                    ui.ctx().request_repaint();
                                }
                            }
                            self.tab_mut().zoom_drag = None;
                        }
                    } else if response.hovered() || response.is_pointer_button_down_on() {
                        let started = ui.ctx().input(|input| {
                            let pos = input.pointer.interact_pos()?;
                            for btn in BUTTONS {
                                if input.pointer.button_pressed(btn) {
                                    match self.shortcuts.mouse.resolve(
                                        Scope::ZoomDrag,
                                        Trigger::Button(btn),
                                        input.modifiers,
                                        |k| input.key_down(k),
                                    ) {
                                        Some(MouseAction::ZoomScrubby) => {
                                            return Some((btn, pos, true));
                                        }
                                        Some(MouseAction::ZoomMarquee) => {
                                            return Some((btn, pos, false));
                                        }
                                        _ => {}
                                    }
                                    if let Some(act) = self.shortcuts.mouse.resolve(
                                        Scope::Zoom,
                                        Trigger::Button(btn),
                                        input.modifiers,
                                        |k| input.key_down(k),
                                    ) && matches!(
                                        act,
                                        MouseAction::ZoomIn | MouseAction::ZoomOut
                                    ) {
                                        return Some((btn, pos, false));
                                    }
                                }
                            }
                            None
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
                let pixel_at = |position: egui::Pos2| (position - rect.min) / zoom;
                let document_point = |position: egui::Pos2| {
                    let pixel = pixel_at(position);
                    (pixel.x >= 0.0 && pixel.y >= 0.0 && pixel.x < pixels.x && pixel.y < pixels.y)
                        .then_some((pixel.x as u32, pixel.y as u32))
                };
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
                        let px = if sample_comp {
                            self.tab()
                                .editor
                                .document
                                .composite()
                                .ok()
                                .and_then(|c| {
                                    c.get(point.1 as usize * width as usize + point.0 as usize)
                                        .copied()
                                })
                                .unwrap_or_default()
                        } else {
                            let layer = self.tab().editor.selected;
                            self.tab()
                                .editor
                                .document
                                .pixel(layer, point.0, point.1)
                                .unwrap_or_default()
                        };
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
                                    (self.palette.primary, self.bucket_contiguous)
                                }
                                MouseAction::BucketSecondary => {
                                    (self.palette.secondary, self.bucket_contiguous)
                                }
                                MouseAction::BucketGlobalPrimary => (self.palette.primary, false),
                                MouseAction::BucketGlobalSecondary => {
                                    (self.palette.secondary, false)
                                }
                                _ => (self.palette.primary, self.bucket_contiguous),
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
                        let pressed = ui.ctx().input(|input| {
                            BUTTONS.into_iter().find_map(|btn| {
                                input.pointer.button_pressed(btn).then(|| {
                                    self.shortcuts
                                        .mouse
                                        .resolve(
                                            Scope::Selection,
                                            Trigger::Button(btn),
                                            input.modifiers,
                                            |k| input.key_down(k),
                                        )
                                        .map(|act| (btn, act))
                                })?
                            })
                        });
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
                                .map_or(vec2(start_pt.0 as f32, start_pt.1 as f32), pixel_at);
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
                            let pixel = pixel_at(position);
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
                                                rect.min
                                                    + zoom * vec2(x as f32 + 0.5, y as f32 + 0.5)
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
                        let pressed = ui.ctx().input(|input| {
                            BUTTONS.into_iter().find_map(|btn| {
                                input.pointer.button_pressed(btn).then(|| {
                                    self.shortcuts
                                        .mouse
                                        .resolve(
                                            scope,
                                            Trigger::Button(btn),
                                            input.modifiers,
                                            |k| input.key_down(k),
                                        )
                                        .map(|act| (btn, act))
                                })?
                            })
                        });
                        if let Some((btn, action)) = pressed {
                            self.finish_opacity();
                            let world_pt = pixel_at(pos);
                            match (self.tool, action) {
                                (
                                    Tool::Gradient,
                                    MouseAction::GradientPrimary | MouseAction::GradientSecondary,
                                ) => {
                                    let swapped = action == MouseAction::GradientSecondary;
                                    let session = GradientSession::new(
                                        world_pt,
                                        world_pt,
                                        self.palette.primary,
                                        self.palette.secondary,
                                        swapped,
                                        self.gradient_shape,
                                        self.gradient_repeat,
                                        self.gradient_mode,
                                        self.gradient_dither,
                                        self.stroke_blend,
                                    );
                                    let tab = self.tab_mut();
                                    tab.gradient = Some(session);
                                    tab.session_creation = Some((btn, world_pt));
                                    tab.editor.mark_full_refresh();
                                }
                                (
                                    Tool::LineCurve,
                                    MouseAction::LinePrimary | MouseAction::LineSecondary,
                                ) => {
                                    let color = if action == MouseAction::LineSecondary {
                                        self.palette.secondary
                                    } else {
                                        self.palette.primary
                                    };
                                    let session = LineCurveSession::new(
                                        world_pt,
                                        world_pt,
                                        self.curve_kind,
                                        self.shape_thickness,
                                        self.dash_preset,
                                        self.line_start_cap,
                                        self.line_end_cap,
                                        color,
                                        self.stroke_blend,
                                    );
                                    let tab = self.tab_mut();
                                    tab.line_curve = Some(session);
                                    tab.session_creation = Some((btn, world_pt));
                                    tab.editor.mark_full_refresh();
                                }
                                (
                                    Tool::DrawShape,
                                    MouseAction::ShapePrimary | MouseAction::ShapeSecondary,
                                ) => {
                                    let (outline, fill) = if action == MouseAction::ShapeSecondary {
                                        (self.palette.secondary, self.palette.primary)
                                    } else {
                                        (self.palette.primary, self.palette.secondary)
                                    };
                                    let session = ShapeSession::new(
                                        self.shape_kind,
                                        world_pt,
                                        world_pt,
                                        self.shape_fill_mode,
                                        self.shape_thickness,
                                        self.shape_corner_radius,
                                        self.fill_pattern,
                                        outline,
                                        fill,
                                        self.stroke_blend,
                                    );
                                    let tab = self.tab_mut();
                                    tab.shape_session = Some(session);
                                    tab.session_creation = Some((btn, world_pt));
                                    tab.editor.mark_full_refresh();
                                }
                                (Tool::Text, MouseAction::TextPlace) => {
                                    let session = TextSession::new(
                                        world_pt,
                                        self.text_family.clone(),
                                        self.text_size,
                                        self.text_bold,
                                        self.text_italic,
                                        self.text_underline,
                                        self.text_strikethrough,
                                        self.text_align,
                                        self.palette.primary,
                                        self.stroke_blend,
                                    );
                                    let tab = self.tab_mut();
                                    tab.text_session = Some(session);
                                    tab.session_creation = None;
                                    tab.editor.mark_full_refresh();
                                }
                                _ => {}
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
                        let sc = rect.min + src_pt * zoom;
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
                        let pressed = ui.ctx().input(|input| {
                            BUTTONS.into_iter().find_map(|btn| {
                                input.pointer.button_pressed(btn).then(|| {
                                    self.shortcuts
                                        .mouse
                                        .resolve(
                                            scope,
                                            Trigger::Button(btn),
                                            input.modifiers,
                                            |k| input.key_down(k),
                                        )
                                        .map(|act| (btn, act))
                                })?
                            })
                        });
                        if let Some((btn, action)) = pressed {
                            match action {
                                MouseAction::BrushOptions | MouseAction::EraserOptions => {
                                    self.tool_size_popup = true;
                                }
                                MouseAction::BrushSampleColor => {
                                    let layer = self.tab().editor.selected;
                                    let px = self
                                        .tab()
                                        .editor
                                        .document
                                        .pixel(layer, start_pt.0, start_pt.1)
                                        .unwrap_or_default();
                                    if btn == PointerButton::Secondary {
                                        self.palette.set_secondary(px);
                                    } else {
                                        self.palette.set_primary(px);
                                    }
                                }
                                MouseAction::RecolorSample => {
                                    let layer = self.tab().editor.selected;
                                    let px = self
                                        .tab()
                                        .editor
                                        .document
                                        .pixel(layer, start_pt.0, start_pt.1)
                                        .unwrap_or_default();
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
                                            let primary = if action == MouseAction::PencilSecondary
                                            {
                                                self.palette.secondary
                                            } else {
                                                self.palette.primary
                                            };
                                            Some((
                                                StrokeSpec::Paint {
                                                    primary,
                                                    secondary: self.palette.secondary,
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
                                            let (primary, secondary) =
                                                if action == MouseAction::DrawSecondary {
                                                    (self.palette.secondary, self.palette.primary)
                                                } else {
                                                    (self.palette.primary, self.palette.secondary)
                                                };
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
                                            let replacement =
                                                if action == MouseAction::RecolorSecondary {
                                                    self.palette.secondary
                                                } else {
                                                    self.palette.primary
                                                };
                                            let target = self
                                                .recolor_target
                                                .unwrap_or(self.palette.secondary);
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
                                                    .expect("camada ativa")
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
                    let path = dialog.path;
                    let is_project = dialog.is_project;
                    if ok && is_project {
                        self.tab_mut().name = display_name(&path);
                        self.tab_mut().project_path = Some(path.clone());
                        self.tab_mut().editor.mark_saved();
                        self.recent.push(path);
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
                        if preview_meta.name.trim().is_empty() || preview_meta.name.contains('\0') {
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
            let limit = ctx.input(|input| input.max_texture_side).min(8192) as u32;
            let response = modal(ctx, "new").show(ctx, |ui| {
                ui.heading(tr("new-image"));
                ui.horizontal(|ui| {
                    ui.label(tr("width"));
                    ui.add(
                        egui::DragValue::new(&mut width)
                            .range(1..=limit)
                            .suffix(" px"),
                    );
                    ui.label(tr("height"));
                    ui.add(
                        egui::DragValue::new(&mut height)
                            .range(1..=limit)
                            .suffix(" px"),
                    );
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

fn rectangle_outline(rect: Rect) -> [egui::Pos2; 5] {
    [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ]
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

#[cfg(test)]
fn write_pdn_atomically(document: &Document, path: &Path, replace: bool) -> io::Result<()> {
    let mut bytes = Vec::new();
    document.write_pdn(&mut bytes)?;
    atomic_write_bytes(path, &bytes, replace)
}

impl App {
    fn cancel_active_transform(&mut self, ctx: &egui::Context) {
        if let Some(transform) = self.tab_mut().transform.take() {
            if transform.floating.is_some() {
                let _ = self.tab_mut().editor.undo();
            }
            self.tab_mut().editor.mark_full_refresh();
            ctx.request_repaint();
        }
    }

    fn cancel_active_session(&mut self, ctx: &egui::Context) {
        self.cancel_active_transform(ctx);
        let tab = self.tab_mut();
        let had_other = tab.gradient.take().is_some()
            || tab.line_curve.take().is_some()
            || tab.shape_session.take().is_some()
            || tab.text_session.take().is_some()
            || tab.canvas_resize.take().is_some();
        tab.session_creation = None;
        if had_other {
            tab.editor.mark_full_refresh();
            ctx.request_repaint();
        }
    }

    fn ui_frame(&mut self, ctx: &egui::Context) {
        self.locale.with(|| self.localized_frame(ctx));
    }

    fn localized_frame(&mut self, ctx: &egui::Context) {
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
            && self.tab().text_session.is_some()
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
            } else if let Some(text) = self.tab_mut().text_session.as_mut() {
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
                    let mut dx = 0.0_f32;
                    let mut dy = 0.0_f32;
                    if input.key_pressed(egui::Key::ArrowLeft) {
                        dx -= 5.0;
                    }
                    if input.key_pressed(egui::Key::ArrowRight) {
                        dx += 5.0;
                    }
                    if input.key_pressed(egui::Key::ArrowUp) {
                        dy -= 5.0;
                    }
                    if input.key_pressed(egui::Key::ArrowDown) {
                        dy += 5.0;
                    }
                    (dx, dy)
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
            && self.tab().canvas_resize.is_some()
            && !self.has_modal_open()
            && !ctx.wants_keyboard_input()
        {
            let cur_ctrl = ctx.input(|input| input.modifiers.ctrl);
            let prev_ctrl = self.tab().canvas_resize.as_ref().unwrap().prev_ctrl;
            let ctrl_just_pressed = !prev_ctrl && cur_ctrl;
            if let Some(cr) = self.tab_mut().canvas_resize.as_mut() {
                cr.prev_ctrl = cur_ctrl;
            }
            if ctrl_just_pressed || ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
                self.commit_active_session();
                ctx.request_repaint();
            } else {
                let limit = ctx.input(|i| i.max_texture_side).clamp(1, 8192) as i64;
                let (dx, dy) = ctx.input(|input| {
                    let step = if input.modifiers.shift { 10 } else { 1 };
                    let mut dx = 0_i64;
                    let mut dy = 0_i64;
                    if input.key_pressed(egui::Key::ArrowLeft) {
                        dx -= step;
                    }
                    if input.key_pressed(egui::Key::ArrowRight) {
                        dx += step;
                    }
                    if input.key_pressed(egui::Key::ArrowUp) {
                        dy -= step;
                    }
                    if input.key_pressed(egui::Key::ArrowDown) {
                        dy += step;
                    }
                    (dx, dy)
                });
                if (dx != 0 || dy != 0)
                    && let Some(cr) = self.tab_mut().canvas_resize.as_mut()
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
                        10.0
                    } else {
                        1.0
                    };
                    let mut dx = 0.0_f64;
                    let mut dy = 0.0_f64;
                    if input.key_pressed(egui::Key::ArrowLeft) {
                        dx -= step;
                    }
                    if input.key_pressed(egui::Key::ArrowRight) {
                        dx += step;
                    }
                    if input.key_pressed(egui::Key::ArrowUp) {
                        dy -= step;
                    }
                    if input.key_pressed(egui::Key::ArrowDown) {
                        dy += step;
                    }
                    (dx, dy)
                });
                if dx != 0.0 || dy != 0.0 {
                    let tab = self.tab_mut();
                    if let Some(transform) = tab.transform.as_mut() {
                        transform.nudge(dx, dy);
                    } else if let Some(grad) = tab.gradient.as_mut() {
                        grad.nudge(dx as f32, dy as f32);
                    } else if let Some(lc) = tab.line_curve.as_mut() {
                        lc.nudge(dx as f32, dy as f32);
                    } else if let Some(shape) = tab.shape_session.as_mut() {
                        shape.nudge(dx as f32, dy as f32);
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
                if let Some(transform) = &self.tab().transform
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
                        let color = if *error {
                            self.settings.theme.error()
                        } else {
                            self.settings.theme.accent()
                        };
                        ui.add(
                            egui::Label::new(RichText::new(notice).size(12.0).color(color))
                                .truncate(),
                        )
                        .on_hover_text(notice);
                    } else {
                        ui.label(
                            RichText::new(tr("ready"))
                                .size(12.0)
                                .color(self.settings.theme.text_muted()),
                        );
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
                            self.step_zoom(1.25);
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
                            self.step_zoom(0.8);
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
                            ui.label(
                                RichText::new(coords)
                                    .size(12.0)
                                    .color(self.settings.theme.text_muted()),
                            );
                            ui.separator();
                        }
                        ui.label(
                            RichText::new(tab.view.dimensions(&tab.editor.document))
                                .size(12.0)
                                .color(self.settings.theme.text_muted()),
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
                                mesh.vertices.chunks_exact(4).find_map(|quad| {
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

        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, new_document(32, 24)?);
        app.tab_mut().editor.add_layer()?;
        app.tab_mut()
            .editor
            .document
            .set_layer_name(1, "My saved layer")?;
        let shortcut = app.shortcuts.hint(Command::Save);
        for locale in i18n::Locale::all() {
            app.locale = locale;
            ctx.set_fonts(i18n::fonts(locale));
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0))),
                    ..Default::default()
                },
                |ctx| app.ui_frame(ctx),
            );
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
                app.tab().editor.document.layer_name(1),
                Some("My saved layer")
            );
            assert_eq!(app.shortcuts.hint(Command::Save), shortcut);
        }
        assert!(app.tab_mut().editor.undo()?);
        assert_eq!(app.tab().editor.document.layer_count(), 1);
        Ok(())
    }

    #[test]
    fn pdn_edit_undo_and_png_export_work_in_each_language() -> io::Result<()> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pdn/Untitled.pdn");
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
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, Document::new(100, 100)?);
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0));
        let center = pos2(556.0, 440.0);
        let frame = |app: &mut App, events| {
            let input = egui::RawInput {
                screen_rect: Some(rect),
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| app.ui_frame(ctx));
        };
        let click = |app: &mut App| {
            frame(app, vec![egui::Event::PointerMoved(center)]);
            for pressed in [true, false] {
                frame(
                    app,
                    vec![egui::Event::PointerButton {
                        pos: center,
                        button: PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
            }
        };
        let selection = Selection::new(Shape::Rectangle, (0, 0), (4, 4));
        for tool in [
            Tool::Pan,
            Tool::Brush,
            Tool::Wand,
            Tool::Select(Shape::Rectangle),
            Tool::Bucket,
        ] {
            app.activate_tool(tool);
            app.tab_mut()
                .editor
                .set_selection(Some(selection.clone()), "Seleção");
            click(&mut app);
            assert!(app.tab().editor.selection.is_none(), "{}", tool.label());
            assert!(!app.tab().editor.has_unsaved_edits(), "{}", tool.label());
        }

        app.tab_mut()
            .editor
            .set_selection(Some(selection), "Seleção");
        frame(
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(app.tab().editor.selection.is_none());
        app.activate_tool(Tool::Bucket);
        click(&mut app);
        assert_eq!(
            app.tab().editor.document.pixel(0, 0, 0),
            Some(app.palette.primary)
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
        assert!(app.tab().transform.is_some());
        app.open_in_new_tab(Document::new(4, 4)?, "Novo.pdn".into(), None);
        assert_eq!(app.tabs.len(), 2);
        app.switch_tab(0);
        assert!(app.tab().transform.is_some());
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
        assert!(app.tab().transform.is_some());
        app.tab_mut().transform.as_mut().unwrap().affine.shift = (3.0, 2.0);
        app.commit_active_transform();
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
            (egui::Key::Backspace, app.palette.primary),
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
        let mut dialog = SaveOptionsDialog::new_png(path.clone(), false, &doc, &ctx);
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
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, Document::new(32, 24)?);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0))),
            ..Default::default()
        };
        for theme in [Theme::Light, Theme::Dark] {
            app.settings.theme = theme;
            theme.apply(&ctx);
            app.colors_open = false;
            app.history_open = false;
            let _ = ctx.run(input.clone(), |ctx| app.ui_frame(ctx));
            app.colors_open = true;
            app.history_open = true;
            let _ = ctx.run(input.clone(), |ctx| app.ui_frame(ctx));
        }
        Ok(())
    }

    #[test]
    fn transform_keyboard_arrows_nudge_and_enter_commits() -> io::Result<()> {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, Document::new(20, 20)?);
        let red = Pixel([220, 40, 40, 255]);
        app.tab_mut().editor.document.set_pixel(0, 2, 2, red)?;
        app.tab_mut().editor.set_selection(
            Some(Selection::new(Shape::Rectangle, (2, 2), (2, 2))),
            "Seleção retangular",
        );
        app.execute_command(&ctx, Command::MovePixels);
        assert!(app.tab().transform.is_some());

        let send_key = |app: &mut App, key: egui::Key, modifiers: egui::Modifiers| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0))),
                modifiers,
                events: vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| app.ui_frame(ctx));
        };

        send_key(&mut app, egui::Key::ArrowRight, egui::Modifiers::NONE);
        send_key(&mut app, egui::Key::ArrowDown, egui::Modifiers::CTRL);
        assert_eq!(
            app.tab().transform.as_ref().unwrap().affine.shift,
            (1.0, 10.0)
        );
        send_key(&mut app, egui::Key::Enter, egui::Modifiers::NONE);
        assert!(app.tab().transform.is_none());
        assert_eq!(app.tab().editor.document.pixel(0, 3, 12), Some(red));
        assert_eq!(
            app.tab().editor.document.pixel(0, 2, 2),
            Some(Pixel::default())
        );

        app.tab_mut().editor.set_selection(
            Some(Selection::new(Shape::Rectangle, (3, 12), (3, 12))),
            "Seleção retangular",
        );
        app.execute_command(&ctx, Command::MovePixels);
        send_key(&mut app, egui::Key::ArrowRight, egui::Modifiers::NONE);
        send_key(&mut app, egui::Key::Escape, egui::Modifiers::NONE);
        assert!(app.tab().transform.is_none());
        assert_eq!(app.tab().editor.document.pixel(0, 3, 12), Some(red));

        // Primary mouse click on the canvas commits the transform
        app.execute_command(&ctx, Command::MovePixels);
        app.tab_mut().transform.as_mut().unwrap().nudge(2.0, 0.0);
        let center = pos2(640.0, 400.0);
        let idle_frame = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0))),
            events: vec![egui::Event::PointerMoved(center)],
            ..Default::default()
        };
        let _ = ctx.run(idle_frame, |ctx| app.ui_frame(ctx));
        for pressed in [true, false] {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0))),
                events: vec![
                    egui::Event::PointerMoved(center),
                    egui::Event::PointerButton {
                        pos: center,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| app.ui_frame(ctx));
        }
        assert!(app.tab().transform.is_none());
        assert_eq!(app.tab().editor.document.pixel(0, 5, 12), Some(red));
        assert_eq!(
            app.tab().editor.document.pixel(0, 3, 12),
            Some(Pixel::default())
        );

        // Escape cancels floating paste and also reverts the paste selection
        app.tab_mut().editor.set_selection(None, "Limpar");
        app.clipboard.put(clipboard::Image {
            size: (2, 2),
            origin: (0, 0),
            pixels: vec![Pixel([0, 200, 0, 255]); 4],
        });
        app.paste_from_clipboard(false);
        assert!(app.tab().transform.is_some());
        assert!(app.tab().editor.selection.is_some());
        send_key(&mut app, egui::Key::Escape, egui::Modifiers::NONE);
        assert!(app.tab().transform.is_none());
        assert!(app.tab().editor.selection.is_none());
        assert_eq!(
            app.tab().editor.document.pixel(0, 0, 0),
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
        app.bucket_fill((3, 3), app.palette.primary, false);
        assert_eq!(
            app.tab().editor.document.pixel(0, 5, 5),
            Some(app.palette.primary)
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
        app.tab_mut().gradient = Some(GradientSession::new(
            vec2(4.0, 4.0),
            vec2(50.0, 4.0),
            Pixel([200, 40, 40, 255]),
            Pixel([40, 40, 200, 255]),
            false,
            GradientShape::Linear,
            GradientRepeat::None,
            GradientChannelMode::Colors,
            false,
            StrokeBlend::Overwrite,
        ));
        send_events(&mut app, vec![key_evt(egui::Key::Escape)]);
        assert!(app.tab().gradient.is_none());
        assert_eq!(
            app.tab().editor.document.pixel(0, 10, 4),
            Some(Pixel::default())
        );

        app.tab_mut().gradient = Some(GradientSession::new(
            vec2(4.0, 4.0),
            vec2(50.0, 4.0),
            Pixel([200, 40, 40, 255]),
            Pixel([40, 40, 200, 255]),
            false,
            GradientShape::Linear,
            GradientRepeat::None,
            GradientChannelMode::Colors,
            false,
            StrokeBlend::Overwrite,
        ));
        send_events(&mut app, vec![key_evt(egui::Key::Enter)]);
        assert!(app.tab().gradient.is_none());
        assert_ne!(
            app.tab().editor.document.pixel(0, 25, 20),
            Some(Pixel::default())
        );
        assert!(app.tab_mut().editor.undo()?);

        // Line / Curve commits SSAA pixels + undo
        app.activate_tool(Tool::LineCurve);
        app.tab_mut().line_curve = Some(LineCurveSession::new(
            vec2(6.0, 10.0),
            vec2(54.0, 38.0),
            CurveKind::CubicBezier,
            3,
            DashPreset::Solid,
            LineCapStyle::Flat,
            LineCapStyle::ArrowFilled,
            Pixel([20, 20, 20, 255]),
            StrokeBlend::Normal,
        ));
        send_events(&mut app, vec![key_evt(egui::Key::Enter)]);
        assert!(app.tab().line_curve.is_none());
        assert!(app.tab_mut().editor.undo()?);

        // Shape Freeform commits + undo
        let mut freeform = ShapeSession::new(
            ShapeKind::Freeform,
            vec2(8.0, 8.0),
            vec2(40.0, 40.0),
            ShapeFillMode::Both,
            2,
            8.0,
            FillPattern::Solid,
            Pixel([10, 10, 10, 255]),
            Pixel([180, 90, 220, 255]),
            StrokeBlend::Normal,
        );
        freeform.set_creation_bounds(vec2(8.0, 8.0), vec2(40.0, 12.0), false);
        freeform.set_creation_bounds(vec2(8.0, 8.0), vec2(26.0, 40.0), false);
        app.tab_mut().shape_session = Some(freeform);
        send_events(&mut app, vec![key_evt(egui::Key::Enter)]);
        assert!(app.tab().shape_session.is_none());
        assert_ne!(
            app.tab().editor.document.pixel(0, 20, 20),
            Some(Pixel::default())
        );
        assert!(app.tab_mut().editor.undo()?);

        // Text: Event::Text("Oi") + Enter commits SSAA glyphs without triggering tool shortcuts
        app.activate_tool(Tool::Text);
        app.tab_mut().text_session = Some(TextSession::new(
            vec2(4.0, 4.0),
            "Padrão (Sans)".into(),
            28.0,
            false,
            false,
            false,
            false,
            TextAlign::Left,
            Pixel([25, 25, 25, 255]),
            StrokeBlend::Normal,
        ));
        send_events(&mut app, vec![egui::Event::Text("bt".into())]);
        assert_eq!(app.tool, Tool::Text);
        assert_eq!(app.tab().text_session.as_ref().unwrap().text, "bt");
        send_events(&mut app, vec![key_evt(egui::Key::Enter)]);
        assert!(app.tab().text_session.is_none());
        let any_ink = (0..48)
            .flat_map(|y| (0..64).map(move |x| (x, y)))
            .any(|(x, y)| app.tab().editor.document.pixel(0, x, y).unwrap().0[3] > 0);
        assert!(any_ink);
        assert!(app.tab_mut().editor.undo()?);
        Ok(())
    }

    #[test]
    fn wheel_gesture_adjusts_pixel_widths_and_the_active_vector_preview() -> io::Result<()> {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, Document::new(64, 48)?);
        let wheel = |app: &mut App, lines, modifiers| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0))),
                modifiers,
                events: vec![
                    egui::Event::PointerMoved(pos2(600.0, 400.0)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Line,
                        delta: vec2(0.0, lines),
                        modifiers,
                    },
                ],
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| app.ui_frame(ctx));
        };
        let resize = egui::Modifiers::CTRL | egui::Modifiers::SHIFT;
        for tool in [Tool::Brush, Tool::Eraser, Tool::CloneStamp, Tool::Recolor] {
            app.activate_tool(tool);
            app.diameter = 12;
            wheel(&mut app, 1.0, resize);
            assert_eq!(app.diameter, 13, "{}", tool.label());
            wheel(&mut app, -2.0, resize);
            assert_eq!(app.diameter, 11, "{}", tool.label());
        }

        app.activate_tool(Tool::Pencil);
        wheel(&mut app, -1.0, resize);
        assert_eq!(app.pencil_diameter, 1);
        wheel(&mut app, 4.0, resize);
        assert_eq!(app.pencil_diameter, 5);
        assert_eq!(app.diameter, 11);

        app.activate_tool(Tool::LineCurve);
        app.shape_thickness = 9;
        app.tab_mut().line_curve = Some(LineCurveSession::new(
            vec2(10.0, 18.0),
            vec2(40.0, 18.0),
            CurveKind::CubicBezier,
            2,
            DashPreset::Solid,
            LineCapStyle::Flat,
            LineCapStyle::Flat,
            app.palette.primary,
            StrokeBlend::Normal,
        ));
        let zoom = app.tab().zoom;
        wheel(&mut app, 8.0, resize);
        assert_eq!(app.shape_thickness, 10);
        assert_eq!(app.tab().line_curve.as_ref().unwrap().thickness, 10);
        assert_eq!(
            app.tab().editor.document.pixel(0, 20, 21),
            Some(Pixel::default())
        );
        assert_eq!(app.tab().zoom, zoom);
        app.commit_active_session();
        assert_ne!(
            app.tab().editor.document.pixel(0, 20, 21),
            Some(Pixel::default())
        );

        app.activate_tool(Tool::DrawShape);
        app.tab_mut().shape_session = Some(ShapeSession::new(
            ShapeKind::Freeform,
            vec2(10.0, 10.0),
            vec2(40.0, 40.0),
            ShapeFillMode::Both,
            10,
            0.0,
            FillPattern::Solid,
            app.palette.primary,
            app.palette.secondary,
            StrokeBlend::Normal,
        ));
        wheel(&mut app, 100.0, resize);
        assert_eq!(app.shape_thickness, MAX_OUTLINE_THICKNESS);
        assert_eq!(
            app.tab().shape_session.as_ref().unwrap().thickness,
            MAX_OUTLINE_THICKNESS
        );

        app.activate_tool(Tool::Bucket);
        wheel(&mut app, 1.0, resize);
        assert_eq!(app.diameter, 11);
        assert_eq!(app.tab().zoom, zoom);
        wheel(&mut app, 1.0, egui::Modifiers::CTRL);
        assert!(app.tab().zoom > zoom);
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
    fn color_ribbon_updates_hue_and_dropdown_closes_on_escape_or_outside_click() {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, Document::new(32, 24).unwrap());
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0));

        // 1. Hue selection & bi-directional sync with ColorPalette
        app.palette.set_primary(Pixel([0, 0, 0, 255]));
        app.palette.set_primary_hue(1.0 / 3.0); // Pure green
        assert_eq!(app.palette.primary, Pixel([0, 255, 0, 255]));
        app.palette.set_primary(Pixel([0, 0, 255, 255]));
        assert!((app.palette.primary_hsva.h - (2.0 / 3.0)).abs() < 0.02);

        // 2. Canvas layout does not shift when dropdown opens/closes, and Escape / outside click closes it
        let run_events = |app: &mut App, events| {
            let input = egui::RawInput {
                screen_rect: Some(rect),
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| app.ui_frame(ctx));
            ctx.memory(|m| m.layer_id_at(pos2(600.0, 400.0)))
        };
        app.colors_open = false;
        let _ = run_events(&mut app, Vec::new());
        let canvas_closed = ctx.run(
            egui::RawInput {
                screen_rect: Some(rect),
                ..Default::default()
            },
            |ctx| app.ui_frame(ctx),
        );
        let closed_central = ctx.available_rect();

        app.colors_open = true;
        let canvas_open = ctx.run(
            egui::RawInput {
                screen_rect: Some(rect),
                ..Default::default()
            },
            |ctx| app.ui_frame(ctx),
        );
        let open_central = ctx.available_rect();
        assert_eq!(closed_central, open_central);
        let _ = (canvas_closed, canvas_open);
        assert!(app.color_popup_rect.is_some());

        // ESC closes the Color Card dropdown
        run_events(
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(!app.colors_open);
        assert!(app.color_popup_rect.is_none());

        // Pressing outside closes an open Color Card dropdown
        app.colors_open = true;
        run_events(&mut app, Vec::new());
        assert!(app.color_popup_rect.is_some());
        run_events(
            &mut app,
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
        assert!(!app.colors_open);
    }

    #[test]
    fn ctrl_double_click_bottom_right_enters_canvas_resize_and_commits_or_cancels() -> io::Result<()>
    {
        let ctx = egui::Context::default();
        let mut doc = Document::new(100, 80)?;
        let red = Pixel([200, 40, 40, 255]);
        doc.fill_layer(0, red)?;
        let mut app = App::new(&ctx, doc);
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0));
        let mut time = 0.0_f64;
        let mut run = |app: &mut App, events, mods: egui::Modifiers| {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                modifiers: mods,
                time: Some(time),
                events,
                ..Default::default()
            };
            time += 0.05;
            ctx.run(input, |ctx| app.ui_frame(ctx))
        };

        let _ = run(&mut app, Vec::new(), egui::Modifiers::NONE);
        let _ = run(&mut app, Vec::new(), egui::Modifiers::NONE);
        let canvas_rect = ctx.read_response(egui::Id::new(CANVAS_ID)).unwrap().rect;
        let corner = canvas_rect.right_bottom();

        // Ctrl + Double-Click on bottom-right corner enters interactive canvas resize mode
        for pressed in [true, false, true, false] {
            let _ = run(
                &mut app,
                vec![
                    egui::Event::PointerMoved(corner),
                    egui::Event::PointerButton {
                        pos: corner,
                        button: PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::CTRL,
                    },
                ],
                egui::Modifiers::CTRL,
            );
        }
        assert!(app.tab().canvas_resize.is_some());
        assert_eq!(
            app.tab().canvas_resize.as_ref().unwrap().preview_size,
            (100, 80)
        );

        // Dragging bottom-right corner outward updates preview_size; Esc cancels
        let expanded = canvas_rect.min + vec2(130.0, 100.0);
        let _ = run(
            &mut app,
            vec![
                egui::Event::PointerMoved(corner),
                egui::Event::PointerButton {
                    pos: corner,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            egui::Modifiers::NONE,
        );
        let _ = run(
            &mut app,
            vec![egui::Event::PointerMoved(expanded)],
            egui::Modifiers::NONE,
        );
        let _ = run(
            &mut app,
            vec![
                egui::Event::PointerMoved(expanded),
                egui::Event::PointerButton {
                    pos: expanded,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            egui::Modifiers::NONE,
        );
        assert_eq!(
            app.tab().canvas_resize.as_ref().unwrap().preview_size,
            (130, 100)
        );
        let _ = run(
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            egui::Modifiers::NONE,
        );
        assert!(app.tab().canvas_resize.is_none());
        assert_eq!(app.tab().editor.document.dimensions(), (100, 80));

        // Let egui's multi-click streak reset before starting a fresh Ctrl + Double-Click
        for _ in 0..10 {
            let _ = run(&mut app, Vec::new(), egui::Modifiers::NONE);
        }

        // Re-enter via Ctrl + Double-Click, shrink canvas, and confirm with Ctrl rising edge
        for pressed in [true, false, true, false] {
            let _ = run(
                &mut app,
                vec![
                    egui::Event::PointerMoved(corner),
                    egui::Event::PointerButton {
                        pos: corner,
                        button: PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::CTRL,
                    },
                ],
                egui::Modifiers::CTRL,
            );
        }
        // Release initial Ctrl: must remain in resize mode
        let _ = run(&mut app, Vec::new(), egui::Modifiers::NONE);
        assert!(app.tab().canvas_resize.is_some());

        let shrunk = canvas_rect.min + vec2(60.0, 60.0);
        let _ = run(
            &mut app,
            vec![
                egui::Event::PointerMoved(corner),
                egui::Event::PointerButton {
                    pos: corner,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            egui::Modifiers::NONE,
        );
        let _ = run(
            &mut app,
            vec![egui::Event::PointerMoved(shrunk)],
            egui::Modifiers::NONE,
        );
        let _ = run(
            &mut app,
            vec![
                egui::Event::PointerMoved(shrunk),
                egui::Event::PointerButton {
                    pos: shrunk,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            egui::Modifiers::NONE,
        );
        assert_eq!(
            app.tab().canvas_resize.as_ref().unwrap().preview_size,
            (60, 60)
        );
        // Pressing Ctrl confirms the new canvas size and pushes one Undo step
        let _ = run(&mut app, Vec::new(), egui::Modifiers::CTRL);
        assert!(app.tab().canvas_resize.is_none());
        assert_eq!(app.tab().editor.document.dimensions(), (60, 60));
        assert_eq!(app.tab().editor.document.pixel(0, 10, 10), Some(red));
        assert!(app.tab_mut().editor.undo()?);
        assert_eq!(app.tab().editor.document.dimensions(), (100, 80));

        // Enter key also confirms a canvas resize
        app.tab_mut().canvas_resize = Some(CanvasResizeSession {
            original: (100, 80),
            preview_size: (110, 90),
            dragging: None,
            prev_ctrl: false,
        });
        let _ = run(
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            egui::Modifiers::NONE,
        );
        assert!(app.tab().canvas_resize.is_none());
        assert_eq!(app.tab().editor.document.dimensions(), (110, 90));
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
        let _canceled_dialog =
            SaveOptionsDialog::new_pdn(copy_path.clone(), false, &app.tab().editor.document, &ctx);
        assert!(!copy_path.exists());
        assert_eq!(app.tab().project_path.as_deref(), Some(orig_path.as_path()));
        assert!(app.tab().editor.has_unsaved_edits());

        // Complete Save As to copy_path: original orig.pdn stays byte-identical
        let save_as_dialog =
            SaveOptionsDialog::new_pdn(copy_path.clone(), false, &app.tab().editor.document, &ctx);
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
        let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/pdn/pdn52_roundtrip_layers.pdn");
        let dir = tempfile::tempdir()?;
        let export_png = dir.path().join("workflow_export.png");
        let saved_pdn = dir.path().join("workflow_resumed.pdn");

        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, Document::new(4, 4)?);
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0));

        // 1. Open and understand a layered project (locate layer + hidden layer)
        app.open_path(&ctx, fixture_path);
        assert_eq!(app.tab().editor.document.dimensions(), (96, 64));
        assert_eq!(app.tab().editor.document.layer_count(), 4);
        assert_eq!(app.tab().editor.document.layer_name(1), Some("Sombra 50"));
        assert_eq!(
            app.tab().editor.document.layer_name(2),
            Some("Camada oculta")
        );
        assert_eq!(
            app.tab().editor.document.layer_properties(2),
            Some((false, 255))
        );

        // Acceptance check: reorder, duplicate, merge down, then undo all three
        app.tab_mut().editor.move_layer(1, 3, "move-layer")?;
        app.tab_mut().editor.duplicate_layer(3)?;
        app.tab_mut().editor.merge_layer_down(4)?;
        assert_eq!(app.tab().editor.document.layer_count(), 4);
        for _ in 0..3 {
            assert!(app.tab_mut().editor.undo()?);
        }
        assert_eq!(app.tab().editor.document.layer_name(1), Some("Sombra 50"));
        assert_eq!(
            app.tab().editor.document.layer_name(2),
            Some("Camada oculta")
        );

        // 2. Edit a layer: change opacity, draw a stroke, undo ONLY the stroke
        let before_meta = app.tab().editor.layer_meta(1).unwrap();
        let px_before_stroke = app.tab().editor.document.pixel(1, 20, 20);
        app.tab_mut().editor.selected = 1;
        app.tab_mut()
            .editor
            .set_properties(1, (before_meta.visible, 180))?;
        app.tab_mut()
            .editor
            .record_properties(1, (before_meta.visible, before_meta.opacity));
        app.tab_mut()
            .editor
            .begin_stroke(Pixel([250, 20, 20, 255]), 1);
        app.tab_mut().editor.draw_to(Some((20, 20)));
        app.tab_mut().editor.finish_stroke();
        assert_ne!(app.tab().editor.document.pixel(1, 20, 20), px_before_stroke);
        app.step_history(false);
        assert_eq!(app.tab().editor.document.pixel(1, 20, 20), px_before_stroke);
        assert_eq!(
            app.tab().editor.document.layer_properties(1),
            Some((true, 180))
        );

        // 3. Navigate and edit a detail via zoom + scroll, then return to overview
        app.tab_mut().zoom = Some(4.0);
        app.tab_mut().view.scroll_to = Some(vec2(40.0, 30.0));
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| app.ui_frame(ctx),
        );
        app.tab_mut()
            .editor
            .begin_stroke(Pixel([10, 230, 40, 255]), 1);
        app.tab_mut().editor.draw_to(Some((35, 35)));
        app.tab_mut().editor.finish_stroke();
        let detail_pixel = app.tab().editor.document.pixel(1, 35, 35);
        app.tab_mut().zoom = None;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| app.ui_frame(ctx),
        );

        // 4. Export PNG and verify compositing + transparency + hidden-layer exclusion
        let expected_composite = app.tab().editor.document.composite()?;
        SaveOptionsDialog::new_png(export_png.clone(), false, &app.tab().editor.document, &ctx)
            .write_to_disk()?;
        let exported = Document::read_png(BufReader::new(File::open(&export_png)?))?;
        assert_eq!(exported.composite()?, expected_composite);

        // 5. Save, close, reopen, verify all properties, and continue editing
        app.tab_mut().project_path = Some(saved_pdn.clone());
        assert!(app.save_project(&ctx, false));
        app.close_tab_at(0);
        app.open_path(&ctx, saved_pdn);
        assert_eq!(app.tab().editor.document.dimensions(), (96, 64));
        assert_eq!(app.tab().editor.document.layer_count(), 4);
        assert_eq!(app.tab().editor.document.layer_name(1), Some("Sombra 50"));
        assert_eq!(
            app.tab().editor.document.layer_properties(1),
            Some((true, 180))
        );
        assert_eq!(
            app.tab().editor.document.layer_properties(2),
            Some((false, 255))
        );
        assert_eq!(app.tab().editor.document.pixel(1, 35, 35), detail_pixel);

        app.tab_mut().editor.selected = 0;
        let before_resume_px = app.tab().editor.document.pixel(0, 2, 2);
        app.tab_mut()
            .editor
            .begin_stroke(Pixel([99, 88, 77, 255]), 1);
        app.tab_mut().editor.draw_to(Some((2, 2)));
        app.tab_mut().editor.finish_stroke();
        assert_ne!(app.tab().editor.document.pixel(0, 2, 2), before_resume_px);
        assert!(app.tab_mut().editor.undo()?);
        assert_eq!(app.tab().editor.document.pixel(0, 2, 2), before_resume_px);
        Ok(())
    }

    #[test]
    #[ignore]
    fn phase1_interactive_performance_baseline() -> io::Result<()> {
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

        fn measure_workload(name: &str, width: u32, height: u32, layers: usize) -> io::Result<()> {
            let pdn_bytes = build_workload_pdn(width, height, layers)?;
            let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0));
            let ctx = egui::Context::default();

            let warmups = 2;
            let runs = 10;
            let mut open_ms = Vec::with_capacity(runs);
            let mut stroke_ms = Vec::with_capacity(runs);
            let mut nav_ms = Vec::with_capacity(runs);
            let mut export_ms = Vec::with_capacity(runs);

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
                let dt_open = t0.elapsed().as_secs_f64() * 1000.0;

                let mut total_stroke = 0.0;
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
                    total_stroke += t1.elapsed().as_secs_f64() * 1000.0;
                }
                let dt_stroke = total_stroke / 8.0;

                let mut total_nav = 0.0;
                for step in 0..10_u32 {
                    let t2 = std::time::Instant::now();
                    app.tab_mut().zoom = Some(1.0 + step as f32 * 0.2);
                    app.tab_mut().view.scroll_to =
                        Some(vec2(step as f32 * 16.0, step as f32 * 12.0));
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            ..Default::default()
                        },
                        |ctx| app.ui_frame(ctx),
                    );
                    total_nav += t2.elapsed().as_secs_f64() * 1000.0;
                }
                let dt_nav = total_nav / 10.0;

                let t3 = std::time::Instant::now();
                let mut png_out = Vec::new();
                app.tab().editor.document.write_png(&mut png_out)?;
                let dt_export = t3.elapsed().as_secs_f64() * 1000.0;

                if iter >= warmups {
                    open_ms.push(dt_open);
                    stroke_ms.push(dt_stroke);
                    nav_ms.push(dt_nav);
                    export_ms.push(dt_export);
                }
            }

            let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
            let vm_hwm = status
                .lines()
                .find(|line| line.starts_with("VmHWM:"))
                .map_or("n/a", str::trim);

            let (o_med, o_max) = stats(&open_ms);
            let (s_med, s_max) = stats(&stroke_ms);
            let (n_med, n_max) = stats(&nav_ms);
            let (e_med, e_max) = stats(&export_ms);
            println!(
                "BENCH {name} ({width}x{height} x{layers}): open={o_med:.2}/{o_max:.2} ms | stroke={s_med:.2}/{s_max:.2} ms | nav={n_med:.2}/{n_max:.2} ms | export={e_med:.2}/{e_max:.2} ms | {vm_hwm}"
            );
            Ok(())
        }

        measure_workload("typical", 800, 600, 4)?;
        measure_workload("large", 2560, 1440, 8)?;
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
        let ctx = egui::Context::default();
        let doc = Document::new(640, 480)?;
        let mut app = App::new(&ctx, doc);
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1120.0, 760.0));
        let mut output = egui::FullOutput::default();
        for _ in 0..2 {
            output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| app.ui_frame(ctx),
            );
        }
        let inspector_left = screen.right() - 246.0;
        let max_allowed_right = screen.right() - f32::from(CARD_GAP) + 2.0;
        for clipped in &output.shapes {
            let bounds = clipped.shape.visual_bounding_rect();
            // Skip the SidePanel's full-width background rect (874..1120) and inspect the cards inside.
            if bounds.is_positive()
                && bounds.left() >= inspector_left
                && (bounds.width() < 245.0 || bounds.left() > inspector_left + 1.0)
                && bounds.top() > 100.0
                && bounds.bottom() < screen.bottom() - 30.0
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
        let ctx = egui::Context::default();
        let mut doc = new_document(32, 24)?;
        doc.add_layer("Top")?;
        let mut app = App::new(&ctx, doc);
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1120.0, 760.0));
        let frame = |app: &mut App, events| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ctx| app.ui_frame(ctx),
            );
        };
        let press = |app: &mut App, key| {
            for pressed in [true, false] {
                frame(
                    app,
                    vec![egui::Event::Key {
                        key,
                        physical_key: Some(key),
                        pressed,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
            }
        };
        let click = |app: &mut App, pos| {
            for pressed in [true, false] {
                frame(
                    app,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
        };

        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        press(&mut app, egui::Key::Insert);
        assert_eq!(app.tab().editor.document.layer_count(), 2);
        let card = ctx.read_response(egui::Id::new(LAYERS_ID)).unwrap().rect;
        click(&mut app, card.left_top() + vec2(12.0, 12.0));
        assert!(app.layers_clicked);
        press(&mut app, egui::Key::Insert);
        assert_eq!(app.tab().editor.document.layer_count(), 3);
        assert_eq!(app.tab().editor.selected, 1);
        assert_eq!(app.tab().editor.document.layer_name(2), Some("Top"));
        assert_eq!(
            app.tab().editor.document.pixel(1, 0, 0),
            Some(Pixel::default())
        );
        press(&mut app, egui::Key::Delete);
        assert_eq!(app.tab().editor.document.layer_count(), 2);
        assert_eq!(app.tab().editor.document.layer_name(1), Some("Top"));
        assert!(app.tab_mut().editor.undo()?);
        assert_eq!(app.tab().editor.document.layer_count(), 3);
        assert!(app.tab_mut().editor.undo()?);
        assert_eq!(app.tab().editor.document.layer_count(), 2);

        app.tool = Tool::Pan;
        let canvas = ctx.read_response(egui::Id::new(CANVAS_ID)).unwrap().rect;
        click(&mut app, canvas.center());
        assert!(!app.layers_clicked);
        app.tab_mut().editor.select_all();
        press(&mut app, egui::Key::Delete);
        assert_eq!(app.tab().editor.document.layer_count(), 2);
        assert_eq!(
            app.tab().editor.document.pixel(0, 0, 0),
            Some(Pixel::default())
        );

        click(&mut app, card.left_top() + vec2(12.0, 12.0));
        press(&mut app, egui::Key::Delete);
        press(&mut app, egui::Key::Delete);
        assert_eq!(app.tab().editor.document.layer_count(), 1);
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
