use super::i18n::text as tr;
use std::{
    collections::{BTreeMap, HashMap},
    fs, io,
    path::{Path, PathBuf},
};

use eframe::egui::{self, Key, Modifiers};
use serde::{Deserialize, Serialize};

use super::{
    dialogs::modal,
    mouse_shortcuts::{self, MouseBindings},
    storage::{JsonWrite, UserFile},
};

const FILE_NAME: &str = "shortcuts.json";
/// The name used beside the executable by earlier versions.
const LEGACY_FILE_NAME: &str = "atalhos.json";
const VERSION: u32 = 1;
const PAINT_NET: &str = "Paint.NET";
const ADOBE: &str = "Adobe";

/// Shared surface of the keyboard (`Command`) and mouse (`MouseAction`)
/// binding registries, used by generic profile code.
pub(super) trait BindableAction: Copy + 'static {
    const ALL: &'static [Self];
    fn id(self) -> &'static str;
    fn label_key(self) -> &'static str;
    fn defaults(self, base: Base) -> &'static [&'static str];
}

/// Declares a binding registry enum: each entry has a stable ID (as used in
/// `shortcuts.json`), a translation key and the Paint.NET / Adobe defaults.
macro_rules! bindable_actions {
    (
        $(#[$meta:meta])*
        $vis:vis enum $ty:ident {
            $($action:ident : $id:literal, $label:literal, [$($paint:literal),*], [$($adobe:literal),*];)*
        }
    ) => {
        $(#[$meta])*
        $vis enum $ty { $($action),* }

        impl $ty {
            pub const ALL: &[Self] = &[$(Self::$action),*];

            pub fn id(self) -> &'static str {
                match self { $(Self::$action => $id),* }
            }

            pub fn label(self) -> &'static str {
                $crate::gui::i18n::text(self.label_key())
            }

            pub(super) fn label_key(self) -> &'static str {
                match self { $(Self::$action => $label),* }
            }

            pub(super) fn defaults(self, base: $crate::gui::shortcuts::Base) -> &'static [&'static str] {
                use $crate::gui::shortcuts::Base;
                match (self, base) {
                    $((Self::$action, Base::PaintNet) => &[$($paint),*],
                      (Self::$action, Base::Adobe) => &[$($adobe),*],)*
                }
            }
        }

        impl $crate::gui::shortcuts::BindableAction for $ty {
            const ALL: &'static [Self] = Self::ALL;

            fn id(self) -> &'static str {
                Self::id(self)
            }

            fn label_key(self) -> &'static str {
                Self::label_key(self)
            }

            fn defaults(self, base: $crate::gui::shortcuts::Base) -> &'static [&'static str] {
                Self::defaults(self, base)
            }
        }
    };
}
pub(super) use bindable_actions;

// New commands get a stable ID and entries here and in docs/shortcuts.schema.json.
// Empty defaults identify actions without a corresponding reference shortcut.
macro_rules! actions {
    ($($action:ident : $id:literal, $label:literal, [$($paint:literal),*], [$($adobe:literal),*];)*) => {
        bindable_actions! {
            #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
            pub enum Command {
                $($action : $id, $label, [$($paint),*], [$($adobe),*];)*
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
    InsertLayerInPanel: "layer.panel_insert", "layer-panel-insert", ["Insert"], ["Insert"];
    DeleteLayerInPanel: "layer.panel_delete", "layer-panel-delete", ["Delete"], ["Delete"];
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

impl Command {
    fn layers_only(self) -> bool {
        matches!(self, Self::InsertLayerInPanel | Self::DeleteLayerInPanel)
    }

    fn cycle_group(self, base: Base) -> Option<u8> {
        match self {
            Self::Rectangle | Self::Ellipse => Some(1),
            Self::Lasso | Self::Wand if base == Base::PaintNet => Some(1),
            Self::MovePixels | Self::MoveMask if base == Base::PaintNet => Some(2),
            Self::Bucket | Self::Gradient if base == Base::Adobe => Some(3),
            Self::LineCurve | Self::Shape => Some(4),
            Self::Brush | Self::Pencil if base == Base::Adobe => Some(5),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Hash)]
pub(super) struct ModFlags {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl ModFlags {
    pub(super) fn from_modifiers(modifiers: Modifiers) -> Self {
        Self {
            ctrl: modifiers.ctrl,
            alt: modifiers.alt,
            shift: modifiers.shift,
        }
    }

    pub(super) fn any(self) -> bool {
        self.ctrl || self.alt || self.shift
    }

    pub(super) fn matches(self, modifiers: Modifiers) -> bool {
        self.ctrl == modifiers.ctrl && self.alt == modifiers.alt && self.shift == modifiers.shift
    }

    pub(super) fn all_held(self, modifiers: Modifiers) -> bool {
        (!self.ctrl || modifiers.ctrl)
            && (!self.alt || modifiers.alt)
            && (!self.shift || modifiers.shift)
    }

    pub(super) fn apply_token(&mut self, token: &str) -> bool {
        match token.to_ascii_lowercase().as_str() {
            "ctrl" if !self.ctrl => self.ctrl = true,
            "alt" if !self.alt => self.alt = true,
            "shift" if !self.shift => self.shift = true,
            _ => return false,
        }
        true
    }

    pub(super) fn push_to(self, out: &mut String) {
        for (enabled, modifier) in [
            (self.ctrl, "Ctrl+"),
            (self.alt, "Alt+"),
            (self.shift, "Shift+"),
        ] {
            if enabled {
                out.push_str(modifier);
            }
        }
    }
}

pub(super) fn parse_key(name: &str) -> Option<Key> {
    Key::from_name(name).or_else(|| {
        Key::ALL
            .iter()
            .copied()
            .find(|candidate| candidate.name().eq_ignore_ascii_case(name))
    })
}

pub(super) fn backfill_missing<A: BindableAction>(
    bindings: &mut BTreeMap<String, Vec<String>>,
    base: Base,
    mut is_valid: impl FnMut(&BTreeMap<String, Vec<String>>) -> bool,
) {
    let mut missing = Vec::new();
    for &action in A::ALL {
        if let std::collections::btree_map::Entry::Vacant(entry) =
            bindings.entry(action.id().into())
        {
            entry.insert(Vec::new());
            missing.push(action);
        }
    }
    for action in missing {
        for &value in action.defaults(base) {
            bindings
                .get_mut(action.id())
                .expect("inserted missing action key")
                .push(value.into());
            if !is_valid(bindings) {
                // On schema evolution a new default yields to an existing customization.
                bindings
                    .get_mut(action.id())
                    .expect("inserted missing action key")
                    .pop();
            }
        }
    }
}

/// Error for a binding value that `action` cannot use (`key` takes the
/// action label and the value).
pub(super) fn binding_value_error(key: &str, action: impl BindableAction, value: &str) -> String {
    super::i18n::format_logical(
        key,
        &[&super::i18n::logical_text(action.label_key()), &value],
    )
}

/// Error for `binding` claimed by both `other` and `action` (`key` takes the
/// binding and both action labels).
pub(super) fn binding_conflict_error<A: BindableAction>(
    key: &str,
    binding: &str,
    other: A,
    action: A,
) -> String {
    super::i18n::format_logical(
        key,
        &[
            &binding,
            &super::i18n::logical_text(other.label_key()),
            &super::i18n::logical_text(action.label_key()),
        ],
    )
}

pub(super) fn binding_editor_row(
    ui: &mut egui::Ui,
    label: impl Into<egui::WidgetText>,
    id: &'static str,
    text_width: f32,
    values: &mut Vec<String>,
    defaults: &[&str],
) {
    ui.label(label);
    let mut text = values.join(";");
    if ui
        .add_sized(
            [text_width, 19.0],
            egui::TextEdit::singleline(&mut text).id_salt(id),
        )
        .changed()
    {
        *values = text.split(';').map(str::to_string).collect();
    }
    if ui.small_button(tr("preset-default")).clicked() {
        *values = defaults.iter().map(ToString::to_string).collect();
    }
    ui.end_row();
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
        let base = self.base;
        backfill_missing::<Command>(&mut self.bindings, base, |bindings| {
            Self::actions_for(base, bindings).is_ok()
        });
        mouse_shortcuts::add_missing(&mut self.bindings, self.base);
    }

    fn actions(&self) -> Result<Bindings, String> {
        Self::actions_for(self.base, &self.bindings)
    }

    fn actions_for(
        base: Base,
        all_bindings: &BTreeMap<String, Vec<String>>,
    ) -> Result<Bindings, String> {
        let mut mapped = Bindings::default();
        for &action in Command::ALL {
            for binding in all_bindings.get(action.id()).into_iter().flatten() {
                let chord = Chord::parse(binding)
                    .ok_or_else(|| binding_value_error("invalid-binding", action, binding))?;
                let candidates = mapped.entry((chord, action.layers_only())).or_default();
                if candidates.contains(&action) {
                    continue;
                }
                let native_group = |member: Command| {
                    member
                        .cycle_group(base)
                        .filter(|_| {
                            member
                                .defaults(base)
                                .iter()
                                .any(|value| Chord::parse(value) == Some(chord))
                        })
                        .unwrap_or(0)
                };
                let group = native_group(action);
                if let Some(other) = candidates
                    .iter()
                    .find(|&&other| group == 0 || native_group(other) != group)
                {
                    return Err(binding_conflict_error(
                        "binding-conflict",
                        &chord.label(),
                        *other,
                        action,
                    ));
                }
                candidates.push(action);
            }
        }
        Ok(mapped)
    }

    fn compile(&self) -> Result<(Bindings, MouseBindings), String> {
        Ok((self.actions()?, MouseBindings::compile(&self.bindings)?))
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
    fn validate_all(&self) -> Result<(), String> {
        for (name, profile) in &self.profiles {
            profile
                .compile()
                .map_err(|error| format!("{name}: {error}"))?;
        }
        Ok(())
    }

    fn read(path: &Path) -> io::Result<Self> {
        let mut config: Self = super::storage::read_json(path)?;
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
        for profile in config.profiles.values_mut() {
            profile.add_missing_actions();
        }
        config
            .validate_all()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok(config)
    }

    fn write(&self, path: &Path, replace: bool) -> io::Result<()> {
        super::storage::write_json_atomic(
            path,
            self,
            JsonWrite {
                trailing_newline: true,
                replace,
                ..JsonWrite::PRETTY
            },
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Chord {
    key: Key,
    mods: ModFlags,
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
        let key = parse_key(key.trim())?;
        let mut chord = Self::from_input(key, Modifiers::NONE);
        for value in modifiers
            .split('+')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if !chord.mods.apply_token(value) {
                return None;
            }
        }
        Some(chord)
    }

    fn from_input(key: Key, modifiers: Modifiers) -> Self {
        Self {
            key,
            mods: ModFlags::from_modifiers(modifiers),
        }
    }

    fn label(self) -> String {
        let mut label = String::new();
        self.mods.push_to(&mut label);
        label.push_str(self.key.name());
        label
    }

    fn clipboard_release(self) -> bool {
        matches!(self.key, Key::Copy | Key::Cut | Key::Paste)
            || (self.mods.ctrl && matches!(self.key, Key::C | Key::X | Key::V))
            || (self.mods.shift && matches!(self.key, Key::Delete | Key::Insert))
            || (self.mods.ctrl && self.key == Key::Insert)
    }
}

type Bindings = HashMap<(Chord, bool), Vec<Command>>;

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
        let (bindings, mouse) = config.profiles[&config.active]
            .compile()
            .expect("valid default shortcuts");
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
    pub fn from_user_files() -> Self {
        match super::storage::config_file(FILE_NAME) {
            Ok(path) => Self::load(super::storage::migrate(
                path,
                super::storage::exe_adjacent(LEGACY_FILE_NAME),
                super::storage::JSON_LIMIT,
            )),
            Err(_) => Self {
                storage_error: Some(tr("settings-dir-unavailable").to_owned()),
                ..Self::default()
            },
        }
    }

    fn load(file: UserFile) -> Self {
        let mut shortcuts = Self {
            storage_error: file.migration_error.map(|error| error.to_string()),
            ..Self::default()
        };
        let path = file.path;
        match Profiles::read(&file.read_from) {
            Ok(config) => {
                let (bindings, mouse) = config.profiles[&config.active]
                    .compile()
                    .expect("validated configuration");
                shortcuts.bindings = bindings;
                shortcuts.mouse = mouse;
                shortcuts.config = config;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound && file.read_from == path => {
                if let Err(error) = shortcuts.config.write(&path, false) {
                    shortcuts.storage_error = Some(error.to_string());
                }
            }
            Err(error) => shortcuts.storage_error = Some(error.to_string()),
        }
        shortcuts.path = Some(path);
        shortcuts
    }

    fn hint_for(&self, id: &str) -> String {
        self.config.profiles[&self.config.active]
            .bindings
            .get(id)
            .map(|values| values.join(" / "))
            .unwrap_or_default()
    }

    pub fn hint(&self, action: Command) -> String {
        self.hint_for(action.id())
    }

    pub fn mouse_hint(&self, action: mouse_shortcuts::MouseAction) -> String {
        self.hint_for(action.id())
    }

    pub fn tooltip(&self, label_key: &str, action: Command) -> String {
        super::i18n::format(
            "shortcut-with-hint",
            &[&super::i18n::logical_text(label_key), &self.hint(action)],
        )
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
            self.bindings.iter().any(|((chord, layers_only), actions)| {
                !layers_only
                    && actions.contains(&action)
                    && input.key_down(chord.key)
                    && *chord == Chord::from_input(chord.key, input.modifiers)
            })
        })
    }

    pub fn remember_tool(&mut self, action: Command) {
        let base = self.config.profiles[&self.config.active].base;
        if base == Base::Adobe
            && let Some(group) = action.cycle_group(base)
        {
            self.adobe_group_current.insert(group, action);
        }
    }

    pub fn commands(&mut self, ctx: &egui::Context, layers_focused: bool) -> Vec<Command> {
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
                    let chord = if self.bindings.contains_key(&(chord, layers_focused)) {
                        chord
                    } else {
                        Chord::from_input((*physical_key)?, *modifiers)
                    };
                    if *pressed == chord.clipboard_release() {
                        return None;
                    }
                    let actions = self.bindings.get(&(chord, layers_focused))?;
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
                        let active = actions[0]
                            .cycle_group(base)
                            .and_then(|g| self.adobe_group_current.get(&g).copied())
                            .unwrap_or(actions[0]);
                        let current = actions
                            .iter()
                            .position(|action| *action == active)
                            .unwrap_or(0);
                        (current + usize::from(chord.mods.shift)) % actions.len()
                    } else if let Some((previous, at, index)) = self.last_cycle
                        && previous == chord
                        && input.time - at <= 1.0
                    {
                        (index
                            + if chord.mods.shift {
                                actions.len() - 1
                            } else {
                                1
                            })
                            % actions.len()
                    } else if chord.mods.shift {
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
                                    let values = profile
                                        .bindings
                                        .get_mut(action.id())
                                        .expect("registered action");
                                    binding_editor_row(
                                        ui,
                                        action.label(),
                                        action.id(),
                                        175.0,
                                        values,
                                        action.defaults(profile.base),
                                    );
                                }
                            }
                        });
                });
            if let Some(problem) = &dialog.problem {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    super::i18n::display(problem).into_owned(),
                );
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
        for profile in config.profiles.values_mut() {
            for &action in Command::ALL {
                let values = profile.bindings.get_mut(action.id()).expect("known action");
                values.retain(|value| !value.trim().is_empty());
                for value in values {
                    if let Some(chord) = Chord::parse(value) {
                        *value = chord.label();
                    }
                }
            }
            mouse_shortcuts::clean_empty(&mut profile.bindings);
        }
        config.validate_all()?;
        let (bindings, mouse) = config.profiles[&config.active].compile()?;
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| tr("settings-dir-unavailable").to_owned())?;
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
            ctrl: chord.mods.ctrl,
            command: chord.mods.ctrl,
            shift: chord.mods.shift,
            alt: chord.mods.alt,
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
                actions = shortcuts.commands(ctx, false);
            },
        );
        actions
    }

    fn user_file(path: PathBuf) -> UserFile {
        super::super::storage::migrate(path, None, super::super::storage::JSON_LIMIT)
    }

    #[test]
    fn migrates_customizations_from_the_old_shortcut_file() -> io::Result<()> {
        use super::super::storage::{JSON_LIMIT, migrate};

        let temp = tempfile::tempdir()?;
        let legacy = temp.path().join(LEGACY_FILE_NAME);
        let mut config = Profiles::default();
        let mut custom = config.profiles[PAINT_NET].clone();
        custom
            .bindings
            .insert(Command::Brush.id().into(), vec!["Ctrl+Alt+Shift+Y".into()]);
        config.profiles.insert("Mine".into(), custom);
        config.active = "Mine".into();
        config.write(&legacy, false)?;
        let before = std::fs::read(&legacy)?;

        let new = temp.path().join("config").join(FILE_NAME);
        let shortcuts = Shortcuts::load(migrate(new.clone(), Some(legacy.clone()), JSON_LIMIT));
        assert_eq!(shortcuts.storage_error, None);
        assert_eq!(shortcuts.hint(Command::Brush), "Ctrl+Alt+Shift+Y");
        assert_eq!(std::fs::read(&new)?, before);
        assert_eq!(std::fs::read(&legacy)?, before);
        Ok(())
    }

    #[test]
    fn stock_profiles_match_selection_cycles_and_do_not_conflict() -> Result<(), String> {
        let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
        let mut shortcuts = Shortcuts::load(user_file(directory.path().join(FILE_NAME)));
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
        let mut shortcuts = Shortcuts::load(user_file(path.clone()));
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
        let mut restarted = Shortcuts::load(user_file(path));
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
        let mut shortcuts = Shortcuts::load(user_file(path.clone()));
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
            serde_json::from_str(include_str!("../../docs/shortcuts.schema.json"))?;
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
