use std::{io::Cursor, sync::OnceLock};

use eframe::egui::{
    self, Color32, ColorImage, Painter, Rect, TextureHandle, TextureOptions, pos2, vec2,
};

const ART: &[u8] = include_bytes!("../../app-icons.png");
const TILE: usize = 50;
const COLUMNS: usize = 8;
const SOURCE_WIDTH: usize = 1536;
const SOURCE_HEIGHT: usize = 1024;

// Centers of the glyphs in the supplied card. Labels and group headers are outside
// the samples. Only entries used by the editor are packed into the GPU texture.
macro_rules! icons {
    ($($name:ident: ($x:literal, $y:literal)),* $(,)?) => {
        #[derive(Clone, Copy)]
        pub enum Icon { $($name),* }

        const CENTERS: &[(usize, usize)] = &[$(($x, $y)),*];
    };
}

icons! {
    New: (64, 207),
    Open: (143, 207),
    Save: (223, 207),
    SaveAs: (304, 206),
    Export: (464, 206),
    Cut: (576, 205),
    Copy: (657, 205),
    Paste: (737, 205),
    Delete: (818, 205),
    Undo: (897, 205),
    Redo: (979, 205),
    ZoomIn: (1083, 205),
    ZoomOut: (1161, 205),
    Fit: (1239, 204),
    ActualSize: (1320, 204),
    Grid: (1399, 204),
    Rulers: (1478, 204),
    Rectangle: (147, 392),
    Ellipse: (228, 392),
    Lasso: (300, 392),
    Wand: (367, 392),
    Crop: (436, 392),
    Move: (502, 392),
    Transform: (577, 392),
    Hand: (657, 392),
    Zoom: (731, 392),
    Brush: (830, 392),
    Eraser: (910, 392),
    Eyedropper: (998, 392),
    Bucket: (1095, 392),
    Clone: (1177, 392),
    Text: (56, 571),
    Line: (122, 571),
    Curve: (190, 571),
    ShapeRect: (264, 571),
    ShapeRoundRect: (342, 571),
    ShapeEllipse: (414, 571),
    ShapePolygon: (478, 571),
    ShapeStar: (542, 571),
    NewLayer: (638, 571),
    Duplicate: (706, 571),
    DeleteLayer: (773, 571),
    LayerUp: (836, 571),
    LayerDown: (896, 571),
    MergeDown: (958, 571),
    Swap: (1388, 572),
    Settings: (730, 923),
    Exit: (1464, 922),
}

struct Atlas {
    light: ColorImage,
    dark: ColorImage,
}

impl Atlas {
    fn decode() -> Option<Self> {
        let mut reader = png::Decoder::new(Cursor::new(ART)).read_info().ok()?;
        let mut bytes = vec![0; reader.output_buffer_size()?];
        let frame = reader.next_frame(&mut bytes).ok()?;
        if frame.width as usize != SOURCE_WIDTH
            || frame.height as usize != SOURCE_HEIGHT
            || frame.color_type != png::ColorType::Rgba
            || frame.bit_depth != png::BitDepth::Eight
        {
            return None;
        }
        let size = [COLUMNS * TILE, CENTERS.len().div_ceil(COLUMNS) * TILE];
        let mut light = ColorImage::filled(size, Color32::TRANSPARENT);
        let mut dark = ColorImage::filled(size, Color32::TRANSPARENT);
        let sample = |x, y| -> [u8; 3] {
            let offset = (y * SOURCE_WIDTH + x) * 4;
            bytes[offset..offset + 3].try_into().unwrap()
        };

        for (index, &(x, y)) in CENTERS.iter().enumerate() {
            let (left, top) = (x - TILE / 2, y - TILE / 2);
            // The card uses a slightly different pastel behind some glyphs.
            // Sample each local background, then remove it from the texture.
            let background = std::array::from_fn(|channel| {
                let mut edge = Vec::with_capacity(TILE * 4);
                for i in 0..TILE {
                    for (x, y) in [
                        (left + i, top),
                        (left + i, top + TILE - 1),
                        (left, top + i),
                        (left + TILE - 1, top + i),
                    ] {
                        edge.push(sample(x, y)[channel]);
                    }
                }
                edge.sort_unstable();
                edge[edge.len() / 2]
            });
            for dy in 1..TILE - 1 {
                for dx in 1..TILE - 1 {
                    let (foreground, alpha) =
                        separate_background(sample(left + dx, top + dy), background);
                    if alpha == 0 {
                        continue;
                    }
                    let dest =
                        (index / COLUMNS * TILE + dy) * size[0] + index % COLUMNS * TILE + dx;
                    let [r, g, b] = foreground;
                    light.pixels[dest] = Color32::from_rgba_unmultiplied(r, g, b, alpha);
                    let [r, g, b] = for_dark_theme(foreground);
                    dark.pixels[dest] = Color32::from_rgba_unmultiplied(r, g, b, alpha);
                }
            }
        }
        Some(Self { light, dark })
    }
}

fn separate_background(pixel: [u8; 3], background: [u8; 3]) -> ([u8; 3], u8) {
    let difference = pixel
        .into_iter()
        .zip(background)
        .map(|(p, b)| p.abs_diff(b))
        .max()
        .unwrap();
    if difference <= 12 {
        return ([0; 3], 0);
    }
    let alpha = ((f32::from(difference) - 12.0) / 55.0).min(1.0);
    let color = std::array::from_fn(|channel| {
        let bg = f32::from(background[channel]);
        (bg + (f32::from(pixel[channel]) - bg) / alpha)
            .round()
            .clamp(0.0, 255.0) as u8
    });
    (color, (alpha * 255.0).round() as u8)
}

fn for_dark_theme([r, g, b]: [u8; 3]) -> [u8; 3] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max < 112 {
        return [224, 225, 248];
    }
    if max - min > 55 && b > r && b > g {
        return [
            r.saturating_add(52),
            g.saturating_add(47),
            b.saturating_add(18),
        ];
    }
    [r, g, b]
}

fn texture(ctx: &egui::Context, dark: bool) -> Option<TextureHandle> {
    static ATLAS: OnceLock<Option<Atlas>> = OnceLock::new();
    let id = egui::Id::new(("app-icon-atlas", dark));
    if let Some(texture) = ctx.data(|data| data.get_temp::<TextureHandle>(id)) {
        return Some(texture);
    }
    let atlas = ATLAS.get_or_init(Atlas::decode).as_ref()?;
    let image = if dark { &atlas.dark } else { &atlas.light };
    let texture = ctx.load_texture("Ícones do OpenPaintR", image.clone(), TextureOptions::LINEAR);
    ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
    Some(texture)
}

impl Icon {
    fn uv(self) -> Rect {
        let tile = self as usize;
        let rows = CENTERS.len().div_ceil(COLUMNS);
        let (column, row) = (tile % COLUMNS, tile / COLUMNS);
        let coord = |x, y| pos2(x as f32 / COLUMNS as f32, y as f32 / rows as f32);
        Rect::from_min_max(coord(column, row), coord(column + 1, row + 1))
    }

    pub fn paint(self, painter: &Painter, rect: Rect, dark: bool, enabled: bool) {
        if let Some(texture) = texture(painter.ctx(), dark) {
            let tint = if enabled {
                Color32::WHITE
            } else {
                Color32::WHITE.gamma_multiply(0.38)
            };
            painter.image(texture.id(), rect, self.uv(), tint);
        }
    }

    pub fn image(self, ui: &egui::Ui, size: f32) -> Option<egui::Image<'static>> {
        let texture = texture(ui.ctx(), ui.visuals().dark_mode)?;
        Some(egui::Image::new((texture.id(), vec2(size, size))).uv(self.uv()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_supplied_card_produces_transparent_glyphs_for_both_themes() {
        let atlas = Atlas::decode().expect("card PNG válido");
        for (index, _) in CENTERS.iter().enumerate() {
            let left = index % COLUMNS * TILE;
            let top = index / COLUMNS * TILE;
            let mut visible = 0;
            for row in top..top + TILE {
                for col in left..left + TILE {
                    let index = row * atlas.light.size[0] + col;
                    visible += usize::from(atlas.light.pixels[index].a() > 127);
                    assert_eq!(atlas.light.pixels[index].a(), atlas.dark.pixels[index].a());
                }
            }
            assert!(
                (55..TILE * TILE / 2).contains(&visible),
                "glifo {index}: {visible} pixels visíveis"
            );
        }
    }
}
