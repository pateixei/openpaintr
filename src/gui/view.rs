use super::i18n::text as tr;
use eframe::egui::{self, Color32, Pos2, Rect, Vec2, pos2, vec2};
use openpaintr::Document;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Unit {
    #[default]
    Pixels,
    Inches,
    Centimeters,
}

impl Unit {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pixels => tr("pixels"),
            Self::Inches => tr("inches"),
            Self::Centimeters => tr("centimeters"),
        }
    }

    fn step_in_pixels(self, dpi: f64) -> f64 {
        match self {
            Self::Pixels => 1.0,
            Self::Inches => dpi,
            Self::Centimeters => dpi / 2.54,
        }
    }

    pub fn suffix(self) -> &'static str {
        match self {
            Self::Pixels => "px",
            Self::Inches => "pol",
            Self::Centimeters => "cm",
        }
    }
}

#[derive(Default)]
pub struct View {
    pub offset: Vec2,
    pub scroll_to: Option<Vec2>,
    pub fit_selection: bool,
    pub grid: bool,
    pub rulers: bool,
    pub unit: Unit,
    pub pointer: Option<Pos2>,
}

impl View {
    pub fn dimensions(&self, document: &Document) -> String {
        let (width, height) = document.dimensions();
        let (dpi_x, dpi_y) = document.resolution_dpi();
        match self.unit {
            Unit::Pixels => format!("{width} × {height} px"),
            unit => format!(
                "{:.2} × {:.2} {}",
                f64::from(width) / unit.step_in_pixels(dpi_x),
                f64::from(height) / unit.step_in_pixels(dpi_y),
                unit.suffix()
            ),
        }
    }

    pub fn coords(&self, document: &Document) -> Option<String> {
        let pos = self.pointer?;
        let (dpi_x, dpi_y) = document.resolution_dpi();
        Some(match self.unit {
            Unit::Pixels => format!("{}, {} px", pos.x as u32, pos.y as u32),
            unit => format!(
                "{:.2}, {:.2} {}",
                f64::from(pos.x) / unit.step_in_pixels(dpi_x),
                f64::from(pos.y) / unit.step_in_pixels(dpi_y),
                unit.suffix()
            ),
        })
    }

    pub fn guides(&self, ui: &egui::Ui, rect: Rect, zoom: f32, dpi: (f64, f64)) {
        let clip = ui.clip_rect().intersect(rect);
        if self.grid && zoom >= 4.0 && clip.is_positive() {
            let painter = ui.painter_at(clip);
            let color = Color32::from_black_alpha(100);
            for x in ((clip.left() - rect.left()) / zoom).ceil() as u32
                ..=((clip.right() - rect.left()) / zoom) as u32
            {
                let x = rect.left() + x as f32 * zoom;
                painter.line_segment(
                    [pos2(x, clip.top()), pos2(x, clip.bottom())],
                    (1.0_f32, color),
                );
            }
            for y in ((clip.top() - rect.top()) / zoom).ceil() as u32
                ..=((clip.bottom() - rect.top()) / zoom) as u32
            {
                let y = rect.top() + y as f32 * zoom;
                painter.line_segment(
                    [pos2(clip.left(), y), pos2(clip.right(), y)],
                    (1.0_f32, color),
                );
            }
        }
        if !self.rulers {
            return;
        }
        let viewport = ui.clip_rect();
        let horizontal = Rect::from_min_size(viewport.min, vec2(viewport.width(), 22.0));
        let vertical = Rect::from_min_size(viewport.min, vec2(34.0, viewport.height()));
        let color = ui.visuals().text_color();
        let fill = ui.visuals().panel_fill;
        let painter = ui.painter_at(viewport);
        painter.rect_filled(horizontal, 0.0, fill);
        painter.rect_filled(vertical, 0.0, fill);
        let font = egui::FontId::proportional(10.0);
        for (horizontal_axis, density) in [(true, dpi.0), (false, dpi.1)] {
            let origin = if horizontal_axis {
                rect.left()
            } else {
                rect.top()
            };
            let (min, max) = if horizontal_axis {
                (vertical.right(), viewport.right())
            } else {
                (horizontal.bottom(), viewport.bottom())
            };
            let spacing = (self.unit.step_in_pixels(density) * f64::from(zoom)) as f32;
            if !spacing.is_finite() || spacing <= f32::MIN_POSITIVE {
                continue;
            }
            let minimum = if self.unit == Unit::Pixels { 1.0 } else { 0.01 };
            let desired = (55.0 / spacing).max(minimum);
            if !desired.is_finite() {
                continue;
            }
            let magnitude = 10.0_f32.powf(desired.log10().floor());
            let step = [1.0, 2.0, 5.0, 10.0]
                .into_iter()
                .map(|step| step * magnitude)
                .find(|step| *step >= desired)
                .unwrap_or(10.0 * magnitude);
            let pixels = step * spacing;
            let start = ((min - origin) / pixels).ceil().max(0.0) as u32;
            let end = ((max - origin) / pixels).floor().max(0.0) as u32;
            for index in start..=end {
                let position = origin + index as f32 * pixels;
                if position < min || position > max {
                    continue;
                }
                let decimals = if step >= 1.0 {
                    0
                } else if step >= 0.1 {
                    1
                } else {
                    2
                };
                let label = format!("{:.*}", decimals, index as f32 * step);
                if horizontal_axis {
                    painter.line_segment(
                        [
                            pos2(position, horizontal.bottom() - 5.0),
                            pos2(position, horizontal.bottom()),
                        ],
                        (1.0_f32, color),
                    );
                    painter.text(
                        pos2(position + 2.0, horizontal.top() + 2.0),
                        egui::Align2::LEFT_TOP,
                        label,
                        font.clone(),
                        color,
                    );
                } else {
                    painter.line_segment(
                        [
                            pos2(vertical.right() - 5.0, position),
                            pos2(vertical.right(), position),
                        ],
                        (1.0_f32, color),
                    );
                    painter.text(
                        pos2(vertical.left() + 2.0, position),
                        egui::Align2::LEFT_CENTER,
                        label,
                        font.clone(),
                        color,
                    );
                }
            }
        }
    }
}
