use std::{
    fs::File,
    io::{self, BufReader, Cursor},
    path::Path,
};

use openpaintr::{BLEND_MODES, Document, Pixel, PngCompression, PngDepth};

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pdn")
        .join(name)
}

fn diff_stats(actual: &[Pixel], expected: &[Pixel]) -> (usize, u8) {
    actual
        .iter()
        .zip(expected)
        .filter(|(a, e)| a != e)
        .fold((0_usize, 0_u8), |(count, max_diff), (&a, &e)| {
            (count + 1, max_diff.max(channel_diff(a, e)))
        })
}

fn assert_layers_equal(actual: &Document, expected: &Document, check_pixels: bool, label: &str) {
    assert_eq!(actual.dimensions(), expected.dimensions(), "{label}");
    assert_eq!(actual.layer_count(), expected.layer_count(), "{label}");
    for idx in 0..expected.layer_count() {
        assert_eq!(actual.layer_name(idx), expected.layer_name(idx), "{label}#{idx}");
        assert_eq!(
            actual.layer_properties(idx),
            expected.layer_properties(idx),
            "{label}#{idx}"
        );
        assert_eq!(
            actual.layer_blend_mode(idx),
            expected.layer_blend_mode(idx),
            "{label}#{idx}"
        );
        if check_pixels {
            assert_eq!(
                actual.layer_pixels(idx),
                expected.layer_pixels(idx),
                "{label}#{idx}"
            );
        }
    }
}

fn build_pdn52_blend_mosaic() -> io::Result<Document> {
    let cols = BLEND_MODES.len() as u32;
    let col_w = 20_u32;
    let width = cols * col_w;
    let height = 120_u32;
    let mut doc = Document::new(width, height)?;
    doc.set_layer_name(0, "Backdrop")?;

    // Layer 0: opaque (0..40), semitransparent (40..80), checker transparent/alpha (80..120)
    for y in 0..height {
        for x in 0..width {
            let r = ((x * 13 + y * 5) % 256) as u8;
            let g = ((x * 7 + y * 11 + 80) % 256) as u8;
            let b = (255 - (x * 3 + y * 9) % 256) as u8;
            let px = if y < 40 {
                Pixel([r, g, b, 255])
            } else if y < 80 {
                let a = (32 + (x * 17 + y * 13) % 193) as u8;
                Pixel([r, g, b, a])
            } else if ((x / 5) + (y / 5)).is_multiple_of(2) {
                Pixel([0, 0, 0, 0])
            } else {
                Pixel([r, g, b, 160])
            };
            doc.set_pixel(0, x, y, px)?;
        }
    }

    // Layer 1: hidden trap across entire canvas to catch accidental hidden-layer compositing
    let hidden = doc.add_layer("HiddenTrap")?;
    doc.set_layer_blend_mode(hidden, 8)?; // Difference
    doc.set_layer_properties(hidden, false, 255)?;
    doc.fill_layer(hidden, Pixel([255, 0, 255, 255]))?;

    // Layers 2..16: one column per blend mode at opacity 255 with varying pixel alpha
    const ALPHAS: [u8; 5] = [255, 210, 128, 45, 1];
    for (mode_idx, mode_name) in BLEND_MODES.iter().enumerate() {
        let layer = doc.add_layer(format!("Mode-{mode_idx:02}-{mode_name}"))?;
        doc.set_layer_blend_mode(layer, mode_idx as u8)?;
        doc.set_layer_properties(layer, true, 255)?;
        let x0 = mode_idx as u32 * col_w;
        for y in 0..height {
            for lx in 0..col_w {
                let x = x0 + lx;
                let (r, g, b) = if y == 0 && lx < 4 {
                    match lx {
                        0 => (0, 0, 0),
                        1 => (255, 255, 255),
                        2 => (255, 0, 64),
                        _ => (128, 64, 192),
                    }
                } else {
                    (
                        ((lx * 29 + y * 3 + mode_idx as u32 * 11) % 256) as u8,
                        ((lx * 19 + y * 17 + 40) % 256) as u8,
                        (255 - (lx * 11 + y * 7) % 256) as u8,
                    )
                };
                let a = ALPHAS[((lx + y) as usize) % ALPHAS.len()];
                doc.set_pixel(layer, x, y, Pixel([r, g, b, a]))?;
            }
        }
    }

    // Layers 16..30: 3-layer overlapping stack on lower half (y = 60..120) with layer opacity 149
    for (mode_idx, mode_name) in BLEND_MODES.iter().enumerate() {
        let layer = doc.add_layer(format!("Stack-{mode_idx:02}-{mode_name}"))?;
        doc.set_layer_blend_mode(layer, mode_idx as u8)?;
        doc.set_layer_properties(layer, true, 149)?;
        let x0 = mode_idx as u32 * col_w;
        for y in 60..height {
            for lx in 0..col_w {
                let x = x0 + lx;
                let r = ((lx * 23 + y * 13 + 90) % 256) as u8;
                let g = (255 - (lx * 13 + y * 5) % 256) as u8;
                let b = ((lx * 31 + y * 7 + mode_idx as u32 * 7) % 256) as u8;
                let a = ALPHAS[((lx * 2 + y + 1) as usize) % ALPHAS.len()];
                doc.set_pixel(layer, x, y, Pixel([r, g, b, a]))?;
            }
        }
    }

    Ok(doc)
}

fn build_pdn52_roundtrip_doc() -> io::Result<Document> {
    let width = 96_u32;
    let height = 64_u32;
    let mut png_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_pixel_dims(Some(png::PixelDimensions {
            xppu: 5669,
            yppu: 5669,
            unit: png::Unit::Meter,
        }));
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&vec![0u8; (width * height * 4) as usize])?;
    }
    let mut doc = Document::read_png(Cursor::new(png_bytes))?;
    doc.set_layer_name(0, "Fundo solido")?;
    for y in 0..height {
        for x in 0..width {
            doc.set_pixel(
                0,
                x,
                y,
                Pixel([
                    (20 + (x * 2) % 220) as u8,
                    (40 + (y * 3) % 200) as u8,
                    160,
                    255,
                ]),
            )?;
        }
    }

    let shade = doc.add_layer("Sombra 50")?;
    doc.set_layer_blend_mode(shade, 1)?; // Multiply
    doc.set_layer_properties(shade, true, 128)?;
    for y in 10..50 {
        for x in 12..72 {
            doc.set_pixel(shade, x, y, Pixel([40, 80, 180, 210]))?;
        }
    }

    let hidden = doc.add_layer("Camada oculta")?;
    doc.set_layer_blend_mode(hidden, 0)?;
    doc.set_layer_properties(hidden, false, 255)?;
    doc.fill_layer(hidden, Pixel([0, 220, 220, 255]))?;

    let glow = doc.add_layer("Brilho aditivo")?;
    doc.set_layer_blend_mode(glow, 2)?; // Additive
    doc.set_layer_properties(glow, true, 200)?;
    for y in 20..58 {
        for x in 28..88 {
            doc.set_pixel(glow, x, y, Pixel([220, 140, 40, 180]))?;
        }
    }

    Ok(doc)
}

#[test]
#[ignore]
fn generate_pdn52_stage_inputs() -> io::Result<()> {
    let mosaic = build_pdn52_blend_mosaic()?;
    mosaic.write_pdn(File::create(fixture("pdn52_blend_mosaic.pdn"))?)?;
    let roundtrip = build_pdn52_roundtrip_doc()?;
    roundtrip.write_pdn(File::create(fixture("pdn52_roundtrip_layers.pdn"))?)?;
    Ok(())
}

#[test]
fn old_pdn_3510_matches_reference_png_pixel_for_pixel() -> io::Result<()> {
    let pdn = Document::read_pdn(File::open(fixture("oldPDN3510.pdn"))?)?;
    let reference = Document::read_png(BufReader::new(File::open(fixture(
        "oldPDN3510-reference.png",
    ))?))?;
    assert_eq!(pdn.dimensions(), reference.dimensions());
    let actual = pdn.composite()?;
    let expected = reference.composite()?;
    let (diffs, _) = diff_stats(&actual, &expected);
    assert_eq!(diffs, 0, "compositing mismatch in oldPDN3510.pdn");
    Ok(())
}

#[test]
fn flatten_blend_and_versioned_pdn_fixtures_round_trip_cleanly() -> io::Result<()> {
    for (name, expected_layers) in [
        ("Untitled.pdn", 1usize),
        ("Untitled3.pdn", 2),
        ("oldPDN3510.pdn", 2),
        ("FlattenBlendTest.pdn", 14),
    ] {
        let doc = Document::read_pdn(File::open(fixture(name))?)?;
        assert_eq!(doc.layer_count(), expected_layers, "{name}");
        let orig_comp = doc.composite()?;

        let mut encoded = Vec::new();
        doc.write_pdn(&mut encoded)?;
        let reopened = Document::read_pdn(encoded.as_slice())?;
        assert_layers_equal(&reopened, &doc, false, name);
        assert_eq!(reopened.composite()?, orig_comp, "{name}");
    }
    Ok(())
}

#[test]
fn png_export_preserves_alpha_and_depth_options_separately() -> io::Result<()> {
    let mut doc = Document::new(5, 1)?;
    doc.fill_layer(0, Pixel([20, 40, 60, 200]))?;
    doc.set_pixel(0, 0, 0, Pixel([0, 0, 0, 0]))?;
    doc.set_pixel(0, 1, 0, Pixel([10, 20, 30, 1]))?;
    doc.set_pixel(0, 2, 0, Pixel([100, 150, 200, 128]))?;
    doc.set_pixel(0, 3, 0, Pixel([200, 100, 50, 254]))?;
    doc.set_pixel(0, 4, 0, Pixel([255, 255, 255, 255]))?;

    let hidden = doc.add_layer("Oculta")?;
    doc.set_layer_properties(hidden, false, 255)?;
    doc.fill_layer(hidden, Pixel([255, 0, 255, 255]))?;

    let expected_rgba = doc.composite()?;
    for compression in PngCompression::ALL {
        let mut rgba_bytes = Vec::new();
        doc.write_png_with_options(&mut rgba_bytes, PngDepth::Rgba32, compression)?;
        let decoded = Document::read_png(Cursor::new(rgba_bytes))?;
        assert_eq!(decoded.composite()?, expected_rgba, "{compression:?}");
    }

    for (depth, compression) in [
        (PngDepth::Rgb24White, PngCompression::Balanced),
        (PngDepth::Rgb24Black, PngCompression::High),
    ] {
        let mut bytes = Vec::new();
        doc.write_png_with_options(&mut bytes, depth, compression)?;
        let decoded = Document::read_png(Cursor::new(bytes))?;
        assert_eq!(
            decoded.composite()?,
            expected_rgba
                .iter()
                .map(|&px| depth.flatten_pixel(px))
                .collect::<Vec<_>>(),
            "{depth:?}"
        );
    }
    Ok(())
}

fn channel_diff(a: Pixel, b: Pixel) -> u8 {
    (0..4)
        .map(|i| a.0[i].abs_diff(b.0[i]))
        .max()
        .unwrap_or(0)
}

fn premul_rgb_diff(a: Pixel, b: Pixel) -> u8 {
    let pm = |px: Pixel, ch: usize| -> u8 {
        ((u16::from(px.0[ch]) * u16::from(px.0[3]) + 127) / 255) as u8
    };
    (0..3)
        .map(|i| pm(a, i).abs_diff(pm(b, i)))
        .max()
        .unwrap_or(0)
}

#[test]
fn pdn52_blend_mosaic_matches_paintnet_52_reference() -> io::Result<()> {
    let pdn = Document::read_pdn(File::open(fixture("pdn52_blend_mosaic.pdn"))?)?;
    let reference = Document::read_png(BufReader::new(File::open(fixture(
        "pdn52_blend_mosaic-pdn52.png",
    ))?))?;
    assert_eq!(pdn.dimensions(), (280, 120));
    assert_eq!(pdn.dimensions(), reference.dimensions());

    let actual = pdn.composite()?;
    let expected = reference.composite()?;
    let width = pdn.dimensions().0 as usize;
    let height = pdn.dimensions().1 as usize;
    let col_w = 20_usize;

    const MAX_3LAYER_PREMUL: [u8; 14] = [
        2,  // Normal
        3,  // Multiply
        2,  // Additive
        5,  // Color Burn
        4,  // Color Dodge
        5,  // Reflect
        15, // Glow: 1/(255-B) amplifies +-1 intermediate rounding near 255
        3,  // Overlay
        3,  // Difference
        3,  // Negation
        2,  // Lighten
        3,  // Darken
        3,  // Screen
        98, // Xor: bitwise carry flip on +-1 intermediate byte
    ];

    let mut overall_alpha_max = 0_u8;
    let mut overall_two_layer_max = 0_u8;
    for (mode_idx, mode_name) in BLEND_MODES.iter().enumerate() {
        let mut mode_diffs = 0_usize;
        let mut alpha_max = 0_u8;
        let mut two_layer_max = 0_u8;
        let mut three_layer_premul_max = 0_u8;
        let x0 = mode_idx * col_w;
        for y in 0..height {
            for x in x0..(x0 + col_w) {
                let idx = y * width + x;
                let a = actual[idx];
                let e = expected[idx];
                if a != e {
                    mode_diffs += 1;
                    alpha_max = alpha_max.max(a.0[3].abs_diff(e.0[3]));
                    if y < 60 {
                        two_layer_max = two_layer_max.max(channel_diff(a, e));
                    } else {
                        three_layer_premul_max = three_layer_premul_max.max(premul_rgb_diff(a, e));
                    }
                }
            }
        }
        println!(
            "PDN 5.2 mosaic mode {mode_idx:02} ({mode_name:<11}): diff_px={mode_diffs:4}/{}, alpha_max={alpha_max}, two_layer_max={two_layer_max}, three_layer_premul_max={three_layer_premul_max}",
            col_w * height
        );
        assert!(
            alpha_max <= 1,
            "{mode_name}: alpha divergence {alpha_max} > 1"
        );
        assert!(
            two_layer_max <= 2,
            "{mode_name}: 2-layer divergence {two_layer_max} > 2"
        );
        assert!(
            three_layer_premul_max <= MAX_3LAYER_PREMUL[mode_idx],
            "{mode_name}: 3-layer premul divergence {three_layer_premul_max} > {}",
            MAX_3LAYER_PREMUL[mode_idx]
        );
        overall_alpha_max = overall_alpha_max.max(alpha_max);
        overall_two_layer_max = overall_two_layer_max.max(two_layer_max);
    }
    println!(
        "PDN 5.2 mosaic summary: 33600 px verified, overall_alpha_max={overall_alpha_max}, overall_two_layer_max={overall_two_layer_max}"
    );
    Ok(())
}

#[test]
fn pdn52_flatten_blend_test_matches_paintnet_52_reference() -> io::Result<()> {
    let pdn = Document::read_pdn(File::open(fixture("FlattenBlendTest.pdn"))?)?;
    let reference = Document::read_png(BufReader::new(File::open(fixture(
        "FlattenBlendTest-pdn52.png",
    ))?))?;
    assert_eq!(pdn.dimensions(), reference.dimensions());

    let actual = pdn.composite()?;
    let expected = reference.composite()?;
    let (diff_px, max_lsb) = diff_stats(&actual, &expected);
    println!(
        "FlattenBlendTest vs Paint.NET 5.2: diff_px={diff_px}/{}, max_lsb_diff={max_lsb}",
        actual.len()
    );
    assert!(
        max_lsb <= 3,
        "unexpected FlattenBlendTest divergence vs Paint.NET 5.2: max_lsb_diff={max_lsb}"
    );
    Ok(())
}

#[test]
fn pdn52_resaved_roundtrip_preserves_layers_metadata_and_pixels() -> io::Result<()> {
    let orig = Document::read_pdn(File::open(fixture("pdn52_roundtrip_layers.pdn"))?)?;
    let resaved = Document::read_pdn(File::open(fixture("pdn52_roundtrip_resaved.pdn"))?)?;
    let ref_png = Document::read_png(BufReader::new(File::open(fixture(
        "pdn52_roundtrip_layers-pdn52.png",
    ))?))?;

    assert_eq!(resaved.dimensions(), (96, 64));
    let (dx, dy) = resaved.resolution_dpi();
    let (ox, oy) = orig.resolution_dpi();
    println!("PDN 5.2 roundtrip DPI: orig=({ox:.2}, {oy:.2}), resaved=({dx:.2}, {dy:.2})");
    assert!((dx - ox).abs() < 0.5 && (dy - oy).abs() < 0.5);

    assert_eq!(resaved.layer_count(), 4);
    assert_layers_equal(&resaved, &orig, true, "layer");

    let orig_comp = orig.composite()?;
    let resaved_comp = resaved.composite()?;
    assert_eq!(resaved_comp, orig_comp);

    let ref_comp = ref_png.composite()?;
    assert_eq!(orig_comp.len(), ref_comp.len());
    let (diff_px, max_lsb) = diff_stats(&orig_comp, &ref_comp);
    println!(
        "Roundtrip composite vs Paint.NET 5.2 PNG: diff_px={diff_px}/{}, max_lsb_diff={max_lsb}",
        orig_comp.len()
    );
    assert!(max_lsb <= 3);
    Ok(())
}
