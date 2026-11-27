use std::collections::{BTreeMap, HashMap};

use eframe::egui::{self, Key, Modifiers, PointerButton};

use super::shortcuts::{
    Base, ModFlags, backfill_missing, bindable_actions, binding_conflict_error, binding_editor_row,
    binding_value_error, parse_key,
};

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
    ColorRibbonSwatch,
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
        bindable_actions! {
            #[derive(Clone, Copy, Debug, Eq, PartialEq)]
            pub enum MouseAction {
                $($action : $id, $label, [$($paint),*], [$($adobe),*];)*
            }
        }

        impl MouseAction {
            pub fn scope(self) -> Scope {
                match self { $(Self::$action => Scope::$scope),* }
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
    RibbonSwatchPrimary: "colors.ribbon_primary", "mouse.colors.ribbon_primary", ColorRibbonSwatch, ["Left"], ["Left"];
    RibbonSwatchSecondary: "colors.ribbon_secondary", "mouse.colors.ribbon_secondary", ColorRibbonSwatch, ["Right"], ["Alt+Left"];
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

// `PointerButton` does not implement `Hash`, so hash its discriminant.
impl std::hash::Hash for Trigger {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        if let Self::Button(button) | Self::Double(button) = *self {
            (button as u8).hash(state);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Gesture {
    mods: ModFlags,
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
            mods: ModFlags::default(),
            held: None,
            trigger,
        };
        for token in modifiers
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
        {
            if gesture.mods.apply_token(token) {
                continue;
            }
            if gesture.held.is_none() {
                gesture.held = Some(parse_key(token)?);
            } else {
                return None;
            }
        }
        Some(gesture)
    }

    fn matches(self, trigger: Trigger, modifiers: Modifiers) -> bool {
        self.trigger == trigger && self.mods.matches(modifiers)
    }
}

#[derive(Default)]
pub struct MouseBindings(HashMap<(Scope, Gesture), MouseAction>);

impl MouseBindings {
    pub fn compile(values: &BTreeMap<String, Vec<String>>) -> Result<Self, String> {
        let mut bindings = HashMap::new();
        for &action in MouseAction::ALL {
            for value in values.get(action.id()).into_iter().flatten() {
                let gesture = Gesture::parse(value)
                    .ok_or_else(|| binding_value_error("invalid-gesture", action, value))?;
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
                    return Err(binding_value_error("unsupported-gesture", action, value));
                }
                for (&(other_scope, other_gesture), &other) in &bindings {
                    if other_gesture == gesture
                        && other != action
                        && scope.conflicts_with(other_scope)
                    {
                        return Err(binding_conflict_error(
                            "gesture-conflict",
                            value,
                            other,
                            action,
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
        down: impl FnMut(Key) -> bool,
    ) -> Option<MouseAction> {
        self.resolve_gesture(scope, trigger, modifiers, down)
            .map(|(_, action)| action)
    }

    fn resolve_gesture(
        &self,
        scope: Scope,
        trigger: Trigger,
        modifiers: Modifiers,
        mut down: impl FnMut(Key) -> bool,
    ) -> Option<(Gesture, MouseAction)> {
        self.0
            .iter()
            .filter(|&(&(registered, gesture), _)| {
                scope == registered
                    && gesture.matches(trigger, modifiers)
                    && gesture.held.is_none_or(&mut down)
            })
            .max_by_key(|&(&(_, gesture), _)| (gesture.held.is_some(), gesture.held))
            .map(|(&(_, gesture), &action)| (gesture, action))
    }

    pub fn wheel_chord_down(&self, action: MouseAction, input: &egui::InputState) -> bool {
        [Trigger::Wheel, Trigger::WheelX]
            .into_iter()
            .any(|trigger| {
                self.resolve_gesture(action.scope(), trigger, input.modifiers, |key| {
                    input.key_down(key)
                })
                .is_some_and(|(gesture, resolved)| {
                    resolved == action && (gesture.mods.any() || gesture.held.is_some())
                })
            })
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
                && gesture.mods.all_held(input.modifiers)
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

    pub fn active_button(ctx: &egui::Context) -> Option<PointerButton> {
        ctx.input(|input| {
            BUTTONS
                .into_iter()
                .find(|&btn| input.pointer.button_down(btn) || input.pointer.button_pressed(btn))
        })
    }

    pub fn active_match<T>(
        &self,
        ctx: &egui::Context,
        allow_held: bool,
        mut pick: impl FnMut(&Self, &egui::InputState, PointerButton) -> Option<T>,
    ) -> Option<T> {
        ctx.input(|input| {
            BUTTONS.into_iter().find_map(|btn| {
                let active = input.pointer.button_pressed(btn)
                    || (allow_held && input.pointer.button_down(btn));
                active.then(|| pick(self, input, btn))?
            })
        })
    }

    fn button_in(
        &self,
        ctx: &egui::Context,
        scopes: &[Scope],
        double: bool,
        allow_held: bool,
    ) -> Option<(PointerButton, MouseAction)> {
        ctx.input(|input| {
            BUTTONS.into_iter().find_map(|btn| {
                let active = if double {
                    input.pointer.button_double_clicked(btn)
                } else {
                    input.pointer.button_pressed(btn)
                        || (allow_held && input.pointer.button_down(btn))
                };
                if !active {
                    return None;
                }
                let trigger = if double {
                    Trigger::Double(btn)
                } else {
                    Trigger::Button(btn)
                };
                scopes.iter().find_map(|&scope| {
                    self.resolve(scope, trigger, input.modifiers, |k| input.key_down(k))
                        .map(|action| (btn, action))
                })
            })
        })
    }

    pub fn pressed_in(
        &self,
        ctx: &egui::Context,
        scope: Scope,
    ) -> Option<(PointerButton, MouseAction)> {
        self.button_in(ctx, &[scope], false, false)
    }

    pub fn pressed_in_any(
        &self,
        ctx: &egui::Context,
        scopes: &[Scope],
    ) -> Option<(PointerButton, MouseAction)> {
        self.button_in(ctx, scopes, false, false)
    }

    pub fn double_clicked_in(
        &self,
        ctx: &egui::Context,
        scope: Scope,
    ) -> Option<(PointerButton, MouseAction)> {
        self.button_in(ctx, &[scope], true, false)
    }

    pub fn active_in(
        &self,
        ctx: &egui::Context,
        scope: Scope,
    ) -> Option<(PointerButton, MouseAction)> {
        self.button_in(ctx, &[scope], false, true)
    }
}

pub fn add_missing(values: &mut BTreeMap<String, Vec<String>>, base: Base) {
    backfill_missing::<MouseAction>(values, base, |bindings| {
        MouseBindings::compile(bindings).is_ok()
    });
}

pub fn settings_rows(ui: &mut egui::Ui, values: &mut BTreeMap<String, Vec<String>>, base: Base) {
    for &action in MouseAction::ALL {
        let bindings = values.get_mut(action.id()).expect("known gesture");
        binding_editor_row(
            ui,
            egui::RichText::new(action.label()).small(),
            action.id(),
            172.0,
            bindings,
            action.defaults(base),
        );
    }
}

pub fn clean_empty(values: &mut BTreeMap<String, Vec<String>>) {
    for &action in MouseAction::ALL {
        if let Some(bindings) = values.get_mut(action.id()) {
            bindings.retain(|value| !value.trim().is_empty());
        }
    }
}

const BUTTONS: [PointerButton; 5] = [
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
            let (button, modifiers) = match base {
                Base::PaintNet => (PointerButton::Secondary, Modifiers::NONE),
                Base::Adobe => (PointerButton::Primary, Modifiers::ALT),
            };
            assert_eq!(
                bindings.resolve(
                    Scope::ColorRibbonSwatch,
                    Trigger::Button(button),
                    modifiers,
                    |_| false
                ),
                Some(MouseAction::RibbonSwatchSecondary)
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
