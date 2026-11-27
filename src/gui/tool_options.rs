//! Contextual toolbar options for the painting, selection and vector tools.

use super::*;

impl App {
    pub(super) fn gradient_params(&self) -> GradientParams {
        GradientParams {
            blend: self.stroke_blend,
            ..self.gradient
        }
    }

    pub(super) fn set_gradient_params(&mut self, p: GradientParams) {
        self.stroke_blend = p.blend;
        self.gradient = p;
    }

    pub(super) fn line_curve_params(&self) -> LineCurveParams {
        LineCurveParams {
            thickness: self.shape_thickness,
            blend: self.stroke_blend,
            ..self.line_curve
        }
    }

    pub(super) fn set_line_curve_params(&mut self, p: LineCurveParams) {
        self.shape_thickness = p.thickness;
        self.stroke_blend = p.blend;
        self.line_curve = p;
    }

    pub(super) fn shape_params(&self) -> ShapeParams {
        ShapeParams {
            thickness: self.shape_thickness,
            pattern: self.fill_pattern,
            blend: self.stroke_blend,
            ..self.shape
        }
    }

    pub(super) fn set_shape_params(&mut self, p: ShapeParams) {
        self.shape_thickness = p.thickness;
        self.fill_pattern = p.pattern;
        self.stroke_blend = p.blend;
        self.shape = p;
    }

    pub(super) fn text_params(&self) -> TextParams {
        TextParams {
            blend: self.stroke_blend,
            ..self.text.clone()
        }
    }

    pub(super) fn set_text_params(&mut self, p: TextParams) {
        self.stroke_blend = p.blend;
        self.text = p;
    }

    /// Applies `update` to the active session, if any, and schedules a full
    /// canvas refresh so its preview is re-rendered.
    pub(super) fn update_session(&mut self, update: impl FnOnce(&mut ActiveSession)) {
        let tab = self.tab_mut();
        if let Some(session) = tab.session.as_mut() {
            update(session);
        }
        tab.editor.mark_full_refresh();
    }

    /// Contextual options for the active tool when no editing session is open.
    pub(super) fn tool_options(&mut self, ui: &mut egui::Ui, compact: bool, wrap_wand: bool) {
        let theme = self.settings.theme;
        ui.label(theme.muted_label(tr("tool-field")));
        tool_icons::paint_small_tool(ui, self.tool);
        ui.label(theme.section_label(self.tool.label()));

        ui.spacing_mut().slider_width = if compact { 50.0 } else { 80.0 };
        match self.tool {
            Tool::Brush | Tool::Eraser | Tool::CloneStamp => {
                ui.add_space(4.0);
                self.brush_size_controls(ui, "size-field", "hardness-field");
                labeled_slider(ui, theme, "spacing-field", &mut self.spacing, 1..=200, "%");
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
                    ui.label(RichText::new(hint).size(11.5).color(theme.text_muted()));
                }
            }
            Tool::Pencil => {
                ui.add_space(6.0);
                labeled_slider(
                    ui,
                    theme,
                    "size-field",
                    &mut self.pencil_diameter,
                    1..=MAX_BRUSH_DIAMETER,
                    " px",
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
                self.brush_size_controls(ui, "short-size-field", "short-hardness-field");
                let target = self.recolor_target.unwrap_or(self.palette.secondary());
                let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                paint_color_swatch(
                    ui.painter(),
                    Some(&self.checker),
                    r,
                    target,
                    3.0,
                    egui::Stroke::new(1.0_f32, theme.border_color()),
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
                let mut params = self.gradient_params();
                if Self::gradient_controls(ui, &mut params) {
                    self.set_gradient_params(params);
                }
            }
            Tool::LineCurve => {
                let mut params = self.line_curve_params();
                if Self::line_curve_controls(ui, &mut params) {
                    self.set_line_curve_params(params);
                }
            }
            Tool::DrawShape => {
                let mut params = self.shape_params();
                if Self::shape_controls(ui, &mut params) {
                    self.set_shape_params(params);
                }
            }
            Tool::Text => {
                let mut params = self.text_params();
                if Self::text_controls(ui, &self.font_catalog.families, &mut params) {
                    self.set_text_params(params);
                }
            }
            Tool::Select(_) | Tool::Lasso | Tool::Wand => {
                ui.add_space(6.0);
                ui.label(theme.muted_label(tr("mode-field")));
                self.combine_selector(ui);
                labeled_slider(
                    ui,
                    theme,
                    "feather-field",
                    &mut self.selection_feather,
                    0..=100,
                    "%",
                )
                .on_hover_text(tr("feather-help"));
                if self.tool == Tool::Wand && !wrap_wand {
                    ui.add_space(4.0);
                    self.wand_options(ui);
                }
            }
            Tool::Pan | Tool::Zoom => {}
        }
    }

    /// Diameter and hardness sliders shared by the brush-like tools.
    fn brush_size_controls(&mut self, ui: &mut egui::Ui, size_key: &str, hardness_key: &str) {
        let theme = self.settings.theme;
        labeled_slider(
            ui,
            theme,
            size_key,
            &mut self.diameter,
            1..=MAX_BRUSH_DIAMETER,
            " px",
        );
        labeled_slider(ui, theme, hardness_key, &mut self.hardness, 0..=100, "%");
    }

    /// Tolerance-based options moved to a second toolbar row on narrow windows.
    pub(super) fn wrapped_tool_options(&mut self, ui: &mut egui::Ui) {
        match self.tool {
            Tool::Wand => self.wand_options(ui),
            Tool::Bucket => self.bucket_options(ui),
            Tool::Recolor => self.tolerance_options(ui, "recolor_alpha_wrap"),
            _ => {}
        }
    }

    pub(super) fn session_commit_buttons(&mut self, ui: &mut egui::Ui) {
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
        let theme = self.settings.theme;
        labeled_slider(
            ui,
            theme,
            "tolerance-field",
            &mut self.color_tolerance,
            0..=100,
            "%",
        );
        enum_combo(
            ui,
            id_salt,
            Some(125.0),
            &mut self.alpha_match,
            &AlphaMatchMode::ALL,
            AlphaMatchMode::label,
        );
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

    fn pattern_selector(ui: &mut egui::Ui, id_salt: &str, pattern: &mut FillPattern) -> bool {
        enum_combo(
            ui,
            id_salt,
            Some(110.0),
            pattern,
            &FillPattern::ALL,
            FillPattern::label,
        )
    }

    fn blend_selector(ui: &mut egui::Ui, id_salt: &str, blend: &mut StrokeBlend) -> bool {
        enum_combo(
            ui,
            id_salt,
            Some(90.0),
            blend,
            &StrokeBlend::ALL,
            StrokeBlend::label,
        )
    }

    pub(super) fn gradient_controls(ui: &mut egui::Ui, p: &mut GradientParams) -> bool {
        let mut changed = false;
        changed |= enum_combo(
            ui,
            "grad_shape",
            Some(115.0),
            &mut p.shape,
            &GradientShape::ALL,
            GradientShape::label,
        );
        changed |= enum_combo(
            ui,
            "grad_repeat",
            Some(96.0),
            &mut p.repeat,
            &GradientRepeat::ALL,
            GradientRepeat::label,
        );
        changed |= enum_combo(
            ui,
            "grad_channel",
            Some(98.0),
            &mut p.channel_mode,
            &GradientChannelMode::ALL,
            GradientChannelMode::label,
        );
        changed |= ui.checkbox(&mut p.dither, tr("dither")).changed();
        changed | Self::blend_selector(ui, "grad_blend", &mut p.blend)
    }

    pub(super) fn line_curve_controls(ui: &mut egui::Ui, p: &mut LineCurveParams) -> bool {
        let mut changed = false;
        changed |= enum_combo(
            ui,
            "curve_kind",
            Some(100.0),
            &mut p.kind,
            &CurveKind::ALL,
            CurveKind::label,
        );
        changed |= ui
            .add(egui::Slider::new(&mut p.thickness, 1..=MAX_OUTLINE_THICKNESS).suffix(" px"))
            .changed();
        changed |= enum_combo(
            ui,
            "dash_preset",
            Some(105.0),
            &mut p.dash,
            &DashPreset::ALL,
            DashPreset::label,
        );
        changed |= enum_combo(
            ui,
            "start_cap",
            Some(96.0),
            &mut p.start_cap,
            &LineCapStyle::ALL,
            LineCapStyle::label,
        );
        changed |= enum_combo(
            ui,
            "end_cap",
            Some(96.0),
            &mut p.end_cap,
            &LineCapStyle::ALL,
            LineCapStyle::label,
        );
        changed | Self::blend_selector(ui, "line_blend", &mut p.blend)
    }

    pub(super) fn shape_controls(ui: &mut egui::Ui, p: &mut ShapeParams) -> bool {
        let mut changed = false;
        let prev_k = p.kind;
        tool_icons::paint_shape_icon(ui, p.kind);
        egui::ComboBox::from_id_salt("shape_kind")
            .width(132.0)
            .selected_text(p.kind.label())
            .show_ui(ui, |ui| {
                for fam in ShapeFamily::ALL {
                    ui.label(RichText::new(fam.label()).strong().size(11.0));
                    for k in ShapeKind::ALL.iter().copied().filter(|k| k.family() == fam) {
                        ui.selectable_value(&mut p.kind, k, k.label());
                    }
                    ui.separator();
                }
            });
        changed |= p.kind != prev_k;
        changed |= enum_combo(
            ui,
            "shape_fill_mode",
            Some(120.0),
            &mut p.fill_mode,
            &ShapeFillMode::ALL,
            ShapeFillMode::label,
        );
        changed |= ui
            .add(egui::Slider::new(&mut p.thickness, 1..=MAX_OUTLINE_THICKNESS).suffix(" px"))
            .changed();
        if p.kind == ShapeKind::RoundedRect {
            changed |= ui
                .add(
                    egui::DragValue::new(&mut p.corner_radius)
                        .range(0.0..=120.0)
                        .prefix("R "),
                )
                .changed();
        }
        changed |= Self::pattern_selector(ui, "shape_pattern", &mut p.pattern);
        changed | Self::blend_selector(ui, "shape_blend", &mut p.blend)
    }

    pub(super) fn text_controls(
        ui: &mut egui::Ui,
        families: &[String],
        p: &mut TextParams,
    ) -> bool {
        let mut changed = false;
        let prev_f = p.family.clone();
        egui::ComboBox::from_id_salt("text_family")
            .width(115.0)
            .selected_text(text_tool::family_label(&p.family))
            .show_ui(ui, |ui| {
                for fam in families {
                    ui.selectable_value(&mut p.family, fam.clone(), text_tool::family_label(fam));
                }
            });
        changed |= p.family != prev_f;
        changed |= ui
            .add(
                egui::DragValue::new(&mut p.size_px)
                    .range(6.0..=240.0)
                    .suffix(" px"),
            )
            .changed();
        for (flag, label, tooltip) in [
            (&mut p.bold, "B", "text-bold"),
            (&mut p.italic, "I", "text-italic"),
            (&mut p.underline, "U", "text-underline"),
            (&mut p.strikethrough, "S", "text-strikethrough"),
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
        changed |= enum_combo(
            ui,
            "text_align",
            Some(82.0),
            &mut p.align,
            &TextAlign::ALL,
            TextAlign::label,
        );
        changed | Self::blend_selector(ui, "text_blend", &mut p.blend)
    }
}

/// A muted field label followed by its slider.
fn labeled_slider<N: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    theme: Theme,
    label_key: &str,
    value: &mut N,
    range: std::ops::RangeInclusive<N>,
    suffix: &str,
) -> egui::Response {
    ui.label(theme.muted_label(tr(label_key)));
    ui.add(egui::Slider::new(value, range).suffix(suffix))
}
