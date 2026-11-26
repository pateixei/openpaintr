use super::i18n::text as tr;
use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File},
    io::{self, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use eframe::egui::{self, Key, Modifiers};
use serde::{Deserialize, Serialize};

use super::{
    dialogs::modal,
    mouse_shortcuts::{self, MouseBindings},
};

const FILE_NAME: &str = "atalhos.json";
const VERSION: u32 = 1;
const PAINT_NET: &str = "Paint.NET";
const ADOBE: &str = "Adobe";

// New commands get a stable ID and entries here and in docs/atalhos.schema.json.
// Empty defaults identify actions without a corresponding reference shortcut.
macro_rules! actions {
    ($($action:ident : $id:literal, $label:literal, [$($paint:literal),*], [$($adobe:literal),*];)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Command { $($action),* }

        impl Command {
            pub const ALL: &[Self] = &[$(Self::$action),*];

            pub fn id(self) -> &'static str {
                match self { $(Self::$action => $id),* }
            }

            pub fn label(self) -> &'static str {
                tr(self.label_key())
            }

            fn label_key(self) -> &'static str {
                match self { $(Self::$action => $label),* }
            }

            fn defaults(self, base: Base) -> &'static [&'static str] {
                match (self, base) {
                    $((Self::$action, Base::PaintNet) => &[$($paint),*],
                      (Self::$action, Base::Adobe) => &[$($adobe),*],)*
                }
            }
        }
    };
}

actions! {
    Brush: "tool.brush", "brush", ["B"], ["B", "Shift+B"];
    Pencil: "tool.pencil", "pencil", ["P"], ["B", "Shift+B"];
    Bucket: "tool.bucket", "bucket", ["F"], ["G", "Shift+G"];
    Eraser: "tool.eraser", "eraser", ["E"], ["E"];
    Eyedropper: "tool.eyedropper", "eyedropper", ["K"], ["I"];
    CloneStamp: "tool.clone_stamp", "clone-stamp", ["L"], ["S"];
    Recolor: "tool.recolor", "recolor", ["R"], ["J"];
    Gradient: "tool.gradient", "gradient", ["G"], ["G", "Shift+G"];
    LineCurve: "tool.line_curve", "line-curve", ["O", "Shift+O"], ["U", "Shift+U"];
    Shape: "tool.shape", "shapes", ["O", "Shift+O"], ["U", "Shift+U"];
    Text: "tool.text", "text", ["T"], ["T"];
    Rectangle: "tool.rectangle_select", "rectangle-select", ["S", "Shift+S"], ["M", "Shift+M"];
    Lasso: "tool.lasso", "lasso", ["S", "Shift+S"], ["L"];
    Ellipse: "tool.ellipse_select", "ellipse-select", ["S", "Shift+S"], ["M", "Shift+M"];
    Wand: "tool.magic_wand", "magic-wand", ["S", "Shift+S"], ["W"];
    Pan: "tool.pan", "hand", ["H"], ["H"];
    Zoom: "tool.zoom", "zoom-tool", ["Z"], ["Z"];
    TemporaryPan: "tool.temporary_pan", "temporary-hand", ["Space"], ["Space"];
    BrushSmaller: "tool.brush_smaller", "brush-narrower", ["OpenBracket"], ["OpenBracket"];
    BrushLarger: "tool.brush_larger", "brush-wider", ["CloseBracket"], ["CloseBracket"];
    BrushSmallerFast: "tool.brush_smaller_fast", "brush-narrower-five", ["Ctrl+OpenBracket"], [];
    BrushLargerFast: "tool.brush_larger_fast", "brush-wider-five", ["Ctrl+CloseBracket"], [];
    New: "file.new", "new-image", ["Ctrl+N"], ["Ctrl+N"];
    Open: "file.open", "open", ["Ctrl+O"], ["Ctrl+O"];
    Save: "file.save", "save", ["Ctrl+S"], ["Ctrl+S"];
    SaveAs: "file.save_as", "save-as", ["Ctrl+Shift+S"], ["Ctrl+Shift+S"];
    Export: "file.export_png", "export-png", [], [];
    Close: "file.close", "close-document", ["Ctrl+W", "Ctrl+F4"], ["Ctrl+W"];
    CloseAll: "file.close_all", "close-all", ["Ctrl+Shift+W"], ["Ctrl+Alt+W"];
    Exit: "file.exit", "exit", ["Ctrl+Q"], ["Ctrl+Q"];
    NextDocument: "view.next_document", "next-document", ["Ctrl+Tab", "Ctrl+PageDown"], ["Ctrl+Tab", "Ctrl+PageDown"];
    PrevDocument: "view.prev_document", "previous-document", ["Ctrl+Shift+Tab", "Ctrl+PageUp"], ["Ctrl+Shift+Tab", "Ctrl+PageUp"];
    Undo: "edit.undo", "undo", ["Ctrl+Z"], ["Ctrl+Z"];
    Redo: "edit.redo", "redo", ["Ctrl+Y"], ["Ctrl+Shift+Z"];
    Cut: "edit.cut", "cut", ["Ctrl+X", "Shift+Delete"], ["Ctrl+X", "F2"];
    Copy: "edit.copy", "copy", ["Ctrl+C", "Ctrl+Insert"], ["Ctrl+C", "F3"];
    CopyComposite: "edit.copy_composite", "copy-composite", ["Ctrl+Shift+C"], ["Ctrl+Shift+C"];
    Paste: "edit.paste", "paste", ["Ctrl+V", "Shift+Insert"], ["Ctrl+V", "F4"];
    PasteLayer: "edit.paste_layer", "paste-new-layer", ["Ctrl+Shift+V"], [];
    PasteImage: "edit.paste_image", "paste-new-image", ["Ctrl+Alt+V"], [];
    CopyMask: "selection.copy_mask", "copy-mask", ["Ctrl+Alt+Shift+C"], [];
    PasteMask: "selection.paste_mask", "paste-mask-replace", ["Ctrl+Alt+Shift+V"], [];
    SelectAll: "selection.all", "select-all", ["Ctrl+A"], ["Ctrl+A"];
    Deselect: "selection.clear", "deselect", ["Ctrl+D", "Enter"], ["Ctrl+D"];
    Invert: "selection.invert", "invert-selection", ["Ctrl+I"], ["Ctrl+Shift+I", "Shift+F7"];
    Fill: "selection.fill", "fill-current", ["Backspace"], ["Alt+Backspace"];
    Erase: "selection.erase", "erase-selection", ["Delete"], ["Delete"];
    MovePixels: "selection.move_pixels", "move-transform-pixels", ["M"], ["V"];
    MoveMask: "selection.move_mask", "move-transform-mask", ["M", "Shift+M"], [];
    ZoomIn: "view.zoom_in", "zoom-in", ["Ctrl+Plus", "Ctrl+Equals", "Ctrl+Shift+Plus"], ["Ctrl+Plus", "Ctrl+Equals", "Ctrl+Shift+Plus"];
    ZoomOut: "view.zoom_out", "zoom-out", ["Ctrl+Minus"], ["Ctrl+Minus"];
    Fit: "view.fit", "fit-window", ["Ctrl+B"], ["Ctrl+0"];
    FitSelection: "view.fit_selection", "fit-selection", ["Ctrl+Shift+B"], [];
    ActualSize: "view.actual_size", "actual-size", ["Ctrl+0"], ["Ctrl+1"];
    Fullscreen: "view.fullscreen", "full-screen", ["F11"], ["F11"];
    Grid: "view.pixel_grid", "pixel-grid", [], [];
    Rulers: "view.rulers", "rulers", [], ["Ctrl+R"];
    Crop: "image.crop", "crop-selection", ["Ctrl+Shift+X"], [];
    Resize: "image.resize", "resize-image", ["Ctrl+R"], ["Ctrl+Alt+I"];
    CanvasSize: "image.canvas_size", "canvas-size", ["Ctrl+Shift+R"], ["Ctrl+Alt+C"];
    FlipImageX: "image.flip_horizontal", "flip-image-horizontal", [], [];
    FlipImageY: "image.flip_vertical", "flip-image-vertical", [], [];
    RotateCw: "image.rotate_clockwise", "rotate-image-clockwise", ["Ctrl+H"], [];
    RotateCcw: "image.rotate_counterclockwise", "rotate-image-counterclockwise", ["Ctrl+G"], [];
    RotateHalf: "image.rotate_half", "rotate-image-half", [], [];
    Flatten: "image.flatten", "flatten", ["Ctrl+Shift+F"], [];
    AddLayer: "layer.add", "add-layer", ["Ctrl+Shift+N"], ["Ctrl+Alt+Shift+N"];
    DeleteLayer: "layer.delete", "delete-layer", ["Ctrl+Shift+Delete"], [];
    DuplicateLayer: "layer.duplicate", "duplicate-layer", ["Ctrl+Shift+D"], ["Ctrl+J"];
    MergeLayerDown: "layer.merge_down", "merge-layer-down", ["Ctrl+M"], ["Ctrl+E"];
    ImportLayer: "layer.import_file", "import-layer-file", [], [];
    LayerProperties: "layer.properties", "layer-properties", ["F4"], [];
    MoveLayerTop: "layer.move_top", "layer-to-top", [], ["Ctrl+Shift+CloseBracket"];
    MoveLayerUp: "layer.move_up", "layer-up", [], ["Ctrl+CloseBracket"];
    MoveLayerDown: "layer.move_down", "layer-down", [], ["Ctrl+OpenBracket"];
    MoveLayerBottom: "layer.move_bottom", "layer-to-bottom", [], ["Ctrl+Shift+OpenBracket"];
    GoTopLayer: "layer.go_top", "select-top-layer", ["Alt+Home"], ["Alt+Period"];
    GoUpLayer: "layer.go_up", "select-layer-above", ["Alt+PageUp"], ["Alt+CloseBracket"];
    GoDownLayer: "layer.go_down", "select-layer-below", ["Alt+PageDown"], ["Alt+OpenBracket"];
    GoBottomLayer: "layer.go_bottom", "select-bottom-layer", ["Alt+End"], ["Alt+Comma"];
    ToggleLayer: "layer.toggle_visibility", "toggle-layer-visibility", ["Ctrl+Comma"], [];
    FlipLayerX: "layer.flip_horizontal", "flip-layer-horizontal", [], [];
    FlipLayerY: "layer.flip_vertical", "flip-layer-vertical", [], [];
    RotateLayerHalf: "layer.rotate_half", "rotate-layer-half", [], [];
    RotateZoom: "layer.rotate_zoom", "layer-rotate-zoom", ["Ctrl+Shift+Z"], [];
    FreeTransform: "edit.free_transform", "free-transform", [], ["Ctrl+T"];
    SwapColors: "color.swap", "toggle-colors", ["X"], ["X"];
    ResetColors: "color.reset", "reset-colors", [], ["D"];
    Settings: "settings.shortcuts", "configure-shortcuts", ["Alt+X"], ["Ctrl+Alt+Shift+K"];
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) enum Base {
    #[serde(rename = "Paint.NET")]
    PaintNet,
    Adobe,
}

#[derive(Clone, Deserialize, Serialize)]
struct Profile {
    base: Base,
    #[serde(default)]
    bindings: BTreeMap<String, Vec<String>>,
}

impl Profile {
    fn new(base: Base) -> Self {
        let mut profile = Self {
            base,
            bindings: BTreeMap::new(),
        };
        profile.add_missing_actions();
        profile
    }

    fn add_missing_actions(&mut self) {
        let mut missing = Vec::new();
        for &action in Command::ALL {
            if let std::collections::btree_map::Entry::Vacant(entry) =
                self.bindings.entry(action.id().into())
            {
                entry.insert(Vec::new());
                missing.push(action);
            }
        }
        for action in missing {
            for value in action.defaults(self.base) {
                self.bindings
                    .get_mut(action.id())
                    .unwrap()
                    .push((*value).into());
                if self.actions().is_err() {
                    // On schema evolution a new default yields to an existing customization.
                    self.bindings.get_mut(action.id()).unwrap().pop();
                }
            }
        }
        mouse_shortcuts::add_missing(&mut self.bindings, self.base);
    }

    fn actions(&self) -> Result<Bindings, String> {
        let mut mapped = Bindings::default();
        for &action in Command::ALL {
            for binding in &self.bindings[action.id()] {
                let chord = Chord::parse(binding).ok_or_else(|| {
                    super::i18n::format_logical("invalid-binding", &[&super::i18n::logical_text(action.label_key()), &binding])
                })?;
                let candidates = mapped.entry(chord).or_default();
                if candidates.contains(&action) {
                    continue;
                }
                let native_group = |member: Command| {
                    let group = match member {
                        Command::Rectangle | Command::Ellipse | Command::Lasso | Command::Wand => 1,
                        Command::MovePixels | Command::MoveMask if self.base == Base::PaintNet => 2,
                        Command::Bucket | Command::Gradient if self.base == Base::Adobe => 3,
                        Command::LineCurve | Command::Shape => 4,
                        Command::Brush | Command::Pencil if self.base == Base::Adobe => 5,
                        _ => 0,
                    };
                    if member
                        .defaults(self.base)
                        .iter()
                        .any(|value| Chord::parse(value) == Some(chord))
                    {
                        group
                    } else {
                        0
                    }
                };
                let group = native_group(action);
                if let Some(other) = candidates
                    .iter()
                    .find(|&&other| group == 0 || native_group(other) != group)
                {
                    return Err(super::i18n::format_logical(
                        "binding-conflict",
                        &[&chord.label(), &super::i18n::logical_text(other.label_key()), &super::i18n::logical_text(action.label_key())],
                    ));
                }
                candidates.push(action);
            }
        }
        Ok(mapped)
    }
}

#[derive(Clone, Deserialize, Serialize)]
struct Profiles {
    version: u32,
    active: String,
    profiles: BTreeMap<String, Profile>,
}

impl Default for Profiles {
    fn default() -> Self {
        Self {
            version: VERSION,
            active: PAINT_NET.into(),
            profiles: BTreeMap::from([
                (PAINT_NET.into(), Profile::new(Base::PaintNet)),
                (ADOBE.into(), Profile::new(Base::Adobe)),
            ]),
        }
    }
}

impl Profiles {
    fn read(path: &Path) -> io::Result<Self> {
        let mut config: Self =
            serde_json::from_reader(BufReader::new(File::open(path)?)).map_err(io::Error::other)?;
        if config.version != VERSION {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                super::i18n::format_logical("unsupported-shortcuts-version", &[&config.version]),
            ));
        }
        for (name, base) in [(PAINT_NET, Base::PaintNet), (ADOBE, Base::Adobe)] {
            config
                .profiles
                .entry(name.into())
                .or_insert_with(|| Profile::new(base));
        }
        if !config.profiles.contains_key(&config.active) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                tr("no-active-profile"),
            ));
        }
        for (name, profile) in &mut config.profiles {
            profile.add_missing_actions();
            profile.actions().map_err(|error| {
                io::Error::new(io::ErrorKind::InvalidData, format!("{name}: {error}"))
            })?;
            MouseBindings::compile(&profile.bindings).map_err(|error| {
                io::Error::new(io::ErrorKind::InvalidData, format!("{name}: {error}"))
            })?;
        }
        Ok(config)
    }

    fn write(&self, path: &Path, replace: bool) -> io::Result<()> {
        let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
        {
            let mut writer = BufWriter::new(file.as_file_mut());
            serde_json::to_writer_pretty(&mut writer, self).map_err(io::Error::other)?;
            writeln!(writer)?;
            writer.flush()?;
        }
        file.as_file().sync_all()?;
        if replace {
            file.persist(path)?;
        } else {
            file.persist_noclobber(path)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Chord {
    key: Key,
    ctrl: bool,
    shift: bool,
    alt: bool,
}

impl Chord {
    fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (modifiers, key) = if text == "+" {
            ("", "Plus")
        } else if let Some(modifiers) = text.strip_suffix("++") {
            (modifiers, "Plus")
        } else {
            text.rsplit_once('+').unwrap_or(("", text))
        };
        let key = key.trim();
        let key = Key::from_name(key).or_else(|| {
            Key::ALL
                .iter()
                .copied()
                .find(|candidate| candidate.name().eq_ignore_ascii_case(key))
        })?;
        let mut chord = Self::from_input(key, Modifiers::NONE);
        for value in modifiers
            .split('+')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            match value.to_ascii_lowercase().as_str() {
                "ctrl" => chord.ctrl = true,
                "shift" => chord.shift = true,
                "alt" => chord.alt = true,
                _ => return None,
            }
        }
        Some(chord)
    }

    fn from_input(key: Key, modifiers: Modifiers) -> Self {
        Self {
            key,
            ctrl: modifiers.ctrl,
            shift: modifiers.shift,
            alt: modifiers.alt,
        }
    }

    fn label(self) -> String {
        let mut label = String::new();
        for (enabled, modifier) in [
            (self.ctrl, "Ctrl+"),
            (self.alt, "Alt+"),
            (self.shift, "Shift+"),
        ] {
            if enabled {
                label.push_str(modifier);
            }
        }
        label.push_str(self.key.name());
        label
    }

    fn clipboard_release(self) -> bool {
        matches!(self.key, Key::Copy | Key::Cut | Key::Paste)
            || (self.ctrl && matches!(self.key, Key::C | Key::X | Key::V))
            || (self.shift && matches!(self.key, Key::Delete | Key::Insert))
            || (self.ctrl && self.key == Key::Insert)
    }
}

type Bindings = HashMap<Chord, Vec<Command>>;

pub struct Shortcuts {
    path: Option<PathBuf>,
    pub storage_error: Option<String>,
    config: Profiles,
    bindings: Bindings,
    pub mouse: MouseBindings,
    last_cycle: Option<(Chord, f64, usize)>,
    adobe_group_current: HashMap<u8, Command>,
}

impl Default for Shortcuts {
    fn default() -> Self {
        let config = Profiles::default();
        let active = &config.profiles[&config.active];
        let bindings = active.actions().expect("atalhos padrão válidos");
        let mouse = MouseBindings::compile(&active.bindings).expect("gestos padrão válidos");
        Self {
            path: None,
            storage_error: None,
            config,
            bindings,
            mouse,
            last_cycle: None,
            adobe_group_current: HashMap::new(),
        }
    }
}

impl Shortcuts {
    pub fn from_executable() -> Self {
        match std::env::current_exe().and_then(|exe| {
            exe.parent()
                .map(|parent| parent.join(FILE_NAME))
                .ok_or_else(|| io::Error::other(tr("executable-dir-unavailable")))
        }) {
            Ok(path) => Self::load(path),
            Err(error) => Self {
                storage_error: Some(error.to_string()),
                ..Self::default()
            },
        }
    }

    fn load(path: PathBuf) -> Self {
        let mut shortcuts = Self::default();
        match Profiles::read(&path) {
            Ok(config) => {
                let active = &config.profiles[&config.active];
                shortcuts.bindings = active.actions().expect("configuração validada");
                shortcuts.mouse =
                    MouseBindings::compile(&active.bindings).expect("gestos validados");
                shortcuts.config = config;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if let Err(error) = shortcuts.config.write(&path, false) {
                    shortcuts.storage_error = Some(error.to_string());
                }
            }
            Err(error) => shortcuts.storage_error = Some(error.to_string()),
        }
        shortcuts.path = Some(path);
        shortcuts
    }

    pub fn hint(&self, action: Command) -> String {
        self.config.profiles[&self.config.active].bindings[action.id()].join(" / ")
    }

    pub fn mouse_hint(&self, action: mouse_shortcuts::MouseAction) -> String {
        self.config.profiles[&self.config.active].bindings[action.id()].join(" / ")
    }

    pub fn button(&self, ui: &egui::Ui, action: Command) -> egui::Button<'static> {
        let button = match super::tool_icons::command_image(ui, action) {
            Some(image) => egui::Button::image_and_text(image, action.label()),
            None => egui::Button::new(action.label()),
        };
        button.shortcut_text(self.hint(action))
    }

    pub fn held(&self, ctx: &egui::Context, action: Command) -> bool {
        ctx.input(|input| {
            self.bindings.iter().any(|(chord, actions)| {
                actions.contains(&action)
                    && input.key_down(chord.key)
                    && *chord == Chord::from_input(chord.key, input.modifiers)
            })
        })
    }

    fn tool_cycle_group(action: Command) -> Option<u8> {
        match action {
            Command::Rectangle | Command::Ellipse => Some(1),
            Command::Bucket | Command::Gradient => Some(3),
            Command::LineCurve | Command::Shape => Some(4),
            Command::Brush | Command::Pencil => Some(5),
            _ => None,
        }
    }

    pub fn remember_tool(&mut self, action: Command) {
        if let Some(group) = Self::tool_cycle_group(action) {
            self.adobe_group_current.insert(group, action);
        }
    }

    pub fn commands(&mut self, ctx: &egui::Context) -> Vec<Command> {
        ctx.input(|input| {
            input
                .events
                .iter()
                .filter_map(|event| {
                    let egui::Event::Key {
                        key,
                        physical_key,
                        pressed,
                        modifiers,
                        repeat,
                    } = event
                    else {
                        return None;
                    };
                    let chord = Chord::from_input(*key, *modifiers);
                    let chord = if self.bindings.contains_key(&chord) {
                        chord
                    } else {
                        Chord::from_input((*physical_key)?, *modifiers)
                    };
                    if *pressed == chord.clipboard_release() {
                        return None;
                    }
                    let actions = self.bindings.get(&chord)?;
                    if *repeat
                        && !matches!(
                            actions.as_slice(),
                            [Command::ZoomIn
                                | Command::ZoomOut
                                | Command::BrushSmaller
                                | Command::BrushLarger
                                | Command::BrushSmallerFast
                                | Command::BrushLargerFast]
                        )
                    {
                        return None;
                    }
                    if actions.len() == 1 {
                        self.last_cycle = None;
                        return Some(actions[0]);
                    }
                    let base = self.config.profiles[&self.config.active].base;
                    let index = if base == Base::Adobe {
                        let active = Self::tool_cycle_group(actions[0])
                            .and_then(|g| self.adobe_group_current.get(&g).copied())
                            .unwrap_or(actions[0]);
                        let current = actions
                            .iter()
                            .position(|action| *action == active)
                            .unwrap_or(0);
                        (current + usize::from(chord.shift)) % actions.len()
                    } else if let Some((previous, at, index)) = self.last_cycle
                        && previous == chord
                        && input.time - at <= 1.0
                    {
                        (index + if chord.shift { actions.len() - 1 } else { 1 }) % actions.len()
                    } else if chord.shift {
                        actions.len() - 1
                    } else {
                        0
                    };
                    self.last_cycle = Some((chord, input.time, index));
                    let action = actions[index];
                    self.remember_tool(action);
                    Some(action)
                })
                .collect()
        })
    }

    pub fn dialog(&self) -> ShortcutDialog {
        ShortcutDialog {
            draft: self.config.clone(),
            name: String::new(),
            problem: self.storage_error.clone(),
            mouse_tab: false,
        }
    }

    pub fn settings(&mut self, ctx: &egui::Context, dialog: &mut ShortcutDialog) -> bool {
        let response = modal(ctx, "configurar_atalhos").show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 36.0).clamp(320.0, 560.0));
            ui.heading(tr("shortcuts"));
            if let Some(path) = &self.path {
                ui.weak(path.display().to_string());
            }
            ui.horizontal(|ui| {
                ui.label(tr("active-profile"));
                egui::ComboBox::from_id_salt("perfil_atalhos")
                    .selected_text(&dialog.draft.active)
                    .show_ui(ui, |ui| {
                        for name in dialog.draft.profiles.keys() {
                            ui.selectable_value(&mut dialog.draft.active, name.clone(), name);
                        }
                    });
                ui.separator();
                ui.selectable_value(&mut dialog.mouse_tab, false, tr("keyboard"));
                ui.selectable_value(&mut dialog.mouse_tab, true, tr("mouse"));
            });
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut dialog.name).hint_text(tr("new-profile-name")),
                );
                let name = dialog.name.trim();
                if ui
                    .add_enabled(
                        !name.is_empty() && !dialog.draft.profiles.contains_key(name),
                        egui::Button::new(tr("duplicate-profile")),
                    )
                    .clicked()
                {
                    dialog.draft.profiles.insert(
                        name.into(),
                        dialog.draft.profiles[&dialog.draft.active].clone(),
                    );
                    dialog.draft.active = name.into();
                    dialog.name.clear();
                }
            });
            if dialog.mouse_tab {
                ui.label(tr("mouse-bindings-help"));
            } else {
                ui.label(tr("key-bindings-help"));
            }
            let profile = dialog
                .draft
                .profiles
                .get_mut(&dialog.draft.active)
                .expect("perfil ativo");
            egui::ScrollArea::vertical()
                .max_height((ctx.content_rect().height() - 250.0).max(140.0))
                .show(ui, |ui| {
                    egui::Grid::new("comandos_atalhos")
                        .num_columns(3)
                        .striped(true)
                        .show(ui, |ui| {
                            if dialog.mouse_tab {
                                mouse_shortcuts::settings_rows(
                                    ui,
                                    &mut profile.bindings,
                                    profile.base,
                                );
                            } else {
                                for &action in Command::ALL {
                                    ui.label(action.label());
                                    let values = profile
                                        .bindings
                                        .get_mut(action.id())
                                        .expect("ação cadastrada");
                                    let mut text = values.join(";");
                                    if ui
                                        .add_sized(
                                            [175.0, 19.0],
                                            egui::TextEdit::singleline(&mut text)
                                                .id_salt(action.id()),
                                        )
                                        .changed()
                                    {
                                        *values = text.split(';').map(str::to_string).collect();
                                    }
                                    if ui.small_button(tr("preset-default")).clicked() {
                                        *values = action
                                            .defaults(profile.base)
                                            .iter()
                                            .map(ToString::to_string)
                                            .collect();
                                    }
                                    ui.end_row();
                                }
                            }
                        });
                });
            if let Some(problem) = &dialog.problem {
                ui.colored_label(ui.visuals().error_fg_color, super::i18n::display(problem).into_owned());
            }
            ui.horizontal(|ui| {
                (
                    ui.button(tr("save-profiles")).clicked(),
                    ui.button(tr("cancel")).clicked(),
                )
            })
            .inner
        });
        if response.inner.0 {
            match self.save_draft(dialog.draft.clone()) {
                Ok(()) => return true,
                Err(error) => dialog.problem = Some(error),
            }
        }
        response.inner.1 || response.should_close()
    }

    fn save_draft(&mut self, mut config: Profiles) -> Result<(), String> {
        for (name, profile) in &mut config.profiles {
            for &action in Command::ALL {
                let values = profile
                    .bindings
                    .get_mut(action.id())
                    .expect("ação conhecida");
                values.retain(|value| !value.trim().is_empty());
                for value in values {
                    if let Some(chord) = Chord::parse(value) {
                        *value = chord.label();
                    }
                }
            }
            mouse_shortcuts::clean_empty(&mut profile.bindings);
            profile
                .actions()
                .map_err(|error| format!("{name}: {error}"))?;
            MouseBindings::compile(&profile.bindings)
                .map_err(|error| format!("{name}: {error}"))?;
        }
        let active = &config.profiles[&config.active];
        let bindings = active.actions()?;
        let mouse = MouseBindings::compile(&active.bindings)?;
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| tr("executable-dir-unavailable").to_owned())?;
        // A corrupt or newer file is kept for recovery rather than overwritten.
        let replace = match fs::metadata(path) {
            Ok(_) => {
                Profiles::read(path)
                    .map_err(|error| super::i18n::format_logical("shortcuts-kept", &[&error]))?;
                true
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.to_string()),
        };
        config
            .write(path, replace)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        self.config = config;
        self.bindings = bindings;
        self.mouse = mouse;
        self.storage_error = None;
        self.last_cycle = None;
        Ok(())
    }
}

pub struct ShortcutDialog {
    draft: Profiles,
    name: String,
    problem: Option<String>,
    mouse_tab: bool,
}

#[cfg(test)]
mod tests {
    use super::super::mouse_shortcuts::MouseAction;
    use super::*;

    fn send(
        shortcuts: &mut Shortcuts,
        ctx: &egui::Context,
        input: &str,
        pressed: bool,
        time: f64,
    ) -> Vec<Command> {
        let chord = Chord::parse(input).unwrap();
        let modifiers = Modifiers {
            ctrl: chord.ctrl,
            command: chord.ctrl,
            shift: chord.shift,
            alt: chord.alt,
            ..Modifiers::NONE
        };
        let event = egui::Event::Key {
            key: chord.key,
            physical_key: Some(chord.key),
            pressed,
            repeat: false,
            modifiers,
        };
        let mut actions = Vec::new();
        let _ = ctx.run(
            egui::RawInput {
                events: vec![event],
                time: Some(time),
                modifiers,
                ..Default::default()
            },
            |ctx| {
                actions = shortcuts.commands(ctx);
            },
        );
        actions
    }

    #[test]
    fn stock_profiles_match_selection_cycles_and_do_not_conflict() -> Result<(), String> {
        let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
        let mut shortcuts = Shortcuts::load(directory.path().join(FILE_NAME));
        let ctx = egui::Context::default();
        assert_eq!(
            send(&mut shortcuts, &ctx, "S", true, 0.0),
            [Command::Rectangle]
        );
        assert_eq!(send(&mut shortcuts, &ctx, "S", false, 0.1), []);
        assert_eq!(send(&mut shortcuts, &ctx, "S", true, 0.2), [Command::Lasso]);
        assert_eq!(send(&mut shortcuts, &ctx, "S", false, 0.3), []);
        assert_eq!(
            send(&mut shortcuts, &ctx, "Shift+S", true, 2.0),
            [Command::Wand]
        );
        assert_eq!(
            send(&mut shortcuts, &ctx, "Ctrl+Y", true, 2.5),
            [Command::Redo]
        );
        assert_eq!(
            send(&mut shortcuts, &ctx, "Ctrl+Shift+Z", true, 2.6),
            [Command::RotateZoom]
        );

        let mut draft = shortcuts.config.clone();
        draft.active = ADOBE.into();
        shortcuts.save_draft(draft)?;
        assert_eq!(
            send(&mut shortcuts, &ctx, "M", true, 3.0),
            [Command::Rectangle]
        );
        assert_eq!(send(&mut shortcuts, &ctx, "M", false, 3.1), []);
        assert_eq!(
            send(&mut shortcuts, &ctx, "Shift+M", true, 3.2),
            [Command::Ellipse]
        );
        assert_eq!(send(&mut shortcuts, &ctx, "Ctrl+Shift+Z", false, 3.3), []);
        assert_eq!(
            send(&mut shortcuts, &ctx, "Ctrl+Shift+Z", true, 3.4),
            [Command::Redo]
        );
        assert_eq!(
            send(&mut shortcuts, &ctx, "Ctrl+Shift+C", false, 3.6),
            [Command::CopyComposite]
        );
        Ok(())
    }

    #[test]
    fn new_profile_and_active_binding_survive_restart_and_schema_evolution() -> Result<(), String> {
        let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
        let path = directory.path().join(FILE_NAME);
        let mut shortcuts = Shortcuts::load(path.clone());
        let mut draft = shortcuts.config.clone();
        let mut custom = draft.profiles[PAINT_NET].clone();
        custom
            .bindings
            .insert(Command::Brush.id().into(), vec!["F10".into()]);
        custom
            .bindings
            .insert(Command::Zoom.id().into(), Vec::new());
        custom
            .bindings
            .insert(Command::Export.id().into(), vec!["H".into()]);
        custom.bindings.remove(Command::Pan.id());
        custom.bindings.remove(Command::Eraser.id());
        custom.add_missing_actions();
        assert_eq!(custom.bindings[Command::Pan.id()], Vec::<String>::new());
        assert_eq!(custom.bindings[Command::Eraser.id()], ["E"]);
        draft.profiles.insert("Meu perfil".into(), custom);
        draft.active = "Meu perfil".into();
        shortcuts.save_draft(draft)?;
        let mut restarted = Shortcuts::load(path);
        assert_eq!(restarted.config.active, "Meu perfil");
        assert_eq!(restarted.hint(Command::Brush), "F10");
        assert!(restarted.hint(Command::Zoom).is_empty());
        assert_eq!(
            send(&mut restarted, &egui::Context::default(), "F10", true, 0.0),
            [Command::Brush]
        );
        Ok(())
    }

    #[test]
    fn conflicting_edits_leave_last_saved_file_intact() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join(FILE_NAME);
        let mut shortcuts = Shortcuts::load(path.clone());
        let before = fs::read(&path)?;
        let mut draft = shortcuts.config.clone();
        draft
            .profiles
            .get_mut(PAINT_NET)
            .unwrap()
            .bindings
            .insert(Command::Save.id().into(), vec!["ctrl+z".into()]);
        let error = shortcuts.save_draft(draft).unwrap_err();
        assert!(
            error.contains(Command::Save.label()) && error.contains(Command::Undo.label()),
            "{error}"
        );
        assert_eq!(fs::read(path)?, before);
        assert_eq!(shortcuts.hint(Command::Save), "Ctrl+S");
        Ok(())
    }

    #[test]
    fn schema_lists_every_registered_action() -> Result<(), serde_json::Error> {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../../docs/atalhos.schema.json"))?;
        let properties = schema["$defs"]["profile"]["properties"]["bindings"]["properties"]
            .as_object()
            .unwrap();
        assert_eq!(
            properties.len(),
            Command::ALL.len() + MouseAction::ALL.len()
        );
        for command in Command::ALL {
            assert!(properties.contains_key(command.id()), "{}", command.id());
        }
        for action in MouseAction::ALL {
            assert!(properties.contains_key(action.id()), "{}", action.id());
        }
        Ok(())
    }
}
