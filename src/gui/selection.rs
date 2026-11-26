use std::{collections::VecDeque, io, rc::Rc};

use gfx::path::Path as GfxPath;
use gfx::raster::Raster;
use gfx::tmask::{TM_ADD, TM_INTERSECT, TM_REPLACE, TM_SUB, TM_XOR, TMask};
use openpaintr::Pixel;

pub type Point = (u32, u32);
pub type Bounds = (Point, Point);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Rectangle,
    Ellipse,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Combine {
    #[default]
    Replace,
    Add,
    Subtract,
    Intersect,
    Xor,
}

impl Combine {
    pub const ALL: [Self; 5] = [
        Self::Replace,
        Self::Add,
        Self::Subtract,
        Self::Intersect,
        Self::Xor,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Replace => "Substituir",
            Self::Add => "Adicionar",
            Self::Subtract => "Subtrair",
            Self::Intersect => "Intersectar",
            Self::Xor => "XOR",
        }
    }

    fn tmask_op(self) -> u32 {
        match self {
            Self::Replace => TM_REPLACE,
            Self::Add => TM_ADD,
            Self::Subtract => TM_SUB,
            Self::Intersect => TM_INTERSECT,
            Self::Xor => TM_XOR,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Region {
    Shape(Shape, Bounds),
    Mask(Rc<Mask>),
}

#[derive(Clone)]
pub struct Mask {
    inner: TMask,
}

impl PartialEq for Mask {
    fn eq(&self, other: &Self) -> bool {
        if self.inner.w != other.inner.w || self.inner.h != other.inner.h {
            return false;
        }
        for y in 0..self.inner.h as i32 {
            for x in 0..self.inner.w as i32 {
                if self.inner.get(x, y) != other.inner.get(x, y) {
                    return false;
                }
            }
        }
        true
    }
}

impl Eq for Mask {}

impl std::fmt::Debug for Mask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mask")
            .field("w", &self.inner.w)
            .field("h", &self.inner.h)
            .finish()
    }
}

impl Mask {
    fn empty(size: Point) -> io::Result<Self> {
        let inner = TMask::new(size.0, size.1).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "tamanho de máscara inválido")
        })?;
        Ok(Self { inner })
    }

    fn get(&self, (x, y): Point) -> u8 {
        self.inner.get(x as i32, y as i32)
    }

    fn contains(&self, point: Point) -> bool {
        self.get(point) >= 128
    }

    fn set(&mut self, (x, y): Point, value: u8) {
        if let Some(row) = self.inner.row_w(x as i32, y as i32)
            && !row.is_empty()
        {
            row[0] = value;
        }
    }

    fn bounds(&self, inverted: bool) -> Option<Bounds> {
        let bbox = if inverted {
            let mut copy = self.inner.clone();
            copy.invert();
            copy.bbox()
        } else {
            self.inner.bbox()
        };
        bbox.map(|[x0, y0, x1, y1]| ((x0 as u32, y0 as u32), ((x1 - 1) as u32, (y1 - 1) as u32)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub region: Region,
    pub inverted: bool,
}

impl Selection {
    pub fn new(shape: Shape, start: Point, end: Point) -> Self {
        Self {
            region: Region::Shape(
                shape,
                (
                    (start.0.min(end.0), start.1.min(end.1)),
                    (start.0.max(end.0), start.1.max(end.1)),
                ),
            ),
            inverted: false,
        }
    }

    pub fn from_predicate(
        size: Point,
        mut contains: impl FnMut(Point) -> bool,
    ) -> io::Result<Self> {
        Self::from_coverage(size, |pt| if contains(pt) { 255 } else { 0 })
    }

    pub fn from_coverage(size: Point, mut coverage: impl FnMut(Point) -> u8) -> io::Result<Self> {
        let mut mask = Mask::empty(size)?;
        for ty in 0..mask.inner.tiles_y() {
            for tx in 0..mask.inner.tiles_x() {
                let x0 = tx * 64;
                let y0 = ty * 64;
                let vw = (size.0 - x0).min(64);
                let vh = (size.1 - y0).min(64);
                let Some(tile) = mask.inner.tile_w(tx, ty) else {
                    continue;
                };
                for ry in 0..vh {
                    for rx in 0..vw {
                        tile[(ry as usize) * 64 + rx as usize] = coverage((x0 + rx, y0 + ry));
                    }
                }
            }
        }
        mask.inner.optimize();
        Ok(Self {
            region: Region::Mask(Rc::new(mask)),
            inverted: false,
        })
    }

    pub fn lasso(size: Point, path: &[Point]) -> io::Result<Self> {
        if path.len() < 3 {
            let point = path.first().copied().unwrap_or_default();
            return Ok(Self::new(Shape::Rectangle, point, point));
        }
        let mut mask = Mask::empty(size)?;
        let mut gfx_path = GfxPath::default();
        let mut ras = Raster::default();
        let pts: Vec<(f32, f32)> = path
            .iter()
            .map(|&(x, y)| (x as f32 + 0.5, y as f32 + 0.5))
            .collect();
        mask.inner
            .raster_polygon(&mut gfx_path, &mut ras, &pts, gfx::RULE_EVENODD, TM_REPLACE);
        mask.inner.optimize();
        Ok(Self {
            region: Region::Mask(Rc::new(mask)),
            inverted: false,
        })
    }

    pub fn wand(
        size: Point,
        seed: Point,
        tolerance_pct: u8,
        alpha_mode: super::tolerance::AlphaMatchMode,
        contiguous: bool,
        mut sample: impl FnMut(Point) -> Pixel,
    ) -> io::Result<Self> {
        let reference = sample(seed);
        let mut matches = |point| {
            super::tolerance::color_matches(reference, sample(point), tolerance_pct, alpha_mode)
        };
        if !contiguous {
            return Self::from_predicate(size, matches);
        }
        let mut mask = Self::visit_connected(size, seed, &mut matches, |_| {})?;
        mask.inner.optimize();
        Ok(Self {
            region: Region::Mask(Rc::new(mask)),
            inverted: false,
        })
    }

    // Neighbors share an edge. Physical boundaries include diagonal chains.
    pub(super) fn visit_connected(
        size: Point,
        seed: Point,
        mut matches: impl FnMut(Point) -> bool,
        mut visit: impl FnMut(Point),
    ) -> io::Result<Mask> {
        let mut mask = Mask::empty(size)?;
        if seed.0 >= size.0 || seed.1 >= size.1 || !matches(seed) {
            return Ok(mask);
        }
        let mut queue = VecDeque::from([seed]);
        mask.set(seed, 255);
        while let Some((x, y)) = queue.pop_front() {
            visit((x, y));
            for candidate in [
                x.checked_sub(1).map(|x| (x, y)),
                y.checked_sub(1).map(|y| (x, y)),
                (x + 1 < size.0).then_some((x + 1, y)),
                (y + 1 < size.1).then_some((x, y + 1)),
            ]
            .into_iter()
            .flatten()
            {
                if !mask.contains(candidate) && matches(candidate) {
                    mask.set(candidate, 255);
                    queue.try_reserve(1).map_err(io::Error::other)?;
                    queue.push_back(candidate);
                }
            }
        }
        Ok(mask)
    }

    fn to_tmask(&self, size: Point) -> io::Result<TMask> {
        let mut tmask = match &self.region {
            Region::Mask(mask) => mask.inner.clone(),
            Region::Shape(Shape::Rectangle, (min, max)) => {
                let mut m = Mask::empty(size)?.inner;
                m.fill_rect(
                    min.0 as i32,
                    min.1 as i32,
                    max.0 as i32 + 1,
                    max.1 as i32 + 1,
                    255,
                );
                m
            }
            Region::Shape(Shape::Ellipse, (min, max)) => {
                let mut m = Mask::empty(size)?.inner;
                for y in min.1..=max.1 {
                    for x in min.0..=max.0 {
                        let val = ellipse_coverage((x, y), *min, *max);
                        if val > 0
                            && let Some(row) = m.row_w(x as i32, y as i32)
                            && !row.is_empty()
                        {
                            row[0] = val;
                        }
                    }
                }
                m.optimize();
                m
            }
        };
        if self.inverted {
            tmask.invert();
        }
        Ok(tmask)
    }

    pub fn combine(self, previous: Option<&Self>, size: Point, mode: Combine) -> io::Result<Self> {
        if mode == Combine::Replace {
            return Ok(self);
        }
        let mut dst = match previous {
            Some(prev) => prev.to_tmask(size)?,
            None => Mask::empty(size)?.inner,
        };
        let src = self.to_tmask(size)?;
        dst.combine(&src, mode.tmask_op());
        dst.optimize();
        Ok(Self {
            region: Region::Mask(Rc::new(Mask { inner: dst })),
            inverted: false,
        })
    }

    pub fn feather(self, size: Point, percent: u8) -> io::Result<Self> {
        if percent == 0 {
            return Ok(self);
        }
        let Some((min, max)) = self.bounds(size) else {
            return Ok(self);
        };
        let shorter_side = (max.0 - min.0 + 1).min(max.1 - min.1 + 1);
        let radius = f64::from(shorter_side) * f64::from(percent.min(100)) / 200.0;
        let padding = radius.ceil() as u32;
        let origin = (min.0.saturating_sub(padding), min.1.saturating_sub(padding));
        let end = (
            max.0.saturating_add(padding).min(size.0 - 1),
            max.1.saturating_add(padding).min(size.1 - 1),
        );
        if min == (0, 0)
            && max == (size.0 - 1, size.1 - 1)
            && matches!(self.region, Region::Shape(Shape::Rectangle, _))
            && !self.inverted
        {
            return Ok(self);
        }
        let mut mask = Mask::empty(size)?;
        super::feather::expand(
            (end.0 - origin.0 + 1, end.1 - origin.1 + 1),
            radius,
            |(x, y)| self.coverage_u8((origin.0 + x, origin.1 + y)),
            |(x, y), coverage| mask.set((origin.0 + x, origin.1 + y), coverage),
        )?;
        mask.inner.optimize();
        Ok(Self {
            region: Region::Mask(Rc::new(mask)),
            inverted: false,
        })
    }

    pub fn fit_to(&self, source: Point, destination: Point) -> io::Result<Self> {
        if source == destination {
            return Ok(self.clone());
        }
        Self::from_coverage(destination, |(x, y)| {
            if x < source.0 && y < source.1 {
                self.coverage_u8((x, y))
            } else {
                0
            }
        })
    }

    pub fn bounds(&self, size: Point) -> Option<Bounds> {
        match &self.region {
            Region::Mask(mask) => mask.bounds(self.inverted),
            Region::Shape(shape, (min, max)) => {
                if !self.inverted {
                    return Some((*min, *max));
                }
                if *shape == Shape::Rectangle {
                    if *min == (0, 0) && *max == (size.0 - 1, size.1 - 1) {
                        return None;
                    }
                    let y0 = if min.0 == 0 && max.0 + 1 == size.0 && min.1 == 0 {
                        max.1 + 1
                    } else {
                        0
                    };
                    let y1 = if min.0 == 0 && max.0 + 1 == size.0 && max.1 + 1 == size.1 {
                        min.1.saturating_sub(1)
                    } else {
                        size.1 - 1
                    };
                    let x0 = if min.1 == 0 && max.1 + 1 == size.1 && min.0 == 0 {
                        max.0 + 1
                    } else {
                        0
                    };
                    let x1 = if min.1 == 0 && max.1 + 1 == size.1 && max.0 + 1 == size.0 {
                        min.0.saturating_sub(1)
                    } else {
                        size.0 - 1
                    };
                    if x0 > x1 || y0 > y1 {
                        return None;
                    }
                    return Some(((x0, y0), (x1, y1)));
                }
                if max.0 - min.0 >= 2 && max.1 - min.1 >= 2 {
                    return Some(((0, 0), (size.0 - 1, size.1 - 1)));
                }
                let mut result: Option<Bounds> = None;
                for y in 0..size.1 {
                    for x in 0..size.0 {
                        if self.contains((x, y)) {
                            result = Some(match result {
                                None => ((x, y), (x, y)),
                                Some((bmin, bmax)) => ((bmin.0.min(x), bmin.1), (bmax.0.max(x), y)),
                            });
                        }
                    }
                }
                result
            }
        }
    }

    pub fn coverage_u8(&self, point: Point) -> u8 {
        let raw = match &self.region {
            Region::Mask(mask) => mask.get(point),
            Region::Shape(Shape::Rectangle, (min, max)) => {
                if (min.0..=max.0).contains(&point.0) && (min.1..=max.1).contains(&point.1) {
                    255
                } else {
                    0
                }
            }
            Region::Shape(Shape::Ellipse, (min, max)) => ellipse_coverage(point, *min, *max),
        };
        if self.inverted { 255 - raw } else { raw }
    }

    pub fn contains(&self, point: Point) -> bool {
        let inside = match &self.region {
            Region::Mask(mask) => mask.contains(point),
            Region::Shape(shape, (min, max)) => {
                (min.0..=max.0).contains(&point.0)
                    && (min.1..=max.1).contains(&point.1)
                    && match shape {
                        Shape::Rectangle => true,
                        Shape::Ellipse => ellipse_contains(point, *min, *max),
                    }
            }
        };
        inside != self.inverted
    }
}

fn ellipse_contains((x, y): Point, min: Point, max: Point) -> bool {
    if !(min.0..=max.0).contains(&x) || !(min.1..=max.1).contains(&y) {
        return false;
    }
    let nx = (2.0 * f64::from(x - min.0) + 1.0) / (f64::from(max.0 - min.0) + 1.0) - 1.0;
    let ny = (2.0 * f64::from(y - min.1) + 1.0) / (f64::from(max.1 - min.1) + 1.0) - 1.0;
    nx * nx + ny * ny <= 1.0
}

fn ellipse_coverage((x, y): Point, min: Point, max: Point) -> u8 {
    use super::stroke_raster::samples;

    if !(min.0..=max.0).contains(&x) || !(min.1..=max.1).contains(&y) {
        return 0;
    }
    let width = f64::from(max.0 - min.0) + 1.0;
    let height = f64::from(max.1 - min.1) + 1.0;
    let nx = (2.0 * f64::from(x - min.0) + 1.0) / width - 1.0;
    let ny = (2.0 * f64::from(y - min.1) + 1.0) / height - 1.0;
    let furthest = (nx.abs() + 0.75 / width).powi(2) + (ny.abs() + 0.75 / height).powi(2);
    let nearest =
        (nx.abs() - 0.75 / width).max(0.0).powi(2) + (ny.abs() - 0.75 / height).max(0.0).powi(2);
    if furthest <= 1.0 {
        255
    } else if nearest > 1.0 {
        0
    } else {
        let count = samples(|sx, sy| {
            (nx + 2.0 * sx / width).powi(2) + (ny + 2.0 * sy / height).powi(2) <= 1.0
        })
        .count_ones();
        ((count * 255 + 8) / 16) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feather_scales_from_zero_to_half_the_shorter_side_and_clips_to_canvas() -> io::Result<()> {
        let size = (32, 16);
        let rectangle = Selection::new(Shape::Rectangle, (8, 0), (23, 7));
        assert_eq!(rectangle.clone().feather(size, 0)?, rectangle);

        let full = rectangle.clone().feather(size, 100)?;
        let half = rectangle.feather(size, 50)?;
        assert_eq!(full.bounds(size), Some(((4, 0), (27, 11))));
        assert_eq!(half.bounds(size), Some(((6, 0), (25, 9))));
        assert_eq!(full.coverage_u8((15, 7)), 255);
        for (y, expected) in [(8, 223), (9, 159), (10, 96), (11, 32), (12, 0)] {
            assert_eq!(full.coverage_u8((15, y)), expected);
        }
        assert!(full.coverage_u8((24, 8)) < full.coverage_u8((15, 8)));
        Ok(())
    }

    #[test]
    fn lasso_and_all_boolean_operations_share_the_same_pixel_mask() -> io::Result<()> {
        let size = (9, 8);
        let lasso = Selection::lasso(size, &[(1, 1), (6, 1), (1, 6)])?;
        assert!(lasso.contains((2, 2)));
        assert!(!lasso.contains((5, 5)));
        let rectangle = Selection::new(Shape::Rectangle, (2, 2), (7, 7));
        for mode in Combine::ALL {
            let mask = rectangle.clone().combine(Some(&lasso), size, mode)?;
            for y in 0..size.1 {
                for x in 0..size.0 {
                    let point = (x, y);
                    let a = lasso.contains(point);
                    let b = rectangle.contains(point);
                    let expected = match mode {
                        Combine::Replace => b,
                        Combine::Add => a || b,
                        Combine::Subtract => a && !b,
                        Combine::Intersect => a && b,
                        Combine::Xor => a != b,
                    };
                    assert_eq!(mask.contains(point), expected);
                }
            }
        }
        Ok(())
    }

    #[test]
    fn wand_can_cross_matches_but_not_gaps_and_mask_paste_stays_in_bounds() -> io::Result<()> {
        let sample = |(x, _)| {
            [
                Pixel([10, 0, 0, 255]),
                Pixel([20, 0, 0, 255]),
                Pixel([80, 0, 0, 255]),
                Pixel([10, 0, 0, 255]),
            ][x as usize]
        };
        let contiguous = Selection::wand(
            (4, 1),
            (0, 0),
            10,
            super::super::tolerance::AlphaMatchMode::Premultiplied,
            true,
            sample,
        )?;
        let global = Selection::wand(
            (4, 1),
            (0, 0),
            10,
            super::super::tolerance::AlphaMatchMode::Premultiplied,
            false,
            sample,
        )?;
        assert!(contiguous.contains((1, 0)));
        assert!(!contiguous.contains((3, 0)));
        assert!(global.contains((3, 0)));
        assert!(!global.contains((2, 0)));
        let mut inverted = global;
        inverted.inverted = true;
        let pasted = inverted.fit_to((4, 1), (9, 1))?;
        assert_eq!(pasted.bounds((9, 1)), Some(((2, 0), (2, 0))));
        Ok(())
    }

    #[test]
    fn inverted_full_rectangle_has_empty_bounds_and_ellipse_ssaa_is_smooth() {
        let mut all = Selection::new(Shape::Rectangle, (0, 0), (7, 7));
        all.inverted = true;
        assert_eq!(all.bounds((8, 8)), None);

        let ellipse = Selection::new(Shape::Ellipse, (0, 0), (6, 6));
        let mut covs = Vec::new();
        for y in 0..=6 {
            for x in 0..=6 {
                covs.push(ellipse.coverage_u8((x, y)));
            }
        }
        assert!(covs.iter().any(|&v| (1..255).contains(&v)));
    }
}
