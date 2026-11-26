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
    Pan: "canvas.pan", "Tela · panorâmica temporária", Canvas, ["Middle", "Space+Left", "Space+Right"], ["Space+Left"];
    CanvasCornerResize: "canvas.corner_resize", "Canto inferior direito · redimensionar tela", CanvasCorner, ["Ctrl+DoubleLeft"], ["Ctrl+DoubleLeft"];
    WheelVertical: "canvas.scroll_vertical", "Tela · rolar verticalmente", Canvas, ["Wheel"], ["Wheel"];
    WheelHorizontal: "canvas.scroll_horizontal", "Tela · rolar horizontalmente", Canvas, ["Shift+Wheel", "WheelX"], ["Ctrl+Wheel", "WheelX"];
    WheelZoom: "canvas.wheel_zoom", "Tela · zoom pela roda", Canvas, ["Ctrl+Wheel"], ["Alt+Wheel"];
    WheelStrokeSize: "canvas.wheel_stroke_size", "Tela · espessura do traço pela roda", Canvas, ["Ctrl+Shift+Wheel"], ["Ctrl+Shift+Wheel"];
    TempZoomIn: "canvas.temp_zoom_in", "Tela · ampliar temporariamente", Canvas, [], ["Ctrl+Space+Left"];
    TempZoomOut: "canvas.temp_zoom_out", "Tela · reduzir temporariamente", Canvas, [], ["Alt+Space+Left"];
    Hand: "hand.pan", "Mão · mover a tela", Hand, ["Left", "Right"], ["Left"];
    ZoomIn: "zoom.in", "Zoom · ampliar", Zoom, ["Left"], ["Left"];
    ZoomOut: "zoom.out", "Zoom · reduzir", Zoom, ["Right"], ["Alt+Left"];
    ZoomFit: "hand.double_fit", "Ícone da Mão · ajustar à janela", ToolbarHand, [], ["DoubleLeft"];
    ZoomActual: "zoom.double_actual", "Ícone do Zoom · tamanho real", ToolbarZoom, [], ["DoubleLeft"];
    LayerProperties: "layer.mouse_properties", "Linha da camada · abrir propriedades", LayerRow, ["DoubleLeft"], [];
    LayerUp: "layer.mouse_move_up", "Botão subir camada · subir um nível", LayerUpButton, ["Left"], ["Left"];
    LayerTop: "layer.mouse_move_top", "Botão subir camada · mover ao topo", LayerUpButton, ["Ctrl+Left"], [];
    LayerDown: "layer.mouse_move_down", "Botão descer camada · descer um nível", LayerDownButton, ["Left"], ["Left"];
    LayerBottom: "layer.mouse_move_bottom", "Botão descer camada · mover à base", LayerDownButton, ["Ctrl+Left"], [];
    DrawPrimary: "brush.primary", "Pincel · cor primária", Brush, ["Left"], ["Left"];
    DrawSecondary: "brush.secondary", "Pincel · cor secundária", Brush, ["Right"], [];
    BrushSampleColor: "brush.sample_color", "Pincel · amostrar cor temporariamente", Brush, ["Ctrl+Left", "Ctrl+Right"], ["Alt+Left"];
    PencilPrimary: "pencil.primary", "Lápis · cor primária", Pencil, ["Left"], ["Left"];
    PencilSecondary: "pencil.secondary", "Lápis · cor secundária", Pencil, ["Right"], [];
    BucketPrimary: "bucket.primary", "Balde · preencher com a cor primária", Bucket, ["Left"], ["Left"];
    BucketSecondary: "bucket.secondary", "Balde · preencher com a cor secundária", Bucket, ["Right"], [];
    BucketGlobalPrimary: "bucket.global_primary", "Balde global · cor primária", Bucket, ["Shift+Left"], [];
    BucketGlobalSecondary: "bucket.global_secondary", "Balde global · cor secundária", Bucket, ["Shift+Right"], [];
    BrushOptions: "brush.options", "Pincel · opções de tamanho", Brush, [], ["Right"];
    Erase: "eraser.erase", "Borracha · apagar", Eraser, ["Left", "Right"], ["Left"];
    EraserOptions: "eraser.options", "Borracha · opções de tamanho", Eraser, [], ["Right"];
    EyedropperPrimary: "eyedropper.primary", "Conta-gotas · definir cor primária", Eyedropper, ["Left"], ["Left"];
    EyedropperSecondary: "eyedropper.secondary", "Conta-gotas · definir cor secundária", Eyedropper, ["Right"], ["Shift+Left"];
    EyedropperComposite: "eyedropper.composite", "Conta-gotas · amostrar composição", Eyedropper, ["Ctrl+Left", "Ctrl+Right"], ["Alt+Left"];
    ClonePaint: "clone.paint", "Carimbo · pintar da origem", CloneStamp, ["Left", "Right"], ["Left"];
    CloneSetAnchor: "clone.set_anchor", "Carimbo · definir origem", CloneStamp, ["Ctrl+Left"], ["Alt+Left"];
    RecolorPrimary: "recolor.primary", "Recolorir · substituir para primária", Recolor, ["Left"], ["Left"];
    RecolorSecondary: "recolor.secondary", "Recolorir · substituir para secundária", Recolor, ["Right"], [];
    RecolorSample: "recolor.sample", "Recolorir · amostrar cor-alvo", Recolor, ["Ctrl+Left", "Ctrl+Right"], ["Alt+Left"];
    GradientPrimary: "gradient.primary", "Gradiente · primária para secundária", Gradient, ["Left"], ["Left"];
    GradientSecondary: "gradient.secondary", "Gradiente · secundária para primária", Gradient, ["Right"], [];
    LinePrimary: "line.primary", "Linha/Curva · cor primária", LineCurve, ["Left"], ["Left"];
    LineSecondary: "line.secondary", "Linha/Curva · cor secundária", LineCurve, ["Right"], [];
    ShapePrimary: "shape.primary", "Formas · cor primária", ShapeTool, ["Left"], ["Left"];
    ShapeSecondary: "shape.secondary", "Formas · cor secundária", ShapeTool, ["Right"], [];
    TextPlace: "text.place", "Texto · posicionar cursor", Text, ["Left"], ["Left"];
    ZoomMarquee: "zoom.marquee", "Zoom · arrastar região", ZoomDrag, ["Left"], [];
    ZoomScrubby: "zoom.scrubby", "Zoom · arrastar contínuo", ZoomDrag, [], ["Left"];
    Select: "selection.create", "Seleção · modo atual", Selection, ["Left", "Right"], ["Left"];
    SelectionRelocate: "selection.relocate", "Seleção · mover contorno durante criação", SelectionDrag, ["Right"], ["Space+Left"];
    SelectSquare: "selection.constrain", "Seleção · quadrado ou círculo", Selection, ["Shift+Left", "Shift+Right"], [];
    SelectAdd: "selection.add", "Seleção · adicionar", Selection, ["Ctrl+Left"], ["Shift+Left"];
    SelectSubtract: "selection.subtract", "Seleção · subtrair", Selection, ["Alt+Left"], ["Alt+Left"];
    SelectXor: "selection.xor", "Seleção · XOR", Selection, ["Ctrl+Right"], [];
    SelectIntersect: "selection.intersect", "Seleção · intersectar", Selection, ["Alt+Right"], ["Alt+Shift+Left"];
    Wand: "wand.sample", "Varinha · modo atual", Wand, ["Left"], ["Left"];
    WandAdd: "wand.add", "Varinha · adicionar", Wand, ["Ctrl+Left"], ["Shift+Left"];
    WandSubtract: "wand.subtract", "Varinha · subtrair", Wand, ["Alt+Left"], ["Alt+Left"];
    WandXor: "wand.xor", "Varinha · XOR", Wand, ["Ctrl+Right"], [];
    WandIntersect: "wand.intersect", "Varinha · intersectar", Wand, ["Alt+Right"], ["Alt+Shift+Left"];
    WandGlobal: "wand.global", "Varinha global · modo atual", Wand, ["Shift+Left"], [];
    WandGlobalAdd: "wand.global_add", "Varinha global · adicionar", Wand, ["Ctrl+Shift+Left"], [];
    WandGlobalSubtract: "wand.global_subtract", "Varinha global · subtrair", Wand, ["Alt+Shift+Left"], [];
    WandGlobalXor: "wand.global_xor", "Varinha global · XOR", Wand, ["Ctrl+Shift+Right"], [];
    WandGlobalIntersect: "wand.global_intersect", "Varinha global · intersectar", Wand, ["Alt+Shift+Right"], [];
    ColorPrimary: "colors.primary", "Roda de cores · cor primária", ColorWheel, ["Left"], ["Left"];
    ColorSecondary: "colors.secondary", "Roda de cores · cor secundária", ColorWheel, ["Right"], [];
    ColorHue: "colors.hue_only", "Roda de cores · somente matiz", ColorWheel, ["Ctrl+Left"], [];
    ColorSaturation: "colors.saturation_only", "Roda de cores · somente saturação", ColorWheel, ["Alt+Left"], [];
    ColorSnap: "colors.snap_hue", "Roda de cores · matiz em passos de 15°", ColorWheel, ["Shift+Left"], [];
    ColorSnapHue: "colors.snap_hue_only", "Roda de cores · somente matiz em passos de 15°", ColorWheel, ["Ctrl+Shift+Left"], [];
    Move: "transform.move", "Transformação · mover", TransformMove, ["Left"], ["Left"];
    MoveCopy: "transform.move_copy", "Transformação · mover uma cópia", TransformMove, ["Ctrl+Left"], ["Alt+Left"];
    Scale: "transform.scale", "Alça · redimensionar", TransformScale, ["Left"], ["Left"];
    ScaleProportional: "transform.scale_proportional", "Alça · preservar proporção", TransformScale, ["Shift+Left"], ["Shift+Left"];
    ScaleCentered: "transform.scale_centered", "Alça · redimensionar pelo centro", TransformScale, ["Alt+Left"], ["Alt+Left"];
    ScaleBoth: "transform.scale_centered_proportional", "Alça · redimensionar pelo centro e preservar proporção", TransformScale, ["Alt+Shift+Left"], ["Alt+Shift+Left"];
    Rotate: "transform.rotate", "Alça de rotação · girar", TransformRotate, ["Left", "Right"], ["Left"];
    RotateSnap: "transform.rotate_snap", "Alça de rotação · girar em passos de 15°", TransformRotate, ["Shift+Left", "Shift+Right"], ["Shift+Left"];
    RotateCopy: "transform.rotate_copy", "Alça de rotação · girar uma cópia", TransformRotate, ["Ctrl+Left", "Ctrl+Right"], [];
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
                let gesture = Gesture::parse(value)
                    .ok_or_else(|| format!("{}: gesto inválido «{value}»", action.label()))?;
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
                    return Err(format!(
                        "{}: tipo de gesto incompatível «{value}»",
                        action.label()
                    ));
                }
                for (&(other_scope, other_gesture), &other) in &bindings {
                    if other_gesture == gesture
                        && other != action
                        && scope.conflicts_with(other_scope)
                    {
                        let other: MouseAction = other;
                        return Err(format!(
                            "«{value}» conflita entre «{}» e «{}».",
                            other.label(),
                            action.label()
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
        if ui.small_button("Padrão").clicked() {
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
