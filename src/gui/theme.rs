use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, Shadow, Stroke, Vec2};
use serde::{Deserialize, Serialize};

pub const CARD_GAP: i8 = 6;
pub const CARD_CORNER_RADIUS: u8 = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    #[default]
    Light,
    Dark,
}

impl Theme {
    pub fn apply(self, ctx: &egui::Context) {
        ctx.set_theme(match self {
            Self::Light => egui::Theme::Light,
            Self::Dark => egui::Theme::Dark,
        });
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = Vec2::splat(6.0);
        style.spacing.button_padding = Vec2::new(8.0, 4.0);
        style.spacing.interact_size = Vec2::new(26.0, 24.0);

        let mut visuals = match self {
            Self::Light => egui::Visuals::light(),
            Self::Dark => egui::Visuals::dark(),
        };
        let (panel, field, active, hover, border) = (
            self.workspace(),
            match self {
                Self::Light => rgb(233, 235, 247),
                Self::Dark => rgb(40, 43, 60),
            },
            self.pill_active_fill(),
            match self {
                Self::Light => rgb(226, 228, 244),
                Self::Dark => rgb(52, 55, 76),
            },
            self.border_color(),
        );
        let fg = self.text_primary();
        let active_fg = self.pill_active_fg();
        let accent = self.accent();

        visuals.panel_fill = panel;
        visuals.window_fill = self.card_fill();
        visuals.extreme_bg_color = match self {
            Self::Light => rgb(255, 255, 255),
            Self::Dark => rgb(18, 20, 29),
        };
        visuals.faint_bg_color = match self {
            Self::Light => rgb(238, 239, 249),
            Self::Dark => rgb(36, 39, 54),
        };
        visuals.window_stroke = Stroke::new(1.0_f32, border);
        visuals.window_corner_radius = CornerRadius::same(CARD_CORNER_RADIUS);
        visuals.menu_corner_radius = CornerRadius::same(2);
        visuals.override_text_color = Some(fg);
        visuals.selection.bg_fill = active;
        visuals.selection.stroke = Stroke::new(1.0_f32, accent);

        set_widget_state(
            &mut visuals.widgets.noninteractive,
            panel,
            Stroke::new(1.0_f32, border),
            Some(fg),
            None,
        );
        for (state, bg, stroke, text_fg) in [
            (&mut visuals.widgets.inactive, field, Stroke::NONE, fg),
            (&mut visuals.widgets.hovered, hover, Stroke::NONE, fg),
            (
                &mut visuals.widgets.active,
                active,
                Stroke::new(1.0_f32, accent),
                active_fg,
            ),
            (&mut visuals.widgets.open, active, Stroke::NONE, active_fg),
        ] {
            set_widget_state(state, bg, stroke, Some(text_fg), Some(5));
        }

        visuals.popup_shadow = self.shadow(3, 8, [28, 90]);

        style.visuals = visuals;
        ctx.set_style(style);
    }

    pub fn menu_bar_style(style: &mut egui::Style) {
        let dark = style.visuals.dark_mode;
        let hover = if dark {
            rgb(48, 51, 72)
        } else {
            rgb(220, 221, 240)
        };
        let open = if dark {
            rgb(64, 59, 108)
        } else {
            rgb(213, 210, 242)
        };
        style.spacing.item_spacing = Vec2::new(2.0, 0.0);
        style.spacing.button_padding = Vec2::new(8.0, 3.0);
        style_menu_widgets(&mut style.visuals.widgets, hover, open, 4);
    }

    pub fn menu_popup_style(style: &mut egui::Style) {
        let dark = style.visuals.dark_mode;
        let bg = if dark {
            rgb(37, 39, 57)
        } else {
            rgb(255, 255, 255)
        };
        let border = if dark {
            rgb(62, 66, 94)
        } else {
            rgb(212, 213, 234)
        };
        let hover = if dark {
            rgb(62, 58, 102)
        } else {
            rgb(232, 229, 250)
        };
        style.spacing.button_padding = Vec2::new(10.0, 4.0);
        style.spacing.item_spacing = Vec2::new(6.0, 2.0);
        style.visuals.menu_corner_radius = CornerRadius::same(2);
        style.visuals.window_corner_radius = CornerRadius::same(2);
        style.visuals.window_fill = bg;
        style.visuals.window_stroke = Stroke::new(1.0_f32, border);
        style_menu_widgets(&mut style.visuals.widgets, hover, hover, 2);
    }

    pub fn workspace(self) -> Color32 {
        match self {
            Self::Light => rgb(245, 246, 252),
            Self::Dark => rgb(23, 25, 36),
        }
    }

    pub fn menubar_fill(self) -> Color32 {
        match self {
            Self::Light => rgb(235, 236, 248),
            Self::Dark => rgb(28, 30, 44),
        }
    }

    /// Downward drop shadow; `alpha` holds the black alpha for [light, dark].
    fn shadow(self, offset_y: i8, blur: u8, alpha: [u8; 2]) -> Shadow {
        Shadow {
            offset: [0, offset_y],
            blur,
            spread: 0,
            color: Color32::from_black_alpha(match self {
                Self::Light => alpha[0],
                Self::Dark => alpha[1],
            }),
        }
    }

    pub fn toolbar_fill(self) -> Color32 {
        match self {
            Self::Light => rgb(241, 242, 250),
            Self::Dark => rgb(32, 34, 49),
        }
    }

    pub fn status_fill(self) -> Color32 {
        match self {
            Self::Light => rgb(236, 237, 248),
            Self::Dark => rgb(27, 29, 42),
        }
    }

    pub fn accent(self) -> Color32 {
        match self {
            Self::Light => rgb(103, 75, 195),
            Self::Dark => rgb(154, 134, 242),
        }
    }

    pub fn error(self) -> Color32 {
        match self {
            Self::Light => rgb(180, 35, 45),
            Self::Dark => rgb(255, 135, 135),
        }
    }

    pub fn border_color(self) -> Color32 {
        match self {
            Self::Light => rgb(218, 220, 238),
            Self::Dark => rgb(56, 60, 84),
        }
    }

    pub fn card_fill(self) -> Color32 {
        match self {
            Self::Light => rgb(255, 255, 255),
            Self::Dark => rgb(34, 37, 52),
        }
    }

    pub fn card_header_fill(self) -> Color32 {
        match self {
            Self::Light => rgb(236, 236, 249),
            Self::Dark => rgb(43, 46, 66),
        }
    }

    pub fn pill_active_fill(self) -> Color32 {
        match self {
            Self::Light => rgb(221, 217, 248),
            Self::Dark => rgb(68, 62, 114),
        }
    }

    pub fn pill_active_fg(self) -> Color32 {
        match self {
            Self::Light => rgb(75, 66, 152),
            Self::Dark => rgb(230, 227, 255),
        }
    }

    pub fn hover_fill(self) -> Color32 {
        match self {
            Self::Light => rgb(228, 229, 244),
            Self::Dark => rgb(48, 51, 72),
        }
    }

    pub fn text_primary(self) -> Color32 {
        match self {
            Self::Light => rgb(30, 34, 51),
            Self::Dark => rgb(230, 232, 246),
        }
    }

    pub fn text_muted(self) -> Color32 {
        match self {
            Self::Light => rgb(104, 108, 128),
            Self::Dark => rgb(150, 156, 182),
        }
    }

    pub fn muted_label(self, text: impl Into<String>) -> egui::RichText {
        egui::RichText::new(text)
            .size(12.0)
            .color(self.text_muted())
    }

    pub fn section_label(self, text: impl Into<String>) -> egui::RichText {
        egui::RichText::new(text)
            .strong()
            .size(12.5)
            .color(self.text_primary())
    }

    pub fn pill_frame(self, active: bool, corner_radius: u8, margin: Margin) -> Frame {
        Frame::new()
            .fill(if active {
                self.pill_active_fill()
            } else {
                Color32::TRANSPARENT
            })
            .corner_radius(corner_radius)
            .inner_margin(margin)
    }

    pub fn styled_card(
        self,
        ui: &mut egui::Ui,
        title: &str,
        header_right: impl FnOnce(&mut egui::Ui),
        body: impl FnOnce(&mut egui::Ui),
    ) -> egui::Response {
        let radius = CornerRadius::same(CARD_CORNER_RADIUS);
        let card_width = ui.available_width();
        let inner_width = (card_width - 2.0).max(0.0);
        Frame::new()
            .fill(self.card_fill())
            .stroke(Stroke::new(1.0_f32, self.border_color()))
            .corner_radius(radius)
            .shadow(self.shadow(1, 4, [10, 45]))
            .show(ui, |ui| {
                ui.set_min_width(inner_width);
                ui.set_max_width(inner_width);
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                Frame::new()
                    .fill(self.card_header_fill())
                    .corner_radius(CornerRadius {
                        nw: CARD_CORNER_RADIUS - 1,
                        ne: CARD_CORNER_RADIUS - 1,
                        sw: 0,
                        se: 0,
                    })
                    .inner_margin(Margin::symmetric(10, 5))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(title)
                                    .strong()
                                    .size(12.5)
                                    .color(self.text_primary()),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                header_right,
                            );
                        });
                    });
                let line = ui
                    .allocate_exact_size(Vec2::new(ui.available_width(), 1.0), egui::Sense::hover())
                    .0;
                ui.painter().line_segment(
                    [line.left_center(), line.right_center()],
                    Stroke::new(1.0_f32, self.border_color()),
                );
                Frame::new().inner_margin(Margin::same(8)).show(ui, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::splat(6.0);
                    body(ui);
                });
            })
            .response
    }
}

fn set_widget_state(
    visuals: &mut egui::style::WidgetVisuals,
    bg: Color32,
    bg_stroke: Stroke,
    fg: Option<Color32>,
    corner_radius: Option<u8>,
) {
    visuals.bg_fill = bg;
    visuals.weak_bg_fill = bg;
    visuals.bg_stroke = bg_stroke;
    if let Some(fg) = fg {
        visuals.fg_stroke = Stroke::new(1.0_f32, fg);
    }
    if let Some(radius) = corner_radius {
        visuals.corner_radius = CornerRadius::same(radius);
    }
}

fn style_menu_widgets(
    widgets: &mut egui::style::Widgets,
    hover: Color32,
    active_open: Color32,
    radius: u8,
) {
    set_widget_state(
        &mut widgets.inactive,
        Color32::TRANSPARENT,
        Stroke::NONE,
        None,
        None,
    );
    set_widget_state(
        &mut widgets.hovered,
        hover,
        Stroke::NONE,
        None,
        Some(radius),
    );
    set_widget_state(
        &mut widgets.active,
        active_open,
        Stroke::NONE,
        None,
        Some(radius),
    );
    set_widget_state(
        &mut widgets.open,
        active_open,
        Stroke::NONE,
        None,
        Some(radius),
    );
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}
