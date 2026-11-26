use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, Shadow, Stroke, Vec2};

pub const CARD_GAP: i8 = 6;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

impl Theme {
    pub fn apply(self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = Vec2::splat(6.0);
        style.spacing.button_padding = Vec2::new(8.0, 4.0);
        style.spacing.interact_size = Vec2::new(26.0, 24.0);

        let mut visuals = match self {
            Self::Light => egui::Visuals::light(),
            Self::Dark => egui::Visuals::dark(),
        };
        let (panel, field, active, hover, border) = match self {
            Self::Light => (
                rgb(245, 246, 252),
                rgb(233, 235, 247),
                rgb(221, 217, 248),
                rgb(226, 228, 244),
                rgb(218, 220, 238),
            ),
            Self::Dark => (
                rgb(23, 25, 36),
                rgb(40, 43, 60),
                rgb(68, 62, 114),
                rgb(52, 55, 76),
                rgb(56, 60, 84),
            ),
        };
        let fg = self.text_primary();
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
        visuals.window_corner_radius = CornerRadius::same(6);
        visuals.menu_corner_radius = CornerRadius::same(2);
        visuals.override_text_color = Some(fg);
        visuals.selection.bg_fill = active;
        visuals.selection.stroke = Stroke::new(1.0_f32, accent);

        visuals.widgets.noninteractive.bg_fill = panel;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, border);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, fg);

        visuals.widgets.inactive.bg_fill = field;
        visuals.widgets.inactive.weak_bg_fill = field;
        visuals.widgets.inactive.bg_stroke = Stroke::NONE;
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, fg);
        visuals.widgets.inactive.corner_radius = CornerRadius::same(5);

        visuals.widgets.hovered.bg_fill = hover;
        visuals.widgets.hovered.weak_bg_fill = hover;
        visuals.widgets.hovered.bg_stroke = Stroke::NONE;
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, fg);
        visuals.widgets.hovered.corner_radius = CornerRadius::same(5);

        visuals.widgets.active.bg_fill = active;
        visuals.widgets.active.weak_bg_fill = active;
        visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, accent);
        visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, self.pill_active_fg());
        visuals.widgets.active.corner_radius = CornerRadius::same(5);

        visuals.widgets.open.bg_fill = active;
        visuals.widgets.open.weak_bg_fill = active;
        visuals.widgets.open.bg_stroke = Stroke::NONE;
        visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, self.pill_active_fg());
        visuals.widgets.open.corner_radius = CornerRadius::same(5);

        visuals.popup_shadow = Shadow {
            offset: [0, 3],
            blur: 8,
            spread: 0,
            color: Color32::from_black_alpha(match self {
                Self::Light => 28,
                Self::Dark => 90,
            }),
        };

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
        style.visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        style.visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
        style.visuals.widgets.inactive.bg_stroke = Stroke::NONE;
        style.visuals.widgets.hovered.weak_bg_fill = hover;
        style.visuals.widgets.hovered.bg_fill = hover;
        style.visuals.widgets.hovered.bg_stroke = Stroke::NONE;
        style.visuals.widgets.hovered.corner_radius = CornerRadius::same(4);
        style.visuals.widgets.active.weak_bg_fill = open;
        style.visuals.widgets.active.bg_fill = open;
        style.visuals.widgets.active.bg_stroke = Stroke::NONE;
        style.visuals.widgets.active.corner_radius = CornerRadius::same(4);
        style.visuals.widgets.open.weak_bg_fill = open;
        style.visuals.widgets.open.bg_fill = open;
        style.visuals.widgets.open.bg_stroke = Stroke::NONE;
        style.visuals.widgets.open.corner_radius = CornerRadius::same(4);
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
        style.visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        style.visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
        style.visuals.widgets.inactive.bg_stroke = Stroke::NONE;
        style.visuals.widgets.hovered.weak_bg_fill = hover;
        style.visuals.widgets.hovered.bg_fill = hover;
        style.visuals.widgets.hovered.bg_stroke = Stroke::NONE;
        style.visuals.widgets.hovered.corner_radius = CornerRadius::same(2);
        style.visuals.widgets.active.weak_bg_fill = hover;
        style.visuals.widgets.active.bg_fill = hover;
        style.visuals.widgets.active.bg_stroke = Stroke::NONE;
        style.visuals.widgets.active.corner_radius = CornerRadius::same(2);
        style.visuals.widgets.open.weak_bg_fill = hover;
        style.visuals.widgets.open.bg_fill = hover;
        style.visuals.widgets.open.bg_stroke = Stroke::NONE;
        style.visuals.widgets.open.corner_radius = CornerRadius::same(2);
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

    pub fn styled_card(
        self,
        ui: &mut egui::Ui,
        title: &str,
        header_right: impl FnOnce(&mut egui::Ui),
        body: impl FnOnce(&mut egui::Ui),
    ) {
        let radius = CornerRadius::same(8);
        Frame::new()
            .fill(self.card_fill())
            .stroke(Stroke::new(1.0_f32, self.border_color()))
            .corner_radius(radius)
            .shadow(Shadow {
                offset: [0, 1],
                blur: 4,
                spread: 0,
                color: Color32::from_black_alpha(match self {
                    Self::Light => 10,
                    Self::Dark => 45,
                }),
            })
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                Frame::new()
                    .fill(self.card_header_fill())
                    .corner_radius(CornerRadius {
                        nw: 7,
                        ne: 7,
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
                let line = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), egui::Sense::hover()).0;
                ui.painter().line_segment(
                    [line.left_center(), line.right_center()],
                    Stroke::new(1.0_f32, self.border_color()),
                );
                Frame::new()
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = Vec2::splat(6.0);
                        body(ui);
                    });
            });
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}
