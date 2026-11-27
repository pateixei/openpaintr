//! Golden tests that pin, byte for byte, the affine resampling that
//! architecture plan item 2.2 rewrites: transform and floating-paste pixels,
//! selection masks, undo results, and image resizing.
//!
//! The hashes were recorded from the implementation before that change. The
//! sources have opaque pixels on their edges and colored transparent pixels,
//! so a destination rectangle that is a pixel too small changes the output.

use super::{
    super::{
        clipboard::Image,
        geometry::{self, Affine, Resampling},
        selection::{Combine, Selection, Shape},
    },
    *,
};

const CANVAS: (u32, u32) = (41, 29);
const ANGLES: [f64; 9] = [0.0, 15.0, 45.0, 90.0, 180.0, 270.0, 37.5, -12.3, 359.9];
const SCALES: [(f64, f64); 6] = [
    (1.0, 1.0),
    (2.0, 2.0),
    (0.5, 0.5),
    (1.7, 0.6),
    (0.05, 3.0),
    (32.0, 32.0),
];
/// Including sub-pixel moves and content pushed partly or wholly off the canvas.
const SHIFTS: [(f64, f64); 5] = [
    (0.0, 0.0),
    (3.0, -2.0),
    (0.5, 0.25),
    (-30.0, 17.5),
    (200.0, 0.0),
];

/// 64-bit FNV-1a.
#[derive(Clone, Copy)]
struct Hash(u64);

impl Hash {
    const EMPTY: Self = Self(0xcbf2_9ce4_8422_2325);

    fn bytes(&mut self, bytes: impl IntoIterator<Item = u8>) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
        }
    }

    fn pixels(&mut self, pixels: &[Pixel]) {
        self.bytes((pixels.len() as u64).to_le_bytes());
        self.bytes(pixels.iter().flat_map(|pixel| pixel.0));
    }

    fn selection(&mut self, selection: Option<&Selection>) {
        let Some(selection) = selection else {
            return self.bytes([0]);
        };
        self.bytes([1]);
        self.bytes(
            (0..CANVAS.1).flat_map(|y| (0..CANVAS.0).map(move |x| selection.coverage_u8((x, y)))),
        );
    }
}

#[track_caller]
fn check(actual: &[(String, Hash)], expected: &[(&str, u64)]) {
    let matches = actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|((name, hash), (key, value))| name == key && hash.0 == *value);
    let table: String = actual
        .iter()
        .map(|(name, hash)| format!("    (\"{name}\", {:#018x}),\n", hash.0))
        .collect();
    assert!(matches, "output changed; actual:\n{table}");
}

/// Deterministic pixels: opaque, translucent, and colored but transparent.
fn texture(width: u32, height: u32, seed: u32) -> Vec<Pixel> {
    (0..width * height)
        .map(|index| {
            let mixed = (index + seed).wrapping_mul(2_654_435_761);
            let [r, g, b, a] = mixed.rotate_left(11).to_le_bytes();
            let x = index % width;
            let y = index / width;
            let edge = x == 0 || y == 0 || x == width - 1 || y == height - 1;
            let alpha = match mixed >> 29 {
                _ if edge => 255,
                0 => 0,
                1..=3 => 255,
                _ => a,
            };
            Pixel([r, g, b, alpha])
        })
        .collect()
}

fn editor() -> io::Result<Editor> {
    let mut editor = Editor::new(Document::new(CANVAS.0, CANVAS.1)?);
    let mut pixels = texture(CANVAS.0, CANVAS.1, 1);
    editor.document.swap_layer_pixels(0, &mut pixels)?;
    Ok(editor)
}

fn selections() -> io::Result<Vec<(&'static str, Selection)>> {
    let ellipse = Selection::new_with_aa(Shape::Ellipse, (5, 4), (23, 19), AaMode::Ssaa4x4);
    let mut inverted = Selection::new(Shape::Rectangle, (9, 7), (30, 20));
    inverted.inverted = true;
    // Touches the right and bottom edges of the canvas.
    let corner = Selection::new(Shape::Rectangle, (33, 22), (40, 28));
    let feathered =
        Selection::lasso(CANVAS, &[(2, 25), (20, 2), (38, 18), (21, 21)])?.feather(CANVAS, 30)?;
    // Two separate parts, so the bounding box has a large empty interior.
    let apart = Selection::new(Shape::Rectangle, (35, 1), (37, 3)).combine(
        Some(&Selection::new(Shape::Rectangle, (1, 24), (3, 26))),
        CANVAS,
        Combine::Add,
    )?;
    Ok(vec![
        ("ellipse", ellipse),
        ("inverted", inverted),
        ("corner", corner),
        ("feathered", feathered),
        ("apart", apart),
    ])
}

fn floating_images() -> [(&'static str, Image); 4] {
    let image = |size: (u32, u32), origin| Image {
        size,
        origin,
        pixels: texture(size.0, size.1, origin.0 + 7),
    };
    [
        ("small", image((8, 6), (14, 11))),
        ("pixel", image((1, 1), (20, 14))),
        ("clipped", image((17, 13), (33, 22))),
        ("oversized", image((53, 37), (0, 0))),
    ]
}

/// Every transform in the matrix, pivoting on the given content center.
fn matrix(center: (f64, f64)) -> impl Iterator<Item = Affine> {
    ANGLES.into_iter().flat_map(move |degrees| {
        SCALES.into_iter().flat_map(move |scale| {
            SHIFTS.into_iter().map(move |shift| Affine {
                center,
                shift,
                scale,
                degrees,
            })
        })
    })
}

/// A dense angle sweep, which catches changes in floating-point rounding.
fn sweep(center: (f64, f64)) -> impl Iterator<Item = Affine> {
    (0..400).map(move |step| Affine {
        center,
        shift: (f64::from(step % 7) * 0.5, f64::from(step % 5) * -0.25),
        scale: (1.0 + f64::from(step % 3) * 0.5, 1.0),
        degrees: f64::from(step) * 0.9,
    })
}

#[test]
fn layer_and_selection_transforms_are_identical() -> io::Result<()> {
    let mut results = Vec::new();
    let mut editor = editor()?;
    let center = (f64::from(CANVAS.0) * 0.5, f64::from(CANVAS.1) * 0.5);
    for method in Resampling::ALL {
        let mut hash = Hash::EMPTY;
        for transform in matrix(center).chain(sweep(center)) {
            hash.pixels(&editor.transform_pixels(transform, method, false, false)?);
        }
        results.push((format!("layer/{method:?}"), hash));
    }
    for (name, selection) in selections()? {
        let (min, max) = selection.bounds(CANVAS).expect("bounds");
        let middle = (
            f64::from(min.0 + max.0 + 1) * 0.5,
            f64::from(min.1 + max.1 + 1) * 0.5,
        );
        editor.selection = Some(selection);
        for method in Resampling::ALL {
            for clone_source in [false, true] {
                let mut hash = Hash::EMPTY;
                for transform in matrix(middle) {
                    hash.pixels(&editor.transform_pixels(transform, method, true, clone_source)?);
                }
                results.push((format!("{name}/{method:?}/clone={clone_source}"), hash));
            }
        }
        let mut hash = Hash::EMPTY;
        for transform in matrix(middle).chain(sweep(middle)) {
            editor.selection = Some(selections()?.into_iter().find(|s| s.0 == name).unwrap().1);
            editor.transform_mask(transform)?;
            hash.selection(editor.selection.as_ref());
        }
        results.push((format!("{name}/mask"), hash));
    }
    check(&results, LAYER_GOLDEN);
    Ok(())
}

#[test]
fn floating_paste_previews_are_identical() -> io::Result<()> {
    let mut results = Vec::new();
    let editor = editor()?;
    for (name, image) in floating_images() {
        let center = (
            f64::from(image.origin.0) + f64::from(image.size.0) * 0.5,
            f64::from(image.origin.1) + f64::from(image.size.1) * 0.5,
        );
        for method in Resampling::ALL {
            let mut hash = Hash::EMPTY;
            for transform in matrix(center).chain(sweep(center)) {
                hash.pixels(&editor.preview_floating(&image, transform, method)?);
            }
            results.push((format!("{name}/{method:?}"), hash));
        }
    }
    check(&results, FLOATING_GOLDEN);
    Ok(())
}

#[test]
fn committed_transforms_and_their_undo_are_identical() -> io::Result<()> {
    let mut results = Vec::new();
    let transform = Affine {
        center: (18.0, 14.0),
        shift: (4.25, -3.0),
        scale: (1.3, 0.8),
        degrees: 33.0,
    };
    for method in Resampling::ALL {
        let mut hash = Hash::EMPTY;
        for case in 0..4 {
            let mut editor = editor()?;
            editor.selection = Some(selections()?.swap_remove(0).1);
            let before = editor.document.layer_pixels(0).expect("layer").to_vec();
            match case {
                0 => editor.apply_transform(transform, method, false, false)?,
                1 => editor.apply_transform(transform, method, true, false)?,
                2 => editor.apply_transform(transform, method, true, true)?,
                _ => {
                    let (_, image) = floating_images().into_iter().next().unwrap();
                    editor.commit_floating(&image, transform, method)?;
                }
            }
            hash.pixels(editor.document.layer_pixels(0).expect("layer"));
            hash.selection(editor.selection.as_ref());
            hash.bytes((editor.last_undo_entry_bytes().expect("entry") as u64).to_le_bytes());
            assert!(editor.undo()?);
            assert_eq!(editor.document.layer_pixels(0).expect("layer"), before);
            assert!(editor.redo()?);
            hash.pixels(editor.document.layer_pixels(0).expect("layer"));
        }
        results.push((format!("commit/{method:?}"), hash));
    }
    check(&results, COMMIT_GOLDEN);
    Ok(())
}

#[test]
fn resized_documents_are_identical() -> io::Result<()> {
    let mut results = Vec::new();
    let mut document = Document::new(13, 9)?;
    let mut pixels = texture(13, 9, 3);
    document.swap_layer_pixels(0, &mut pixels)?;
    let upper = document.add_layer("Upper")?;
    let mut pixels = texture(13, 9, 99);
    document.swap_layer_pixels(upper, &mut pixels)?;
    for method in Resampling::ALL {
        let mut hash = Hash::EMPTY;
        for size in [
            (1, 1),
            (5, 20),
            (40, 3),
            (13, 9),
            (26, 18),
            (97, 61),
            (7, 4),
        ] {
            let resized = geometry::resize(&document, size, method)?;
            for layer in 0..resized.layer_count() {
                hash.pixels(resized.layer_pixels(layer).expect("layer"));
            }
        }
        results.push((format!("resize/{method:?}"), hash));
    }
    check(&results, RESIZE_GOLDEN);
    Ok(())
}

const LAYER_GOLDEN: &[(&str, u64)] = &[
    ("layer/Nearest", 0x99137001f81bee1e),
    ("layer/Antialiased", 0x613ad2b02cca8729),
    ("layer/Bilinear", 0x534f3e333531addf),
    ("ellipse/Nearest/clone=false", 0x586052d53919499b),
    ("ellipse/Nearest/clone=true", 0x4209ef3b40482dec),
    ("ellipse/Antialiased/clone=false", 0xf87a8b7d59108a83),
    ("ellipse/Antialiased/clone=true", 0x4fcfd852024a7f35),
    ("ellipse/Bilinear/clone=false", 0xf69b676bd7aa9ee7),
    ("ellipse/Bilinear/clone=true", 0x251f50b99c714b22),
    ("ellipse/mask", 0x30b4c17d97761ebe),
    ("inverted/Nearest/clone=false", 0xf61c2b13b841207b),
    ("inverted/Nearest/clone=true", 0x8cb39578df486313),
    ("inverted/Antialiased/clone=false", 0xaac87f42555eb59a),
    ("inverted/Antialiased/clone=true", 0x81752def1b9f3530),
    ("inverted/Bilinear/clone=false", 0x8f218adc7b233f0c),
    ("inverted/Bilinear/clone=true", 0x648681f0f7c264a2),
    ("inverted/mask", 0xba1e04ca0b1010a1),
    ("corner/Nearest/clone=false", 0xe31380bd4b5ca3b2),
    ("corner/Nearest/clone=true", 0x18385e9ccd8de89a),
    ("corner/Antialiased/clone=false", 0xdd6dabf3795b5585),
    ("corner/Antialiased/clone=true", 0x0398701ceb168724),
    ("corner/Bilinear/clone=false", 0xc0fb78fbd8b8dfb4),
    ("corner/Bilinear/clone=true", 0x0ca84f82c7e27a7c),
    ("corner/mask", 0xf0629b2ebf93bfed),
    ("feathered/Nearest/clone=false", 0x8f2fa4c839c273d1),
    ("feathered/Nearest/clone=true", 0xa01490dd5ae8268c),
    ("feathered/Antialiased/clone=false", 0x943f3f7d2d0ddc9f),
    ("feathered/Antialiased/clone=true", 0x899d1ae788c67372),
    ("feathered/Bilinear/clone=false", 0x18a2ce048d04989e),
    ("feathered/Bilinear/clone=true", 0x7c4400f4159c0b3c),
    ("feathered/mask", 0x7bfae76d313201e1),
    ("apart/Nearest/clone=false", 0xb72c1abc8baaee36),
    ("apart/Nearest/clone=true", 0x90e192c9d498e838),
    ("apart/Antialiased/clone=false", 0x8002f2437a9e714b),
    ("apart/Antialiased/clone=true", 0x323d4badbe1ec044),
    ("apart/Bilinear/clone=false", 0x0629f2b43ccfb454),
    ("apart/Bilinear/clone=true", 0xd2a0908a243875a6),
    ("apart/mask", 0x96c3a119f9f7a296),
];

const FLOATING_GOLDEN: &[(&str, u64)] = &[
    ("small/Nearest", 0x2eb4bc259e3a8488),
    ("small/Antialiased", 0xb33e20c299797850),
    ("small/Bilinear", 0x7e4fcc5a9ec7ad42),
    ("pixel/Nearest", 0x3179b9bdc2bfefa0),
    ("pixel/Antialiased", 0xb8b22569a4691209),
    ("pixel/Bilinear", 0x77e0ac43771ac337),
    ("clipped/Nearest", 0x7bd7c2641b187dc5),
    ("clipped/Antialiased", 0xafc46b93a6472802),
    ("clipped/Bilinear", 0x48587ef662e958ec),
    ("oversized/Nearest", 0x399abd53789b40b1),
    ("oversized/Antialiased", 0x933de38ca82e95bc),
    ("oversized/Bilinear", 0x59caf680eeeffeff),
];

const COMMIT_GOLDEN: &[(&str, u64)] = &[
    ("commit/Nearest", 0x0f35021f8722b3a7),
    ("commit/Antialiased", 0x4473a5b5466c7313),
    ("commit/Bilinear", 0xfe868c944c3f367b),
];

const RESIZE_GOLDEN: &[(&str, u64)] = &[
    ("resize/Nearest", 0x7030b931172ffdbe),
    ("resize/Antialiased", 0x7a5d5dbfe3f75d17),
    ("resize/Bilinear", 0x88478d5bfc1573bb),
];
