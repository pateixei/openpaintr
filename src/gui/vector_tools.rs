use super::app_settings::AaMode;
use super::i18n::text as tr;
use std::collections::HashMap;

use eframe::egui::{self, Color32, CursorIcon, Rect, Stroke, Vec2, vec2};
use gfx::{
    dash::{
        DASH_COUNT, DASH_DASH, DASH_DASHDOT, DASH_DASHDOTDOT, DASH_DOT, DASH_SOLID, DStyle,
        LCAP_ARROW, LCAP_ARROW_FILLED, LCAP_FLAT, LCAP_ROUND, LCAP_SQUARE, dash_pattern,
        dash_stroke,
    },
    path::Path as GfxPath,
    raster::Raster,
    {JOIN_ROUND, RULE_EVENODD, RULE_NONZERO},
};
use openpaintr::Pixel;

use super::{
    geometry::Affine,
    gradient_tool::{SessionOutcome, snap_if},
    paint_dashed_line,
    selection::{Point, Selection},
    tolerance::{self, FillPattern, StrokeBlend},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CurveKind {
    #[default]
    CubicBezier,
    Spline,
}

impl CurveKind {
    pub const ALL: [Self; 2] = [Self::CubicBezier, Self::Spline];

    pub fn label(self) -> &'static str {
        match self {
            Self::CubicBezier => tr("bezier"),
            Self::Spline => tr("spline"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DashPreset {
    #[default]
    Solid,
    Dash,
    Dot,
    DashDot,
    DashDotDot,
}

impl DashPreset {
    pub const ALL: [Self; 5] = [
        Self::Solid,
        Self::Dash,
        Self::Dot,
        Self::DashDot,
        Self::DashDotDot,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Solid => tr("solid-line"),
            Self::Dash => tr("dashed-line"),
            Self::Dot => tr("dotted-line"),
            Self::DashDot => tr("dash-dot"),
            Self::DashDotDot => tr("dash-dot-dot"),
        }
    }

    pub fn code(self) -> u32 {
        match self {
            Self::Solid => DASH_SOLID,
            Self::Dash => DASH_DASH,
            Self::Dot => DASH_DOT,
            Self::DashDot => DASH_DASHDOT,
            Self::DashDotDot => DASH_DASHDOTDOT.min(DASH_COUNT - 1),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineCapStyle {
    Flat,
    #[default]
    Round,
    Square,
    Arrow,
    ArrowFilled,
}

impl LineCapStyle {
    pub const ALL: [Self; 5] = [
        Self::Flat,
        Self::Round,
        Self::Square,
        Self::Arrow,
        Self::ArrowFilled,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Flat => tr("flat-cap"),
            Self::Round => tr("round-cap"),
            Self::Square => tr("square-cap"),
            Self::Arrow => tr("open-arrow"),
            Self::ArrowFilled => tr("filled-arrow"),
        }
    }

    pub fn code(self) -> u32 {
        match self {
            Self::Flat => LCAP_FLAT,
            Self::Round => LCAP_ROUND,
            Self::Square => LCAP_SQUARE,
            Self::Arrow => LCAP_ARROW,
            Self::ArrowFilled => LCAP_ARROW_FILLED,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShapeFillMode {
    Outline,
    Fill,
    #[default]
    Both,
}

impl ShapeFillMode {
    pub const ALL: [Self; 3] = [Self::Outline, Self::Fill, Self::Both];

    pub fn label(self) -> &'static str {
        match self {
            Self::Outline => tr("outline"),
            Self::Fill => tr("fill"),
            Self::Both => tr("outline-fill"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeFamily {
    Basic,
    Polygons,
    Arrows,
    Symbols,
    Freeform,
}

impl ShapeFamily {
    pub const ALL: [Self; 5] = [
        Self::Basic,
        Self::Polygons,
        Self::Arrows,
        Self::Symbols,
        Self::Freeform,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Basic => tr("basic-shapes"),
            Self::Polygons => tr("polygons-stars"),
            Self::Arrows => tr("arrows"),
            Self::Symbols => tr("symbols-callouts"),
            Self::Freeform => tr("freeform-v3"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShapeKind {
    #[default]
    Rectangle,
    RoundedRect,
    Ellipse,
    Diamond,
    Trapezoid,
    Parallelogram,
    Triangle,
    RightTriangle,
    Pentagon,
    Hexagon,
    Star4,
    Star5,
    Star6,
    ArrowRight,
    ArrowLeft,
    ArrowUp,
    ArrowDown,
    DoubleArrow,
    Chevron,
    Heart,
    Lightning,
    Cross,
    Checkmark,
    CalloutRect,
    CalloutOval,
    Freeform,
}

impl ShapeKind {
    pub const ALL: [Self; 26] = [
        Self::Rectangle,
        Self::RoundedRect,
        Self::Ellipse,
        Self::Diamond,
        Self::Trapezoid,
        Self::Parallelogram,
        Self::Triangle,
        Self::RightTriangle,
        Self::Pentagon,
        Self::Hexagon,
        Self::Star4,
        Self::Star5,
        Self::Star6,
        Self::ArrowRight,
        Self::ArrowLeft,
        Self::ArrowUp,
        Self::ArrowDown,
        Self::DoubleArrow,
        Self::Chevron,
        Self::Heart,
        Self::Lightning,
        Self::Cross,
        Self::Checkmark,
        Self::CalloutRect,
        Self::CalloutOval,
        Self::Freeform,
    ];

    pub fn family(self) -> ShapeFamily {
        match self {
            Self::Rectangle
            | Self::RoundedRect
            | Self::Ellipse
            | Self::Diamond
            | Self::Trapezoid
            | Self::Parallelogram => ShapeFamily::Basic,
            Self::Triangle
            | Self::RightTriangle
            | Self::Pentagon
            | Self::Hexagon
            | Self::Star4
            | Self::Star5
            | Self::Star6 => ShapeFamily::Polygons,
            Self::ArrowRight
            | Self::ArrowLeft
            | Self::ArrowUp
            | Self::ArrowDown
            | Self::DoubleArrow
            | Self::Chevron => ShapeFamily::Arrows,
            Self::Heart
            | Self::Lightning
            | Self::Cross
            | Self::Checkmark
            | Self::CalloutRect
            | Self::CalloutOval => ShapeFamily::Symbols,
            Self::Freeform => ShapeFamily::Freeform,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Rectangle => tr("rectangle"),
            Self::RoundedRect => tr("rounded-rectangle"),
            Self::Ellipse => tr("ellipse"),
            Self::Diamond => tr("diamond"),
            Self::Trapezoid => tr("trapezoid"),
            Self::Parallelogram => tr("parallelogram"),
            Self::Triangle => tr("triangle"),
            Self::RightTriangle => tr("right-triangle"),
            Self::Pentagon => tr("pentagon"),
            Self::Hexagon => tr("hexagon"),
            Self::Star4 => tr("star-four"),
            Self::Star5 => tr("star-five"),
            Self::Star6 => tr("star-six"),
            Self::ArrowRight => tr("arrow-right"),
            Self::ArrowLeft => tr("arrow-left"),
            Self::ArrowUp => tr("arrow-up"),
            Self::ArrowDown => tr("arrow-down"),
            Self::DoubleArrow => tr("arrow-double"),
            Self::Chevron => tr("chevron"),
            Self::Heart => tr("heart"),
            Self::Lightning => tr("lightning"),
            Self::Cross => tr("cross"),
            Self::Checkmark => tr("check-mark"),
            Self::CalloutRect => tr("rect-callout"),
            Self::CalloutOval => tr("oval-callout"),
            Self::Freeform => tr("freeform"),
        }
    }
}

pub fn rasterize_path_ssaa(path_4x: &GfxPath, rule: u32, size: Point) -> HashMap<Point, u16> {
    let mut map = HashMap::new();
    if size.0 == 0 || size.1 == 0 || path_4x.is_empty() {
        return map;
    }
    let mut ras = Raster::default();
    path_4x.fill_lines(&mut ras);
    ras.fill_rows(
        [0, 0, (size.0 * 4) as i32, (size.1 * 4) as i32],
        rule,
        &mut |sub_y, sub_x, covs| {
            let sy = sub_y as u32;
            let py = sy >> 2;
            let bit_y = (sy & 3) * 4;
            for (i, &cov) in covs.iter().enumerate() {
                if cov < 128 {
                    continue;
                }
                let sx = (sub_x + i as i32) as u32;
                let px = sx >> 2;
                if px < size.0 && py < size.1 {
                    let bit = bit_y + (sx & 3);
                    *map.entry((px, py)).or_insert(0) |= 1u16 << bit;
                }
            }
        },
    );
    map
}

pub fn rasterize_path_analytical(path_1x: &GfxPath, rule: u32, size: Point) -> HashMap<Point, u8> {
    let mut map = HashMap::new();
    if size.0 == 0 || size.1 == 0 || path_1x.is_empty() {
        return map;
    }
    let mut ras = Raster::default();
    path_1x.fill_lines(&mut ras);
    ras.fill_rows(
        [0, 0, size.0 as i32, size.1 as i32],
        rule,
        &mut |y, x, covs| {
            let py = y as u32;
            if py >= size.1 {
                return;
            }
            for (i, &cov) in covs.iter().enumerate() {
                if cov == 0 {
                    continue;
                }
                let px = (x + i as i32) as u32;
                if px < size.0 {
                    map.insert((px, py), cov);
                }
            }
        },
    );
    map
}

fn rasterize_path(path: &GfxPath, rule: u32, size: Point, aa_mode: AaMode) -> HashMap<Point, u8> {
    match aa_mode {
        AaMode::Analytical => rasterize_path_analytical(path, rule, size),
        AaMode::Ssaa4x4 => rasterize_path_ssaa(path, rule, size)
            .into_iter()
            .filter_map(|(pt, mask)| {
                let alpha = ((mask.count_ones() * 255 + 8) / 16) as u8;
                (alpha > 0).then_some((pt, alpha))
            })
            .collect(),
    }
}

fn apply_coverage_map(
    out: &mut [Pixel],
    base_layer: &[Pixel],
    size: Point,
    selection: Option<&Selection>,
    cov_map: &HashMap<Point, u8>,
    blend: StrokeBlend,
    mut color_at: impl FnMut(Point) -> Pixel,
) {
    let w = size.0 as usize;
    for (&pt @ (x, y), &cov) in cov_map {
        let sel_cov = selection.map_or(255, |sel| sel.coverage_u8(pt));
        if sel_cov == 0 {
            continue;
        }
        let eff = ((u32::from(cov) * u32::from(sel_cov) + 127) / 255) as u8;
        if eff == 0 {
            continue;
        }
        let idx = y as usize * w + x as usize;
        out[idx] = tolerance::blend_covered(color_at(pt), base_layer[idx], eff, blend);
    }
}

#[derive(Clone)]
pub struct LineCurveSession {
    pub points: [Vec2; 4],
    pub kind: CurveKind,
    pub thickness: u32,
    pub dash: DashPreset,
    pub start_cap: LineCapStyle,
    pub end_cap: LineCapStyle,
    pub color: Pixel,
    pub blend: StrokeBlend,
    dragging: Option<(usize, [Vec2; 4], Vec2)>,
}

impl LineCurveSession {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        start: Vec2,
        end: Vec2,
        kind: CurveKind,
        thickness: u32,
        dash: DashPreset,
        start_cap: LineCapStyle,
        end_cap: LineCapStyle,
        color: Pixel,
        blend: StrokeBlend,
    ) -> Self {
        let p1 = start + (end - start) / 3.0;
        let p2 = start + (end - start) * (2.0 / 3.0);
        Self {
            points: [start, p1, p2, end],
            kind,
            thickness: thickness.max(1),
            dash,
            start_cap,
            end_cap,
            color,
            blend,
            dragging: None,
        }
    }

    pub fn set_end(&mut self, start: Vec2, end: Vec2, constrain: bool) {
        let end = snap_if(start, end, constrain);
        self.points = [
            start,
            start + (end - start) / 3.0,
            start + (end - start) * (2.0 / 3.0),
            end,
        ];
    }

    pub fn nudge(&mut self, dx: f32, dy: f32) {
        let d = vec2(dx, dy);
        for p in &mut self.points {
            *p += d;
        }
    }

    pub fn render_preview(
        &self,
        base_layer: &[Pixel],
        size: Point,
        selection: Option<&Selection>,
        aa_mode: AaMode,
    ) -> Vec<Pixel> {
        let mut out = base_layer.to_vec();
        let mut path = GfxPath::default();
        let scale = match aa_mode {
            AaMode::Analytical => 1.0_f32,
            AaMode::Ssaa4x4 => 4.0_f32,
        };
        let s = |v: Vec2| (v.x * scale, v.y * scale);
        let [p0, p1, p2, p3] = self.points;
        let (ax, ay) = s(p0);
        path.move_f(ax, ay);
        match self.kind {
            CurveKind::CubicBezier => {
                path.cubic_f(s(p1), s(p2), s(p3));
            }
            CurveKind::Spline => {
                // Catmull-Rom segments passing cleanly through all 4 control knots.
                let seg = |a: Vec2, b: Vec2, c: Vec2, d: Vec2, out_path: &mut GfxPath| {
                    let c1 = b + (c - a) / 6.0;
                    let c2 = c - (d - b) / 6.0;
                    out_path.cubic_f(s(c1), s(c2), s(c));
                };
                seg(p0, p0, p1, p2, &mut path);
                seg(p0, p1, p2, p3, &mut path);
                seg(p1, p2, p3, p3, &mut path);
            }
        }
        let style = DStyle {
            width: (f64::from(self.thickness.max(1)) * f64::from(scale)).max(0.2),
            pat: dash_pattern(self.dash.code()),
            join: JOIN_ROUND,
            cap0: self.start_cap.code(),
            cap1: self.end_cap.code(),
            dcap: LCAP_FLAT,
            offset: 0.0,
            miter: 4.0,
        };
        dash_stroke(&mut path, &style);
        let cov_map = rasterize_path(&path, RULE_NONZERO, size, aa_mode);
        apply_coverage_map(
            &mut out,
            base_layer,
            size,
            selection,
            &cov_map,
            self.blend,
            |_| self.color,
        );
        out
    }

    pub fn handles(&mut self, rect: Rect, zoom: f32, ui: &egui::Ui) -> SessionOutcome {
        let to_screen = |pt: Vec2| rect.min + pt * zoom;
        let to_world = |pos: egui::Pos2| (pos - rect.min) / zoom;
        let bg_response = ui.interact(
            ui.clip_rect(),
            ui.id().with("line_curve_bg"),
            egui::Sense::click_and_drag(),
        );
        let mut changed = false;
        let screen_pts = self.points.map(to_screen);

        for (idx, pos) in screen_pts.into_iter().enumerate() {
            let response = ui.interact(
                Rect::from_center_size(pos, Vec2::splat(15.0)),
                ui.id().with(("line_curve_pt", idx)),
                egui::Sense::drag(),
            );
            if response.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            } else if response.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::Grab);
            }
            if response.drag_started()
                && let Some(pointer) = response.interact_pointer_pos()
            {
                let origin = to_world(pointer - response.total_drag_delta().unwrap_or_default());
                self.dragging = Some((idx, self.points, origin));
            }
            if let Some((active, _orig_pts, _origin)) = self.dragging
                && active == idx
            {
                if response.drag_stopped() {
                    self.dragging = None;
                }
                if let Some(pointer) = response.interact_pointer_pos() {
                    let mut next = to_world(pointer);
                    let constrain = ui.ctx().input(|i| i.modifiers.shift);
                    if constrain && idx == 0 {
                        next = snap_if(self.points[3], next, true);
                    } else if constrain && idx == 3 {
                        next = snap_if(self.points[0], next, true);
                    }
                    self.points[idx] = next;
                    changed = true;
                }
            }
        }

        // Dragging canvas moves the whole curve cleanly if not on control handle
        if bg_response.drag_started()
            && self.dragging.is_none()
            && let Some(pointer) = bg_response.interact_pointer_pos()
        {
            let origin = to_world(pointer - bg_response.total_drag_delta().unwrap_or_default());
            self.dragging = Some((usize::MAX, self.points, origin));
        }
        if let Some((usize::MAX, orig_pts, origin)) = self.dragging {
            if bg_response.drag_stopped() {
                self.dragging = None;
            }
            if let Some(pointer) = bg_response.interact_pointer_pos() {
                let delta = to_world(pointer) - origin;
                for (i, p) in self.points.iter_mut().enumerate() {
                    *p = orig_pts[i] + delta;
                }
                changed = true;
            }
        }

        let painter = ui.painter();
        paint_dashed_line(
            painter,
            &[screen_pts[0], screen_pts[1], screen_pts[2], screen_pts[3]],
        );
        for (idx, pos) in screen_pts.into_iter().enumerate() {
            let fill = if idx == 0 || idx == 3 {
                Color32::WHITE
            } else {
                Color32::from_rgb(103, 75, 195)
            };
            painter.circle(pos, 5.0, fill, Stroke::new(1.2_f32, Color32::BLACK));
        }

        if bg_response.clicked() && self.dragging.is_none() && !changed {
            SessionOutcome::Commit
        } else if changed {
            SessionOutcome::Changed
        } else {
            SessionOutcome::None
        }
    }
}

#[derive(Clone)]
pub struct ShapeSession {
    pub kind: ShapeKind,
    pub bounds: (Vec2, Vec2),
    pub affine: Affine,
    pub freeform_pts: Vec<Vec2>,
    pub fill_mode: ShapeFillMode,
    pub thickness: u32,
    pub corner_radius: f32,
    pub pattern: FillPattern,
    pub outline_color: Pixel,
    pub fill_color: Pixel,
    pub blend: StrokeBlend,
    dragging: Option<(usize, Affine, Vec2)>,
}

impl ShapeSession {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kind: ShapeKind,
        start: Vec2,
        end: Vec2,
        fill_mode: ShapeFillMode,
        thickness: u32,
        corner_radius: f32,
        pattern: FillPattern,
        outline_color: Pixel,
        fill_color: Pixel,
        blend: StrokeBlend,
    ) -> Self {
        let min = vec2(start.x.min(end.x), start.y.min(end.y));
        let max = vec2(start.x.max(end.x), start.y.max(end.y));
        let center = (min + max) * 0.5;
        let mut affine = Affine::centered((1, 1));
        affine.center = (f64::from(center.x), f64::from(center.y));
        Self {
            kind,
            bounds: (min, max),
            affine,
            freeform_pts: if kind == ShapeKind::Freeform {
                vec![start]
            } else {
                Vec::new()
            },
            fill_mode,
            thickness: thickness.max(1),
            corner_radius,
            pattern,
            outline_color,
            fill_color,
            blend,
            dragging: None,
        }
    }

    pub fn set_creation_bounds(&mut self, anchor: Vec2, current: Vec2, constrain: bool) {
        if self.kind == ShapeKind::Freeform {
            if self
                .freeform_pts
                .last()
                .is_none_or(|p| (*p - current).length() >= 1.5)
            {
                self.freeform_pts.push(current);
            }
            return;
        }
        let mut end = current;
        if constrain {
            let d = current - anchor;
            let side = d.x.abs().max(d.y.abs());
            end = vec2(
                anchor.x + side * if d.x < 0.0 { -1.0 } else { 1.0 },
                anchor.y + side * if d.y < 0.0 { -1.0 } else { 1.0 },
            );
        }
        let min = vec2(anchor.x.min(end.x), anchor.y.min(end.y));
        let max = vec2(anchor.x.max(end.x), anchor.y.max(end.y));
        self.bounds = (min, max);
        let center = (min + max) * 0.5;
        self.affine.center = (f64::from(center.x), f64::from(center.y));
        self.affine.shift = (0.0, 0.0);
    }

    pub fn nudge(&mut self, dx: f32, dy: f32) {
        if self.kind == ShapeKind::Freeform {
            let d = vec2(dx, dy);
            for p in &mut self.freeform_pts {
                *p += d;
            }
        } else {
            self.affine.shift.0 += f64::from(dx);
            self.affine.shift.1 += f64::from(dy);
        }
    }

    pub fn render_preview(
        &self,
        base_layer: &[Pixel],
        size: Point,
        selection: Option<&Selection>,
        aa_mode: AaMode,
    ) -> Vec<Pixel> {
        let mut out = base_layer.to_vec();
        let (min, max) = self.bounds;
        if self.kind != ShapeKind::Freeform
            && (max.x - min.x).abs() < 0.5
            && (max.y - min.y).abs() < 0.5
        {
            return out;
        }
        if self.kind == ShapeKind::Freeform && self.freeform_pts.len() < 2 {
            return out;
        }

        let scale = match aa_mode {
            AaMode::Analytical => 1.0_f32,
            AaMode::Ssaa4x4 => 4.0_f32,
        };
        let mut fill_path = build_shape_path(
            self.kind,
            self.bounds,
            self.corner_radius,
            &self.freeform_pts,
            self.affine,
            scale,
        );
        let do_fill = matches!(self.fill_mode, ShapeFillMode::Fill | ShapeFillMode::Both)
            && !(self.kind == ShapeKind::Freeform && self.freeform_pts.len() < 3);
        let do_outline = matches!(self.fill_mode, ShapeFillMode::Outline | ShapeFillMode::Both);

        if do_fill {
            let fill_map = rasterize_path(&fill_path, RULE_EVENODD, size, aa_mode);
            let fg = if self.fill_mode == ShapeFillMode::Fill {
                self.outline_color
            } else {
                self.fill_color
            };
            let bg = if self.fill_mode == ShapeFillMode::Fill {
                self.fill_color
            } else {
                self.outline_color
            };
            apply_coverage_map(
                &mut out,
                base_layer,
                size,
                selection,
                &fill_map,
                self.blend,
                |pt| self.pattern.sample(pt.0, pt.1, fg, bg),
            );
        }
        if do_outline {
            let style = DStyle {
                width: (f64::from(self.thickness.max(1)) * f64::from(scale)).max(0.2),
                pat: dash_pattern(DASH_SOLID),
                join: JOIN_ROUND,
                cap0: LCAP_ROUND,
                cap1: LCAP_ROUND,
                dcap: LCAP_FLAT,
                offset: 0.0,
                miter: 4.0,
            };
            dash_stroke(&mut fill_path, &style);
            let stroke_map = rasterize_path(&fill_path, RULE_NONZERO, size, aa_mode);
            let base_after_fill = out.clone();
            apply_coverage_map(
                &mut out,
                &base_after_fill,
                size,
                selection,
                &stroke_map,
                self.blend,
                |_| self.outline_color,
            );
        }
        out
    }

    pub fn handles(&mut self, rect: Rect, zoom: f32, ui: &egui::Ui) -> SessionOutcome {
        if self.kind == ShapeKind::Freeform {
            let bg_response = ui.interact(
                ui.clip_rect(),
                ui.id().with("shape_freeform_bg"),
                egui::Sense::click(),
            );
            let painter = ui.painter();
            if self.freeform_pts.len() >= 2 {
                let mut pts: Vec<_> = self
                    .freeform_pts
                    .iter()
                    .map(|&p| rect.min + p * zoom)
                    .collect();
                pts.push(pts[0]);
                paint_dashed_line(painter, &pts);
            }
            return if bg_response.clicked() {
                SessionOutcome::Commit
            } else {
                SessionOutcome::None
            };
        }

        let (min, max) = self.bounds;
        let corners = [min, vec2(max.x, min.y), max, vec2(min.x, max.y)];
        let project = |a: Affine, pt: Vec2| {
            let (x, y) = a.forward((f64::from(pt.x), f64::from(pt.y)));
            rect.min + zoom * vec2(x as f32, y as f32)
        };
        let to_world = |pos: egui::Pos2| (pos - rect.min) / zoom;
        let points = corners.map(|p| project(self.affine, p));
        let top = points[0].lerp(points[1], 0.5);
        let box_center = points[0].lerp(points[2], 0.5);
        let top_dir = top - box_center;
        let rotation = if top_dir.length_sq() > 1e-4 {
            top + top_dir.normalized() * 20.0
        } else {
            top + vec2(0.0, -20.0)
        };

        let bg_response = ui.interact(
            ui.clip_rect(),
            ui.id().with("shape_bg"),
            egui::Sense::click_and_drag(),
        );
        let mut changed = false;

        // 0..=3 corners, 4 rotate, 99 bg move
        let targets = [
            (0usize, points[0], CursorIcon::ResizeNwSe),
            (1, points[1], CursorIcon::ResizeNeSw),
            (2, points[2], CursorIcon::ResizeNwSe),
            (3, points[3], CursorIcon::ResizeNeSw),
            (4, rotation, CursorIcon::Grab),
        ];

        for (idx, pos, cursor) in targets {
            let resp = ui.interact(
                Rect::from_center_size(pos, Vec2::splat(14.0)),
                ui.id().with(("shape_handle", idx)),
                egui::Sense::drag(),
            );
            if resp.dragged() || resp.hovered() {
                ui.ctx().set_cursor_icon(cursor);
            }
            if resp.drag_started()
                && let Some(ptr) = resp.interact_pointer_pos()
            {
                let origin = to_world(ptr - resp.total_drag_delta().unwrap_or_default());
                self.dragging = Some((idx, self.affine, origin));
            }
            if let Some((active, start, origin)) = self.dragging
                && active == idx
            {
                if resp.drag_stopped() {
                    self.dragging = None;
                }
                if let Some(ptr) = resp.interact_pointer_pos() {
                    let current = to_world(ptr);
                    if idx == 4 {
                        let c = vec2(
                            (start.center.0 + start.shift.0) as f32,
                            (start.center.1 + start.shift.1) as f32,
                        );
                        let mut deg =
                            f64::from(((current - c).angle() - (origin - c).angle()).to_degrees());
                        if ui.ctx().input(|i| i.modifiers.shift) {
                            deg = (deg / 15.0).round() * 15.0;
                        }
                        self.affine.degrees = start.degrees + deg;
                    } else {
                        let opp_corner = corners[(idx + 2) % 4];
                        let opp_world =
                            start.forward((f64::from(opp_corner.x), f64::from(opp_corner.y)));
                        let pivot = vec2(opp_world.0 as f32, opp_world.1 as f32);
                        let rot =
                            egui::emath::Rot2::from_angle(-(start.degrees as f32).to_radians());
                        let from = rot * (origin - pivot);
                        let to = rot * (current - pivot);
                        let rx = if from.x.abs() < 0.01 {
                            1.0
                        } else {
                            f64::from(to.x / from.x)
                        };
                        let ry = if from.y.abs() < 0.01 {
                            1.0
                        } else {
                            f64::from(to.y / from.y)
                        };
                        let (rx, ry) = if ui.ctx().input(|i| i.modifiers.shift) {
                            let u = if (rx - 1.0).abs() > (ry - 1.0).abs() {
                                rx
                            } else {
                                ry
                            };
                            (u, u)
                        } else {
                            (rx, ry)
                        };
                        self.affine.scale = (
                            (start.scale.0 * rx).clamp(0.02, 32.0),
                            (start.scale.1 * ry).clamp(0.02, 32.0),
                        );
                        let mut temp = start;
                        temp.scale = self.affine.scale;
                        let temp_opp =
                            temp.forward((f64::from(opp_corner.x), f64::from(opp_corner.y)));
                        self.affine.shift = (
                            start.shift.0 + (opp_world.0 - temp_opp.0),
                            start.shift.1 + (opp_world.1 - temp_opp.1),
                        );
                    }
                    changed = true;
                }
            }
        }

        if bg_response.drag_started()
            && self.dragging.is_none()
            && let Some(ptr) = bg_response.interact_pointer_pos()
        {
            let origin = to_world(ptr - bg_response.total_drag_delta().unwrap_or_default());
            self.dragging = Some((99, self.affine, origin));
        }
        if let Some((99, start, origin)) = self.dragging {
            if bg_response.drag_stopped() {
                self.dragging = None;
            }
            if let Some(ptr) = bg_response.interact_pointer_pos() {
                let d = to_world(ptr) - origin;
                self.affine.shift = (
                    start.shift.0 + f64::from(d.x),
                    start.shift.1 + f64::from(d.y),
                );
                changed = true;
            }
        }

        let painter = ui.painter();
        paint_dashed_line(
            painter,
            &[points[0], points[1], points[2], points[3], points[0]],
        );
        painter.line_segment([top, rotation], Stroke::new(1.0_f32, Color32::WHITE));
        painter.circle(
            rotation,
            5.0,
            Color32::WHITE,
            Stroke::new(1.0_f32, Color32::BLACK),
        );
        for pt in points {
            painter.rect(
                Rect::from_center_size(pt, Vec2::splat(8.0)),
                1,
                Color32::WHITE,
                Stroke::new(1.0_f32, Color32::BLACK),
                egui::StrokeKind::Inside,
            );
        }

        if bg_response.clicked() && self.dragging.is_none() && !changed {
            SessionOutcome::Commit
        } else if changed {
            SessionOutcome::Changed
        } else {
            SessionOutcome::None
        }
    }
}

fn build_shape_path(
    kind: ShapeKind,
    bounds: (Vec2, Vec2),
    corner_radius: f32,
    freeform_pts: &[Vec2],
    affine: Affine,
    scale: f32,
) -> GfxPath {
    let mut path = GfxPath::default();
    if kind == ShapeKind::Freeform {
        if let Some(&first) = freeform_pts.first() {
            path.move_f(first.x * scale, first.y * scale);
            for &pt in &freeform_pts[1..] {
                path.line_f(pt.x * scale, pt.y * scale);
            }
            if freeform_pts.len() >= 3 {
                path.close();
            }
        }
        return path;
    }

    let (min, max) = bounds;
    let w = (max.x - min.x).max(1.0);
    let h = (max.y - min.y).max(1.0);
    let tx = |nx: f32, ny: f32| -> (f32, f32) {
        let local = (f64::from(min.x + nx * w), f64::from(min.y + ny * h));
        let (wx, wy) = affine.forward(local);
        (wx as f32 * scale, wy as f32 * scale)
    };
    let poly = |pts: &[(f32, f32)], closed: bool, p: &mut GfxPath| {
        if let Some(&(x, y)) = pts.first() {
            let (px, py) = tx(x, y);
            p.move_f(px, py);
            for &(nx, ny) in &pts[1..] {
                let (px, py) = tx(nx, ny);
                p.line_f(px, py);
            }
            if closed {
                p.close();
            }
        }
    };

    match kind {
        ShapeKind::Rectangle => poly(
            &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
            true,
            &mut path,
        ),
        ShapeKind::RoundedRect => {
            let rx = (corner_radius / w).clamp(0.0, 0.48);
            let ry = (corner_radius / h).clamp(0.0, 0.48);
            if rx <= 0.005 || ry <= 0.005 {
                poly(
                    &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
                    true,
                    &mut path,
                );
            } else {
                let k = 0.552_284_8_f32;
                let (p0x, p0y) = tx(rx, 0.0);
                path.move_f(p0x, p0y);
                let l = tx(1.0 - rx, 0.0);
                path.line_f(l.0, l.1);
                path.cubic_f(
                    tx(1.0 - rx * (1.0 - k), 0.0),
                    tx(1.0, ry * (1.0 - k)),
                    tx(1.0, ry),
                );
                let l = tx(1.0, 1.0 - ry);
                path.line_f(l.0, l.1);
                path.cubic_f(
                    tx(1.0, 1.0 - ry * (1.0 - k)),
                    tx(1.0 - rx * (1.0 - k), 1.0),
                    tx(1.0 - rx, 1.0),
                );
                let l = tx(rx, 1.0);
                path.line_f(l.0, l.1);
                path.cubic_f(
                    tx(rx * (1.0 - k), 1.0),
                    tx(0.0, 1.0 - ry * (1.0 - k)),
                    tx(0.0, 1.0 - ry),
                );
                let l = tx(0.0, ry);
                path.line_f(l.0, l.1);
                path.cubic_f(tx(0.0, ry * (1.0 - k)), tx(rx * (1.0 - k), 0.0), (p0x, p0y));
                path.close();
            }
        }
        ShapeKind::Ellipse => {
            let k = 0.552_284_8_f32 * 0.5;
            let (p0x, p0y) = tx(0.5, 0.0);
            path.move_f(p0x, p0y);
            path.cubic_f(tx(0.5 + k, 0.0), tx(1.0, 0.5 - k), tx(1.0, 0.5));
            path.cubic_f(tx(1.0, 0.5 + k), tx(0.5 + k, 1.0), tx(0.5, 1.0));
            path.cubic_f(tx(0.5 - k, 1.0), tx(0.0, 0.5 + k), tx(0.0, 0.5));
            path.cubic_f(tx(0.0, 0.5 - k), tx(0.5 - k, 0.0), (p0x, p0y));
            path.close();
        }
        ShapeKind::Diamond => poly(
            &[(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5)],
            true,
            &mut path,
        ),
        ShapeKind::Trapezoid => poly(
            &[(0.2, 0.0), (0.8, 0.0), (1.0, 1.0), (0.0, 1.0)],
            true,
            &mut path,
        ),
        ShapeKind::Parallelogram => poly(
            &[(0.25, 0.0), (1.0, 0.0), (0.75, 1.0), (0.0, 1.0)],
            true,
            &mut path,
        ),
        ShapeKind::Triangle => poly(&[(0.5, 0.0), (1.0, 1.0), (0.0, 1.0)], true, &mut path),
        ShapeKind::RightTriangle => poly(&[(0.0, 0.0), (1.0, 1.0), (0.0, 1.0)], true, &mut path),
        ShapeKind::Pentagon => regular_polygon(5, &tx, &mut path),
        ShapeKind::Hexagon => regular_polygon(6, &tx, &mut path),
        ShapeKind::Star4 => star_polygon(4, 0.38, &tx, &mut path),
        ShapeKind::Star5 => star_polygon(5, 0.38, &tx, &mut path),
        ShapeKind::Star6 => star_polygon(6, 0.45, &tx, &mut path),
        ShapeKind::ArrowRight => poly(
            &[
                (0.0, 0.28),
                (0.58, 0.28),
                (0.58, 0.05),
                (1.0, 0.5),
                (0.58, 0.95),
                (0.58, 0.72),
                (0.0, 0.72),
            ],
            true,
            &mut path,
        ),
        ShapeKind::ArrowLeft => poly(
            &[
                (1.0, 0.28),
                (0.42, 0.28),
                (0.42, 0.05),
                (0.0, 0.5),
                (0.42, 0.95),
                (0.42, 0.72),
                (1.0, 0.72),
            ],
            true,
            &mut path,
        ),
        ShapeKind::ArrowUp => poly(
            &[
                (0.28, 1.0),
                (0.28, 0.42),
                (0.05, 0.42),
                (0.5, 0.0),
                (0.95, 0.42),
                (0.72, 0.42),
                (0.72, 1.0),
            ],
            true,
            &mut path,
        ),
        ShapeKind::ArrowDown => poly(
            &[
                (0.28, 0.0),
                (0.28, 0.58),
                (0.05, 0.58),
                (0.5, 1.0),
                (0.95, 0.58),
                (0.72, 0.58),
                (0.72, 0.0),
            ],
            true,
            &mut path,
        ),
        ShapeKind::DoubleArrow => poly(
            &[
                (0.0, 0.5),
                (0.28, 0.15),
                (0.28, 0.34),
                (0.72, 0.34),
                (0.72, 0.15),
                (1.0, 0.5),
                (0.72, 0.85),
                (0.72, 0.66),
                (0.28, 0.66),
                (0.28, 0.85),
            ],
            true,
            &mut path,
        ),
        ShapeKind::Chevron => poly(
            &[
                (0.0, 0.1),
                (0.55, 0.1),
                (1.0, 0.5),
                (0.55, 0.9),
                (0.0, 0.9),
                (0.45, 0.5),
            ],
            true,
            &mut path,
        ),
        ShapeKind::Heart => {
            let (p0x, p0y) = tx(0.5, 0.92);
            path.move_f(p0x, p0y);
            path.cubic_f(tx(0.05, 0.62), tx(0.0, 0.22), tx(0.28, 0.1));
            path.cubic_f(tx(0.42, 0.04), tx(0.5, 0.2), tx(0.5, 0.28));
            path.cubic_f(tx(0.5, 0.2), tx(0.58, 0.04), tx(0.72, 0.1));
            path.cubic_f(tx(1.0, 0.22), tx(0.95, 0.62), (p0x, p0y));
            path.close();
        }
        ShapeKind::Lightning => poly(
            &[
                (0.58, 0.0),
                (0.18, 0.55),
                (0.46, 0.55),
                (0.35, 1.0),
                (0.82, 0.42),
                (0.52, 0.42),
            ],
            true,
            &mut path,
        ),
        ShapeKind::Cross => poly(
            &[
                (0.34, 0.0),
                (0.66, 0.0),
                (0.66, 0.34),
                (1.0, 0.34),
                (1.0, 0.66),
                (0.66, 0.66),
                (0.66, 1.0),
                (0.34, 1.0),
                (0.34, 0.66),
                (0.0, 0.66),
                (0.0, 0.34),
                (0.34, 0.34),
            ],
            true,
            &mut path,
        ),
        ShapeKind::Checkmark => poly(
            &[
                (0.08, 0.56),
                (0.22, 0.42),
                (0.40, 0.62),
                (0.78, 0.16),
                (0.92, 0.30),
                (0.40, 0.88),
            ],
            true,
            &mut path,
        ),
        ShapeKind::CalloutRect => poly(
            &[
                (0.0, 0.0),
                (1.0, 0.0),
                (1.0, 0.72),
                (0.48, 0.72),
                (0.22, 1.0),
                (0.26, 0.72),
                (0.0, 0.72),
            ],
            true,
            &mut path,
        ),
        ShapeKind::CalloutOval => {
            let k = 0.552_284_8_f32 * 0.5;
            let (p0x, p0y) = tx(0.5, 0.0);
            path.move_f(p0x, p0y);
            path.cubic_f(tx(0.5 + k, 0.0), tx(1.0, 0.38 - k * 0.76), tx(1.0, 0.38));
            path.cubic_f(tx(1.0, 0.38 + k * 0.76), tx(0.5 + k, 0.76), tx(0.46, 0.76));
            let tail = tx(0.22, 1.0);
            path.line_f(tail.0, tail.1);
            let back = tx(0.30, 0.74);
            path.line_f(back.0, back.1);
            path.cubic_f(tx(0.10, 0.68), tx(0.0, 0.52), tx(0.0, 0.38));
            path.cubic_f(tx(0.0, 0.38 - k * 0.76), tx(0.5 - k, 0.0), (p0x, p0y));
            path.close();
        }
        ShapeKind::Freeform => {}
    }
    path
}

fn regular_polygon(sides: usize, tx: &impl Fn(f32, f32) -> (f32, f32), path: &mut GfxPath) {
    for i in 0..sides {
        let angle = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / sides as f32;
        let (x, y) = tx(0.5 + 0.5 * angle.cos(), 0.5 + 0.5 * angle.sin());
        if i == 0 {
            path.move_f(x, y);
        } else {
            path.line_f(x, y);
        }
    }
    path.close();
}

fn star_polygon(
    points: usize,
    inner_ratio: f32,
    tx: &impl Fn(f32, f32) -> (f32, f32),
    path: &mut GfxPath,
) {
    let total = points * 2;
    for i in 0..total {
        let angle = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / total as f32;
        let r = if i.is_multiple_of(2) {
            0.5
        } else {
            0.5 * inner_ratio
        };
        let (x, y) = tx(0.5 + r * angle.cos(), 0.5 + r * angle.sin());
        if i == 0 {
            path.move_f(x, y);
        } else {
            path.line_f(x, y);
        }
    }
    path.close();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_and_shape_rasterize_with_aa_partial_alpha() {
        let size = (16, 16);
        let base = vec![Pixel::default(); 256];
        let mut line = LineCurveSession::new(
            vec2(1.0, 1.0),
            vec2(14.0, 13.0),
            CurveKind::CubicBezier,
            2,
            DashPreset::Dash,
            LineCapStyle::Round,
            LineCapStyle::ArrowFilled,
            Pixel([20, 90, 220, 255]),
            StrokeBlend::Normal,
        );
        line.points[1] = vec2(2.0, 12.0);
        let shape = ShapeSession::new(
            ShapeKind::Star5,
            vec2(1.0, 1.0),
            vec2(14.0, 14.0),
            ShapeFillMode::Both,
            2,
            4.0,
            FillPattern::Checker,
            Pixel([255, 0, 0, 255]),
            Pixel([0, 200, 0, 255]),
            StrokeBlend::Normal,
        );
        for mode in AaMode::ALL {
            let rendered = line.render_preview(&base, size, None, mode);
            assert!(rendered.iter().any(|p| p.0[3] == 255));
            assert!(rendered.iter().any(|p| (1..255).contains(&p.0[3])));

            let rendered_shape = shape.render_preview(&base, size, None, mode);
            assert!(rendered_shape.iter().any(|p| p.0[3] > 0));
            assert!(rendered_shape.iter().any(|p| (1..255).contains(&p.0[3])));
        }
    }
}
