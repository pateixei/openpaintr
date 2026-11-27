//! Where text ink lands, recorded before architecture plan item 2.5 replaced
//! the glyph atlas with outlines. Placement must stay within one pixel; edge
//! coverage is expected to differ.

use eframe::egui::vec2;

use super::{super::text_tool::FontCatalog, *};

/// The bounding box of pixels at least half covered, and the center of mass of
/// all coverage.
#[derive(Debug)]
struct Ink {
    bounds: [i32; 4],
    center: [f64; 2],
}

fn ink(pixels: &[Pixel], width: u32) -> Ink {
    let mut bounds = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
    let (mut mass, mut sum_x, mut sum_y) = (0.0, 0.0, 0.0);
    for (index, pixel) in pixels.iter().enumerate() {
        let (x, y) = (index as u32 % width, index as u32 / width);
        let alpha = f64::from(pixel.0[3]);
        mass += alpha;
        sum_x += alpha * (f64::from(x) + 0.5);
        sum_y += alpha * (f64::from(y) + 0.5);
        if pixel.0[3] >= 128 {
            let (x, y) = (x as i32, y as i32);
            bounds = [
                bounds[0].min(x),
                bounds[1].min(y),
                bounds[2].max(x),
                bounds[3].max(y),
            ];
        }
    }
    assert!(mass > 0.0, "nothing was drawn");
    Ink {
        bounds,
        center: [sum_x / mass, sum_y / mass],
    }
}

fn cases() -> Vec<(String, f32, String, TextParams)> {
    let plain = TextParams::default();
    let styled = |change: fn(&mut TextParams)| {
        let mut params = TextParams::default();
        change(&mut params);
        params
    };
    let mut cases = Vec::new();
    // Including the sizes where the layout resolution changes.
    for size in [24.0, 48.0, 61.0, 81.0, 96.0, 121.0, 240.0] {
        let mut add = |name: &str, text: &str, params: TextParams| {
            cases.push((format!("{size}/{name}"), size, text.to_owned(), params));
        };
        add("kerning", "AVATAR To Wy", plain.clone());
        add("lines", "Hamburg ffi\njpqy Ag\nx", plain.clone());
        add("arabic", "مرحبا بالعالم", plain.clone());
        add("japanese", "こんにちは日本", plain.clone());
        add("chinese", "汉字测试样例", plain.clone());
        add("missing", "a\u{E000}\t.b", plain.clone());
        add(
            "monospace",
            "iW0O_|",
            styled(|p| p.family = text_tool::MONOSPACE_FAMILY.into()),
        );
        add("bold", "Ag Wy\nlil", styled(|p| p.bold = true));
        add("italic", "Ag Wy\nlil", styled(|p| p.italic = true));
        add("underline", "Ag Wy\nlil", styled(|p| p.underline = true));
        add(
            "strikethrough",
            "Ag Wy\nlil",
            styled(|p| p.strikethrough = true),
        );
        add(
            "all-styles",
            "Ag Wy\nlil",
            styled(|p| {
                p.bold = true;
                p.italic = true;
                p.underline = true;
                p.strikethrough = true;
            }),
        );
        add(
            "center",
            "Wide line\ni",
            styled(|p| p.align = TextAlign::Center),
        );
        add(
            "right",
            "Wide line\ni",
            styled(|p| p.align = TextAlign::Right),
        );
    }
    cases
}

fn render(
    catalog: &mut FontCatalog,
    size_px: f32,
    text: &str,
    mut params: TextParams,
    style: TextStyle,
) -> Ink {
    params.size_px = size_px;
    let size = ((size_px * 9.0) as u32, (size_px * 4.2) as u32);
    let x = match params.align {
        TextAlign::Left => 0.0,
        TextAlign::Center => 0.5,
        TextAlign::Right => 1.0,
    };
    // A fractional anchor, away from the edges.
    let anchor = vec2(
        size_px * 0.5 + x * (size.0 as f32 - size_px) + 0.3,
        size_px * 0.3 + 0.6,
    );
    let mut session = TextSession::new(anchor, Pixel([0, 0, 0, 255]), params);
    session.text = text.to_owned();
    let base = vec![Pixel::default(); (size.0 * size.1) as usize];
    ink(
        &session.render_preview(catalog, &base, size, None, AaMode::Analytical, style),
        size.0,
    )
}

#[test]
fn text_ink_stays_within_one_pixel() {
    // The recordings predate Sharp text; none of these cases has a bold file.
    for style in TextStyle::ALL {
        check_placement(style);
    }
}

fn check_placement(style: TextStyle) {
    let mut catalog = FontCatalog::new();
    let actual: Vec<_> = cases()
        .into_iter()
        .map(|(name, size, text, params)| (name, render(&mut catalog, size, &text, params, style)))
        .collect();
    assert_eq!(actual.len(), PLACEMENT.len());
    let moved: Vec<String> = actual
        .iter()
        .zip(PLACEMENT)
        .filter_map(|((name, ink), (key, bounds, center))| {
            assert_eq!(name, key);
            let edge = (0..4)
                .map(|side| (ink.bounds[side] - bounds[side]).abs())
                .max()
                .unwrap();
            let shift = (ink.center[0] - center[0]).hypot(ink.center[1] - center[1]);
            (edge > 1 || shift > 1.0).then(|| {
                format!(
                    "{name}: edges moved {edge} px {:?} -> {:?}, center {shift:.2} px",
                    bounds, ink.bounds
                )
            })
        })
        .collect();
    assert!(moved.is_empty(), "{style:?} moved:\n{}", moved.join("\n"));
}

/// `(case, [left, top, right, bottom], center of mass)`.
type Placement = (&'static str, [i32; 4], [f64; 2]);

const PLACEMENT: &[Placement] = &[
    ("24/kerning", [13, 13, 171, 33], [91.38, 21.95]),
    ("24/lines", [11, 11, 139, 84], [63.90, 36.63]),
    ("24/arabic", [12, 12, 126, 37], [63.50, 24.26]),
    ("24/japanese", [16, 11, 178, 32], [108.01, 22.82]),
    ("24/chinese", [13, 11, 153, 32], [88.81, 21.43]),
    ("24/missing", [13, 11, 89, 29], [49.07, 22.85]),
    ("24/monospace", [15, 12, 92, 35], [50.82, 22.16]),
    ("24/bold", [13, 13, 80, 57], [41.12, 29.70]),
    ("24/italic", [13, 13, 83, 57], [42.62, 29.38]),
    ("24/underline", [12, 13, 80, 63], [40.55, 33.12]),
    ("24/strikethrough", [12, 13, 80, 57], [40.41, 28.89]),
    ("24/all-styles", [12, 13, 84, 63], [42.45, 31.73]),
    ("24/center", [60, 11, 155, 56], [106.46, 24.04]),
    ("24/right", [106, 11, 202, 56], [155.66, 24.04]),
    ("48/kerning", [25, 26, 343, 68], [182.63, 43.67]),
    ("48/lines", [21, 23, 278, 169], [127.84, 73.10]),
    ("48/arabic", [25, 24, 252, 76], [126.80, 48.53]),
    ("48/japanese", [33, 22, 357, 65], [216.07, 45.65]),
    ("48/chinese", [26, 22, 308, 65], [177.44, 42.89]),
    ("48/missing", [27, 23, 178, 59], [98.15, 45.48]),
    ("48/monospace", [31, 23, 184, 70], [101.51, 44.07]),
    ("48/bold", [25, 26, 160, 114], [82.01, 59.27]),
    ("48/italic", [25, 26, 167, 114], [84.78, 58.76]),
    ("48/underline", [24, 26, 160, 127], [80.84, 66.14]),
    ("48/strikethrough", [24, 26, 160, 114], [80.57, 57.53]),
    ("48/all-styles", [24, 26, 169, 127], [84.68, 63.27]),
    ("48/center", [119, 23, 311, 114], [212.64, 47.82]),
    ("48/right", [212, 23, 404, 114], [310.98, 47.82]),
    ("61/kerning", [32, 33, 437, 86], [232.01, 55.25]),
    ("61/lines", [27, 28, 354, 215], [162.28, 92.55]),
    ("61/arabic", [31, 30, 320, 96], [161.09, 61.17]),
    ("61/japanese", [41, 27, 455, 82], [274.31, 57.42]),
    ("61/chinese", [33, 27, 391, 82], [225.44, 53.90]),
    ("61/missing", [34, 28, 226, 75], [125.06, 57.65]),
    ("61/monospace", [39, 29, 234, 89], [128.93, 55.73]),
    ("61/bold", [32, 33, 203, 145], [103.80, 74.98]),
    ("61/italic", [32, 33, 212, 145], [107.49, 74.50]),
    ("61/underline", [30, 33, 203, 160], [102.63, 83.34]),
    ("61/strikethrough", [30, 33, 203, 145], [102.29, 72.95]),
    ("61/all-styles", [30, 33, 214, 160], [107.26, 79.97]),
    ("61/center", [151, 28, 396, 145], [270.27, 60.48]),
    ("61/right", [269, 28, 514, 145], [395.04, 60.49]),
    ("81/kerning", [42, 44, 580, 115], [308.05, 73.39]),
    ("81/lines", [36, 38, 470, 285], [215.76, 122.89]),
    ("81/arabic", [42, 39, 426, 128], [213.84, 81.27]),
    ("81/japanese", [55, 36, 604, 109], [364.20, 76.16]),
    ("81/chinese", [44, 35, 520, 109], [299.22, 71.50]),
    ("81/missing", [45, 38, 301, 100], [165.75, 76.40]),
    ("81/monospace", [51, 38, 311, 118], [170.90, 73.69]),
    ("81/bold", [42, 44, 270, 193], [137.31, 99.44]),
    ("81/italic", [42, 44, 281, 193], [142.67, 98.89]),
    ("81/underline", [40, 44, 270, 213], [136.21, 110.58]),
    ("81/strikethrough", [40, 44, 270, 193], [135.78, 96.83]),
    ("81/all-styles", [40, 44, 284, 213], [141.94, 106.20]),
    ("81/center", [201, 38, 526, 192], [358.86, 80.34]),
    ("81/right", [358, 38, 683, 192], [524.63, 80.34]),
    ("96/kerning", [49, 52, 687, 136], [365.18, 86.85]),
    ("96/lines", [42, 44, 557, 338], [255.55, 145.49]),
    ("96/arabic", [49, 47, 505, 152], [253.40, 96.35]),
    ("96/japanese", [65, 42, 716, 130], [431.83, 90.34]),
    ("96/chinese", [52, 42, 616, 130], [354.68, 84.82]),
    ("96/missing", [53, 45, 357, 119], [196.23, 90.49]),
    ("96/monospace", [61, 45, 369, 140], [202.90, 87.16]),
    ("96/bold", [49, 52, 319, 229], [162.49, 117.69]),
    ("96/italic", [50, 52, 334, 229], [169.11, 117.16]),
    ("96/underline", [48, 52, 320, 253], [161.45, 131.17]),
    ("96/strikethrough", [48, 52, 320, 229], [160.92, 114.65]),
    ("96/all-styles", [48, 52, 336, 253], [167.96, 125.95]),
    ("96/center", [238, 45, 624, 228], [425.23, 95.10]),
    ("96/right", [424, 45, 810, 228], [621.86, 95.10]),
    ("121/kerning", [62, 65, 867, 171], [460.32, 109.31]),
    ("121/lines", [53, 56, 702, 427], [321.98, 183.21]),
    ("121/arabic", [62, 59, 636, 191], [319.37, 121.25]),
    ("121/japanese", [82, 53, 903, 164], [544.18, 113.89]),
    ("121/chinese", [66, 53, 777, 164], [446.96, 106.93]),
    ("121/missing", [67, 56, 450, 150], [247.53, 113.81]),
    ("121/monospace", [77, 57, 465, 177], [255.61, 109.80]),
    ("121/bold", [62, 66, 402, 289], [204.40, 148.05]),
    ("121/italic", [63, 66, 421, 289], [213.13, 147.49]),
    ("121/underline", [60, 66, 403, 319], [203.49, 164.89]),
    ("121/strikethrough", [60, 66, 403, 289], [202.84, 144.38]),
    ("121/all-styles", [60, 66, 424, 319], [211.33, 158.49]),
    ("121/center", [300, 56, 786, 288], [536.09, 119.75]),
    ("121/right", [534, 56, 1021, 288], [784.08, 119.75]),
    ("240/kerning", [123, 128, 1720, 340], [912.58, 216.34]),
    ("240/lines", [105, 110, 1393, 847], [638.57, 363.04]),
    ("240/arabic", [123, 115, 1263, 380], [633.14, 239.99]),
    ("240/japanese", [162, 105, 1792, 324], [1079.37, 225.08]),
    ("240/chinese", [131, 104, 1542, 325], [886.22, 211.31]),
    ("240/missing", [132, 110, 893, 298], [490.86, 225.29]),
    ("240/monospace", [152, 112, 924, 351], [507.01, 217.61]),
    ("240/bold", [123, 130, 797, 573], [403.41, 292.86]),
    ("240/italic", [123, 130, 837, 573], [422.34, 292.28]),
    ("240/underline", [120, 130, 800, 632], [403.32, 326.58]),
    ("240/strikethrough", [120, 130, 800, 573], [402.05, 286.02]),
    ("240/all-styles", [120, 130, 839, 632], [417.37, 314.41]),
    ("240/center", [594, 110, 1560, 571], [1062.66, 237.13]),
    ("240/right", [1059, 110, 2025, 571], [1554.45, 237.13]),
];
