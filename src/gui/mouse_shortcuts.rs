use super::i18n::text as tr;
use std::collections::{BTreeMap, HashMap};

use eframe::egui::{self, Key, Modifiers, PointerButton};

use super::shortcuts::Base;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Scope {
    Canvas,
    CanvasCorner,
    Brush,
    Pencil,
    Bucket,
    Eraser,
    Eyedropper,
    CloneStamp,
    Recolor,
    Gradient,
    LineCurve,
    ShapeTool,
    Text,
    Selection,
    SelectionDrag,
    Wand,
    Hand,
    Zoom,
    ZoomDrag,
    ToolbarHand,
    ToolbarZoom,
    LayerRow,
    LayerUpButton,
    LayerDownButton,
    ColorWheel,
    TransformMove,
    TransformScale,
    TransformRotate,
}

impl Scope {
    fn conflicts_with(self, other: Self) -> bool {
        let canvas_tool = |scope| {
            matches!(
                scope,
                Self::Brush
                    | Self::Pencil
                    | Self::Bucket
                    | Self::Eraser
                    | Self::Eyedropper
                    | Self::CloneStamp
                    | Self::Recolor
                    | Self::Gradient
                    | Self::LineCurve
                    | Self::ShapeTool
                    | Self::Text
                    | Self::Selection
                    | Self::Wand
                    | Self::Hand
                    | Self::Zoom
            )
        };
        self == other
            || (self == Self::Canvas && canvas_tool(other))
            || (other == Self::Canvas && canvas_tool(self))
    }
}

macro_rules! mouse_actions {
    ($($action:ident : $id:literal, $label:literal, $scope:ident, [$($paint:literal),*], [$($adobe:literal),*];)*) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum MouseAction { $($action),* }

        impl MouseAction {
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

            pub fn scope(self) -> Scope {
                match self { $(Self::$action => Scope::$scope),* }
            }

            pub fn defaults(self, base: Base) -> &'static [&'static str] {
                match (self, base) {
                    $((Self::$action, Base::PaintNet) => &[$($paint),*],
                      (Self::$action, Base::Adobe) => &[$($adobe),*],)*
                }
            }
        }
    };
}

mouse_actions! {
    Pan: "canvas.pan", "mouse.canvas.pan", Canvas, ["Middle", "Space+Left", "Space+Right"], ["Space+Left"];
    CanvasCornerResize: "canvas.corner_resize", "mouse.canvas.corner_resize", CanvasCorner, ["Ctrl+DoubleLeft"], ["Ctrl+DoubleLeft"];
    WheelVertical: "canvas.scroll_vertical", "mouse.canvas.scroll_vertical", Canvas, ["Wheel"], ["Wheel"];
    WheelHorizontal: "canvas.scroll_horizontal", "mouse.canvas.scroll_horizontal", Canvas, ["Shift+Wheel", "WheelX"], ["Ctrl+Wheel", "WheelX"];
    WheelZoom: "canvas.wheel_zoom", "mouse.canvas.wheel_zoom", Canvas, ["Ctrl+Wheel"], ["Alt+Wheel"];
    WheelStrokeSize: "canvas.wheel_stroke_size", "mouse.canvas.wheel_stroke_size", Canvas, ["Ctrl+Shift+Wheel"], ["Ctrl+Shift+Wheel"];
    TempZoomIn: "canvas.temp_zoom_in", "mouse.canvas.temp_zoom_in", Canvas, [], ["Ctrl+Space+Left"];
    TempZoomOut: "canvas.temp_zoom_out", "mouse.canvas.temp_zoom_out", Canvas, [], ["Alt+Space+Left"];
    Hand: "hand.pan", "mouse.hand.pan", Hand, ["Left", "Right"], ["Left"];
    ZoomIn: "zoom.in", "mouse.zoom.in", Zoom, ["Left"], ["Left"];
    ZoomOut: "zoom.out", "mouse.zoom.out", Zoom, ["Right"], ["Alt+Left"];
    ZoomFit: "hand.double_fit", "mouse.hand.double_fit", ToolbarHand, [], ["DoubleLeft"];
    ZoomActual: "zoom.double_actual", "mouse.zoom.double_actual", ToolbarZoom, [], ["DoubleLeft"];
    LayerProperties: "layer.mouse_properties", "mouse.layer.mouse_properties", LayerRow, ["DoubleLeft"], [];
    LayerUp: "layer.mouse_move_up", "mouse.layer.mouse_move_up", LayerUpButton, ["Left"], ["Left"];
    LayerTop: "layer.mouse_move_top", "mouse.layer.mouse_move_top", LayerUpButton, ["Ctrl+Left"], [];
    LayerDown: "layer.mouse_move_down", "mouse.layer.mouse_move_down", LayerDownButton, ["Left"], ["Left"];
    LayerBottom: "layer.mouse_move_bottom", "mouse.layer.mouse_move_bottom", LayerDownButton, ["Ctrl+Left"], [];
    DrawPrimary: "brush.primary", "mouse.brush.primary", Brush, ["Left"], ["Left"];
    DrawSecondary: "brush.secondary", "mouse.brush.secondary", Brush, ["Right"], [];
    BrushSampleColor: "brush.sample_color", "mouse.brush.sample_color", Brush, ["Ctrl+Left", "Ctrl+Right"], ["Alt+Left"];
    PencilPrimary: "pencil.primary", "mouse.pencil.primary", Pencil, ["Left"], ["Left"];
    PencilSecondary: "pencil.secondary", "mouse.pencil.secondary", Pencil, ["Right"], [];
    BucketPrimary: "bucket.primary", "mouse.bucket.primary", Bucket, ["Left"], ["Left"];
    BucketSecondary: "bucket.secondary", "mouse.bucket.secondary", Bucket, ["Right"], [];
    BucketGlobalPrimary: "bucket.global_primary", "mouse.bucket.global_primary", Bucket, ["Shift+Left"], [];
    BucketGlobalSecondary: "bucket.global_secondary", "mouse.bucket.global_secondary", Bucket, ["Shift+Right"], [];
    BrushOptions: "brush.options", "mouse.brush.options", Brush, [], ["Right"];
    Erase: "eraser.erase", "mouse.eraser.erase", Eraser, ["Left", "Right"], ["Left"];
    EraserOptions: "eraser.options", "mouse.eraser.options", Eraser, [], ["Right"];
    EyedropperPrimary: "eyedropper.primary", "mouse.eyedropper.primary", Eyedropper, ["Left"], ["Left"];
    EyedropperSecondary: "eyedropper.secondary", "mouse.eyedropper.secondary", Eyedropper, ["Right"], ["Shift+Left"];
    EyedropperComposite: "eyedropper.composite", "mouse.eyedropper.composite", Eyedropper, ["Ctrl+Left", "Ctrl+Right"], ["Alt+Left"];
    ClonePaint: "clone.paint", "mouse.clone.paint", CloneStamp, ["Left", "Right"], ["Left"];
    CloneSetAnchor: "clone.set_anchor", "mouse.clone.set_anchor", CloneStamp, ["Ctrl+Left"], ["Alt+Left"];
    RecolorPrimary: "recolor.primary", "mouse.recolor.primary", Recolor, ["Left"], ["Left"];
    RecolorSecondary: "recolor.secondary", "mouse.recolor.secondary", Recolor, ["Right"], [];
    RecolorSample: "recolor.sample", "mouse.recolor.sample", Recolor, ["Ctrl+Left", "Ctrl+Right"], ["Alt+Left"];
    GradientPrimary: "gradient.primary", "mouse.gradient.primary", Gradient, ["Left"], ["Left"];
    GradientSecondary: "gradient.secondary", "mouse.gradient.secondary", Gradient, ["Right"], [];
    LinePrimary: "line.primary", "mouse.line.primary", LineCurve, ["Left"], ["Left"];
    LineSecondary: "line.secondary", "mouse.line.secondary", LineCurve, ["Right"], [];
    ShapePrimary: "shape.primary", "mouse.shape.primary", ShapeTool, ["Left"], ["Left"];
    ShapeSecondary: "shape.secondary", "mouse.shape.secondary", ShapeTool, ["Right"], [];
    TextPlace: "text.place", "mouse.text.place", Text, ["Left"], ["Left"];
    ZoomMarquee: "zoom.marquee", "mouse.zoom.marquee", ZoomDrag, ["Left"], [];
    ZoomScrubby: "zoom.scrubby", "mouse.zoom.scrubby", ZoomDrag, [], ["Left"];
    Select: "selection.create", "mouse.selection.create", Selection, ["Left", "Right"], ["Left"];
    SelectionRelocate: "selection.relocate", "mouse.selection.relocate", SelectionDrag, ["Right"], ["Space+Left"];
    SelectSquare: "selection.constrain", "mouse.selection.constrain", Selection, ["Shift+Left", "Shift+Right"], [];
    SelectAdd: "selection.add", "mouse.selection.add", Selection, ["Ctrl+Left"], ["Shift+Left"];
    SelectSubtract: "selection.subtract", "mouse.selection.subtract", Selection, ["Alt+Left"], ["Alt+Left"];
    SelectXor: "selection.xor", "mouse.selection.xor", Selection, ["Ctrl+Right"], [];
    SelectIntersect: "selection.intersect", "mouse.selection.intersect", Selection, ["Alt+Right"], ["Alt+Shift+Left"];
    Wand: "wand.sample", "mouse.wand.sample", Wand, ["Left"], ["Left"];
    WandAdd: "wand.add", "mouse.wand.add", Wand, ["Ctrl+Left"], ["Shift+Left"];
    WandSubtract: "wand.subtract", "mouse.wand.subtract", Wand, ["Alt+Left"], ["Alt+Left"];
    WandXor: "wand.xor", "mouse.wand.xor", Wand, ["Ctrl+Right"], [];
    WandIntersect: "wand.intersect", "mouse.wand.intersect", Wand, ["Alt+Right"], ["Alt+Shift+Left"];
    WandGlobal: "wand.global", "mouse.wand.global", Wand, ["Shift+Left"], [];
    WandGlobalAdd: "wand.global_add", "mouse.wand.global_add", Wand, ["Ctrl+Shift+Left"], [];
    WandGlobalSubtract: "wand.global_subtract", "mouse.wand.global_subtract", Wand, ["Alt+Shift+Left"], [];
    WandGlobalXor: "wand.global_xor", "mouse.wand.global_xor", Wand, ["Ctrl+Shift+Right"], [];
    WandGlobalIntersect: "wand.global_intersect", "mouse.wand.global_intersect", Wand, ["Alt+Shift+Right"], [];
    ColorPrimary: "colors.primary", "mouse.colors.primary", ColorWheel, ["Left"], ["Left"];
    ColorSecondary: "colors.secondary", "mouse.colors.secondary", ColorWheel, ["Right"], [];
    ColorHue: "colors.hue_only", "mouse.colors.hue_only", ColorWheel, ["Ctrl+Left"], [];
    ColorSaturation: "colors.saturation_only", "mouse.colors.saturation_only", ColorWheel, ["Alt+Left"], [];
    ColorSnap: "colors.snap_hue", "mouse.colors.snap_hue", ColorWheel, ["Shift+Left"], [];
    ColorSnapHue: "colors.snap_hue_only", "mouse.colors.snap_hue_only", ColorWheel, ["Ctrl+Shift+Left"], [];
    Move: "transform.move", "mouse.transform.move", TransformMove, ["Left"], ["Left"];
    MoveCopy: "transform.move_copy", "mouse.transform.move_copy", TransformMove, ["Ctrl+Left"], ["Alt+Left"];
    Scale: "transform.scale", "mouse.transform.scale", TransformScale, ["Left"], ["Left"];
    ScaleProportional: "transform.scale_proportional", "mouse.transform.scale_proportional", TransformScale, ["Shift+Left"], ["Shift+Left"];
    ScaleCentered: "transform.scale_centered", "mouse.transform.scale_centered", TransformScale, ["Alt+Left"], ["Alt+Left"];
    ScaleBoth: "transform.scale_centered_proportional", "mouse.transform.scale_centered_proportional", TransformScale, ["Alt+Shift+Left"], ["Alt+Shift+Left"];
    Rotate: "transform.rotate", "mouse.transform.rotate", TransformRotate, ["Left", "Right"], ["Left"];
    RotateSnap: "transform.rotate_snap", "mouse.transform.rotate_snap", TransformRotate, ["Shift+Left", "Shift+Right"], ["Shift+Left"];
    RotateCopy: "transform.rotate_copy", "mouse.transform.rotate_copy", TransformRotate, ["Ctrl+Left", "Ctrl+Right"], [];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Trigger {
    Button(PointerButton),
    Double(PointerButton),
    Wheel,
    WheelX,
}

impl std::hash::Hash for Trigger {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let btn_code = |b: PointerButton| match b {
            PointerButton::Primary => 0u8,
            PointerButton::Secondary => 1,
            PointerButton::Middle => 2,
            PointerButton::Extra1 => 3,
            PointerButton::Extra2 => 4,
        };
        match *self {
            Self::Button(b) => {
                0u8.hash(state);
                btn_code(b).hash(state);
            }
            Self::Double(b) => {
                1u8.hash(state);
                btn_code(b).hash(state);
            }
            Self::Wheel => 2u8.hash(state),
            Self::WheelX => 3u8.hash(state),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Gesture {
    ctrl: bool,
    alt: bool,
    shift: bool,
    pub held: Option<Key>,
    pub trigger: Trigger,
}

fn button(name: &str) -> Option<PointerButton> {
    match name.to_ascii_lowercase().as_str() {
        "left" => Some(PointerButton::Primary),
        "right" => Some(PointerButton::Secondary),
        "middle" => Some(PointerButton::Middle),
        "back" => Some(PointerButton::Extra1),
        "forward" => Some(PointerButton::Extra2),
        _ => None,
    }
}

impl Gesture {
    fn parse(value: &str) -> Option<Self> {
        let (modifiers, trigger) = value.trim().rsplit_once('+').unwrap_or(("", value.trim()));
        let trigger = trigger.trim();
        let trigger = match trigger.to_ascii_lowercase().as_str() {
            "wheel" => Trigger::Wheel,
            "wheelx" => Trigger::WheelX,
            value if value.starts_with("double") => Trigger::Double(button(&value[6..])?),
            value => Trigger::Button(button(value)?),
        };
        let mut gesture = Self {
            ctrl: false,
            alt: false,
            shift: false,
            held: None,
            trigger,
        };
        for token in modifiers
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
        {
            match token.to_ascii_lowercase().as_str() {
                "ctrl" if !gesture.ctrl => gesture.ctrl = true,
                "alt" if !gesture.alt => gesture.alt = true,
                "shift" if !gesture.shift => gesture.shift = true,
                _ if gesture.held.is_none() => {
                    gesture.held = Some(Key::from_name(token).or_else(|| {
                        Key::ALL
                            .iter()
                            .copied()
                            .find(|key| key.name().eq_ignore_ascii_case(token))
                    })?);
                }
                _ => return None,
            }
        }
        Some(gesture)
    }

    fn matches(self, trigger: Trigger, modifiers: Modifiers) -> bool {
        self.trigger == trigger
            && self.ctrl == modifiers.ctrl
            && self.alt == modifiers.alt
            && self.shift == modifiers.shift
    }
}

#[derive(Default)]
pub struct MouseBindings(HashMap<(Scope, Gesture), MouseAction>);

impl MouseBindings {
    pub fn compile(values: &BTreeMap<String, Vec<String>>) -> Result<Self, String> {
        let mut bindings = HashMap::new();
        for &action in MouseAction::ALL {
            for value in values.get(action.id()).into_iter().flatten() {
                let gesture = Gesture::parse(value).ok_or_else(|| {
                    super::i18n::format_logical(
                        "invalid-gesture",
                        &[&super::i18n::logical_text(action.label_key()), &value],
                    )
                })?;
                let scope = action.scope();
                let supported = match scope {
                    Scope::ToolbarHand
                    | Scope::ToolbarZoom
                    | Scope::LayerRow
                    | Scope::CanvasCorner => {
                        matches!(gesture.trigger, Trigger::Double(_))
                    }
                    Scope::Canvas
                        if matches!(
                            action,
                            MouseAction::WheelVertical
                                | MouseAction::WheelHorizontal
                                | MouseAction::WheelZoom
                                | MouseAction::WheelStrokeSize
                        ) =>
                    {
                        matches!(gesture.trigger, Trigger::Wheel | Trigger::WheelX)
                    }
                    _ => matches!(gesture.trigger, Trigger::Button(_)),
                };
                if !supported {
                    return Err(super::i18n::format_logical(
                        "unsupported-gesture",
                        &[&super::i18n::logical_text(action.label_key()), &value],
                    ));
                }
                for (&(other_scope, other_gesture), &other) in &bindings {
                    if other_gesture == gesture
                        && other != action
                        && scope.conflicts_with(other_scope)
                    {
                        let other: MouseAction = other;
                        return Err(super::i18n::format_logical(
                            "gesture-conflict",
                            &[
                                &value,
                                &super::i18n::logical_text(other.label_key()),
                                &super::i18n::logical_text(action.label_key()),
                            ],
                        ));
                    }
                }
                bindings.insert((scope, gesture), action);
            }
        }
        Ok(Self(bindings))
    }

    pub fn resolve(
        &self,
        scope: Scope,
        trigger: Trigger,
        modifiers: Modifiers,
        mut down: impl FnMut(Key) -> bool,
    ) -> Option<MouseAction> {
        self.0
            .iter()
            .filter(|&(&(registered, gesture), _)| {
                scope == registered
                    && gesture.matches(trigger, modifiers)
                    && gesture.held.is_none_or(&mut down)
            })
            .max_by_key(|&(&(_, gesture), _)| (gesture.held.is_some(), gesture.held))
            .map(|(_, &action)| action)
    }

    pub fn is_gesture_down(&self, action: MouseAction, input: &egui::InputState) -> bool {
        let scope = action.scope();
        self.0.iter().any(|(&(registered, gesture), &act)| {
            registered == scope
                && act == action
                && match gesture.trigger {
                    Trigger::Button(btn) | Trigger::Double(btn) => input.pointer.button_down(btn),
                    Trigger::Wheel | Trigger::WheelX => false,
                }
                && gesture.held.is_none_or(|key| input.key_down(key))
                && (!gesture.ctrl || input.modifiers.ctrl)
                && (!gesture.alt || input.modifiers.alt)
                && (!gesture.shift || input.modifiers.shift)
        })
    }

    pub fn click(
        &self,
        response: &egui::Response,
        scope: Scope,
        double: bool,
    ) -> Option<MouseAction> {
        let modifiers = response.ctx.input(|input| input.modifiers);
        BUTTONS.into_iter().find_map(|button| {
            let trigger = if double && response.double_clicked_by(button) {
                Trigger::Double(button)
            } else if !double && response.clicked_by(button) {
                Trigger::Button(button)
            } else {
                return None;
            };
            self.at(&response.ctx, scope, trigger, modifiers)
        })
    }

    pub fn at(
        &self,
        ctx: &egui::Context,
        scope: Scope,
        trigger: Trigger,
        modifiers: Modifiers,
    ) -> Option<MouseAction> {
        ctx.input(|input| self.resolve(scope, trigger, modifiers, |key| input.key_down(key)))
    }
}

pub fn add_missing(values: &mut BTreeMap<String, Vec<String>>, base: Base) {
    for &action in MouseAction::ALL {
        if values.contains_key(action.id()) {
            continue;
        }
        values.insert(action.id().into(), Vec::new());
        for &gesture in action.defaults(base) {
            values.get_mut(action.id()).unwrap().push(gesture.into());
            if MouseBindings::compile(values).is_err() {
                values.get_mut(action.id()).unwrap().pop();
            }
        }
    }
}

pub fn settings_rows(ui: &mut egui::Ui, values: &mut BTreeMap<String, Vec<String>>, base: Base) {
    for &action in MouseAction::ALL {
        ui.label(egui::RichText::new(action.label()).small());
        let bindings = values.get_mut(action.id()).expect("gesto conhecido");
        let mut text = bindings.join(";");
        if ui
            .add_sized(
                [172.0, 19.0],
                egui::TextEdit::singleline(&mut text).id_salt(action.id()),
            )
            .changed()
        {
            *bindings = text
                .split(';')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect();
        }
        if ui.small_button(tr("preset-default")).clicked() {
            *bindings = action
                .defaults(base)
                .iter()
                .map(ToString::to_string)
                .collect();
        }
        ui.end_row();
    }
}

pub fn clean_empty(values: &mut BTreeMap<String, Vec<String>>) {
    for &action in MouseAction::ALL {
        if let Some(bindings) = values.get_mut(action.id()) {
            bindings.retain(|value| !value.trim().is_empty());
        }
    }
}

pub const BUTTONS: [PointerButton; 5] = [
    PointerButton::Primary,
    PointerButton::Secondary,
    PointerButton::Middle,
    PointerButton::Extra1,
    PointerButton::Extra2,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_mouse_profiles_compile_and_resolve_key_gestures() -> Result<(), String> {
        for base in [Base::PaintNet, Base::Adobe] {
            let mut values = BTreeMap::new();
            add_missing(&mut values, base);
            let bindings = MouseBindings::compile(&values)?;
            assert_eq!(
                bindings.resolve(
                    Scope::Canvas,
                    Trigger::Wheel,
                    Modifiers::CTRL | Modifiers::SHIFT,
                    |_| false,
                ),
                Some(MouseAction::WheelStrokeSize)
            );
            assert_eq!(
                bindings.resolve(Scope::Canvas, Trigger::Wheel, Modifiers::CTRL, |_| false),
                Some(if base == Base::PaintNet {
                    MouseAction::WheelZoom
                } else {
                    MouseAction::WheelHorizontal
                })
            );
            assert_eq!(
                bindings.resolve(Scope::Canvas, Trigger::Wheel, Modifiers::ALT, |_| false),
                (base == Base::Adobe).then_some(MouseAction::WheelZoom)
            );
            assert_eq!(
                bindings.resolve(
                    Scope::LayerUpButton,
                    Trigger::Button(PointerButton::Primary),
                    Modifiers::CTRL,
                    |_| false
                ),
                (base == Base::PaintNet).then_some(MouseAction::LayerTop),
            );
            assert_eq!(
                bindings.resolve(
                    Scope::Canvas,
                    Trigger::Button(PointerButton::Primary),
                    Modifiers::NONE,
                    |k| k == Key::Space
                ),
                Some(MouseAction::Pan)
            );
        }
        Ok(())
    }

    #[test]
    fn new_wheel_binding_yields_to_an_existing_custom_gesture() -> Result<(), String> {
        let mut values = BTreeMap::from([(
            MouseAction::WheelZoom.id().to_owned(),
            vec!["Ctrl+Shift+Wheel".to_owned()],
        )]);
        add_missing(&mut values, Base::PaintNet);
        assert!(values[MouseAction::WheelStrokeSize.id()].is_empty());
        values.insert(
            MouseAction::WheelStrokeSize.id().into(),
            vec!["Alt+WheelX".into()],
        );
        let bindings = MouseBindings::compile(&values)?;
        assert_eq!(
            bindings.resolve(Scope::Canvas, Trigger::WheelX, Modifiers::ALT, |_| false),
            Some(MouseAction::WheelStrokeSize)
        );
        assert_eq!(
            bindings.resolve(
                Scope::Canvas,
                Trigger::Wheel,
                Modifiers::CTRL | Modifiers::SHIFT,
                |_| false
            ),
            Some(MouseAction::WheelZoom)
        );
        Ok(())
    }
}
