//! Golden tests that pin, byte for byte, everything built on the pixel buffers
//! that architecture plan item 2.1 rewrites: compositing, merging, conversions,
//! PNG import and export, and thumbnails.
//!
//! The hashes were recorded from the implementation before that change. A
//! mismatch means the output changed; the failure prints the actual table.

use std::{
    fs::File,
    io::{self, BufReader, Cursor},
    path::{Path, PathBuf},
};

use openpaintr::{Document, LayerData, Pixel, PngCompression, PngDepth};
use png::{BitDepth, ColorType, Encoder};

const FIXTURES: [&str; 7] = [
    "FlattenBlendTest.pdn",
    "Untitled.pdn",
    "Untitled3.pdn",
    "oldPDN3510.pdn",
    "pdn52_blend_mosaic.pdn",
    "pdn52_roundtrip_layers.pdn",
    "pdn52_roundtrip_resaved.pdn",
];

/// 64-bit FNV-1a.
struct Hash(u64);

impl Hash {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, bytes: &[u8]) -> &mut Self {
        // The length keeps adjacent fields apart.
        for &byte in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
        }
        self
    }

    fn pixels(&mut self, pixels: &[Pixel]) -> &mut Self {
        let bytes: Vec<u8> = pixels.iter().flat_map(|pixel| pixel.0).collect();
        self.bytes(&bytes)
    }

    fn layer(&mut self, layer: &LayerData) -> &mut Self {
        self.bytes(layer.name.as_bytes())
            .bytes(&[u8::from(layer.visible), layer.opacity, layer.blend])
            .pixels(&layer.pixels)
    }
}

fn of(pixels: &[Pixel]) -> u64 {
    Hash::new().pixels(pixels).0
}

/// Every layer's name, properties, and pixels.
fn layers_hash(document: &Document) -> io::Result<u64> {
    let mut hash = Hash::new();
    for index in 0..document.layer_count() {
        hash.layer(&document.clone_layer_data(index)?);
    }
    Ok(hash.0)
}

#[derive(Default)]
struct Golden(Vec<(String, u64)>);

impl Golden {
    fn record(&mut self, name: impl Into<String>, hash: u64) {
        self.0.push((name.into(), hash));
    }

    #[track_caller]
    fn check(&self, expected: &[(&str, u64)]) {
        let matches = self.0.len() == expected.len()
            && self
                .0
                .iter()
                .zip(expected)
                .all(|((name, hash), (key, value))| name == key && hash == value);
        if !matches {
            let table: String = self
                .0
                .iter()
                .map(|(name, hash)| format!("    (\"{name}\", {hash:#018x}),\n"))
                .collect();
            let changed: Vec<_> = self
                .0
                .iter()
                .filter(|(name, hash)| !expected.contains(&(name.as_str(), *hash)))
                .map(|(name, _)| name.as_str())
                .collect();
            panic!("output changed: {changed:?}\nactual:\n{table}");
        }
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pdn")
        .join(name)
}

fn open(name: &str) -> io::Result<Document> {
    Document::read_pdn(File::open(fixture(name))?)
}

/// Rectangles that exercise row offsets, odd widths, and the canvas edges.
fn regions(width: u32, height: u32) -> [((u32, u32), (u32, u32)); 6] {
    let (right, bottom) = (width - 1, height - 1);
    [
        ((0, 0), (0, 0)),
        ((right, bottom), (right, bottom)),
        ((0, height / 2), (right, height / 2)),
        ((width / 2, 0), (width / 2, bottom)),
        (
            (width / 3, height / 3),
            (right - width / 5, bottom - height / 7),
        ),
        ((1.min(right), 1.min(bottom)), (right, bottom)),
    ]
}

fn composites(golden: &mut Golden, label: &str, document: &Document) -> io::Result<()> {
    let (width, height) = document.dimensions();
    let full = document.composite()?;
    golden.record(format!("{label}/composite"), of(&full));
    let mut hash = Hash::new();
    for (min, max) in regions(width, height) {
        let part = document.composite_rect(min, max)?;
        // A region is exactly that part of the full composite.
        let expected: Vec<Pixel> = (min.1..=max.1)
            .flat_map(|y| {
                let row = y as usize * width as usize;
                full[row + min.0 as usize..=row + max.0 as usize]
                    .iter()
                    .copied()
            })
            .collect();
        assert_eq!(part, expected, "{label} {min:?} {max:?}");
        assert_eq!(document.composite_pixel(min.0, min.1)?, part[0]);
        hash.pixels(&part);
    }
    golden.record(format!("{label}/regions"), hash.0);
    Ok(())
}

/// Merges layers from the top down, hashing the data that undo keeps and the
/// resulting layers. `prepare` adjusts the two layers about to be merged.
fn merges(
    golden: &mut Golden,
    label: &str,
    mut document: Document,
    prepare: impl Fn(&mut Document, usize, usize) -> io::Result<()>,
) -> io::Result<()> {
    let mut hash = Hash::new();
    let mut step = 0;
    while document.layer_count() > 1 {
        let top = document.layer_count() - 1;
        prepare(&mut document, top, step)?;
        let (removed, bottom_before) = document.merge_layer_down(top)?;
        hash.layer(&removed).layer(&bottom_before);
        hash.layer(&document.clone_layer_data(top - 1)?);
        step += 1;
    }
    golden.record(label.to_owned(), hash.0);
    Ok(())
}

fn merge_variants(
    golden: &mut Golden,
    label: &str,
    build: impl Fn() -> io::Result<Document>,
) -> io::Result<()> {
    merges(
        golden,
        &format!("{label}/merge"),
        build()?,
        |_, _, _| Ok(()),
    )?;
    // Every state of the lower layer, and a top layer that contributes nothing.
    merges(
        golden,
        &format!("{label}/merge-variants"),
        build()?,
        |document, top, step| {
            document.set_layer_properties(
                top,
                step % 5 != 4,
                if step % 7 == 6 { 0 } else { 201 },
            )?;
            let (visible, opacity) = match step % 3 {
                0 => (true, 255),
                1 => (true, 150),
                _ => (false, 90),
            };
            document.set_layer_properties(top - 1, visible, opacity)
        },
    )
}

#[test]
fn pdn_fixtures_composite_merge_and_export_identically() -> io::Result<()> {
    let mut golden = Golden::default();
    for name in FIXTURES {
        let mut document = open(name)?;
        golden.record(format!("{name}/layers"), layers_hash(&document)?);
        composites(&mut golden, name, &document)?;

        let (width, height) = document.dimensions();
        let mut pam = Vec::new();
        document.write_pam(&mut pam)?;
        golden.record(format!("{name}/pam"), Hash::new().bytes(&pam).0);
        let flattened = document.flattened()?;
        golden.record(format!("{name}/flattened"), layers_hash(&flattened)?);

        let mut hash = Hash::new();
        for side in [1, 24, 160] {
            let (w, h, pixels) = document.composite_thumbnail(side)?;
            hash.bytes(&[w as u8, h as u8]).pixels(&pixels);
            for layer in 0..document.layer_count() {
                let (_, _, pixels) = document.layer_thumbnail(layer, side).expect("layer");
                hash.pixels(&pixels);
            }
        }
        golden.record(format!("{name}/thumbnails"), hash.0);

        let mut hash = Hash::new();
        // Stored blocks: the bytes depend on the pixels, not on the compressor.
        for depth in PngDepth::ALL {
            let mut png = Vec::new();
            document.write_png_with_options(&mut png, depth, PngCompression::None)?;
            hash.bytes(&png);
        }
        golden.record(format!("{name}/png-export"), hash.0);

        let mut pdn = Vec::new();
        document.write_pdn(&mut pdn)?;
        golden.record(format!("{name}/pdn-export"), Hash::new().bytes(&pdn).0);

        // Every layer visible and translucent.
        for layer in 0..document.layer_count() {
            document.set_layer_properties(layer, true, 137)?;
        }
        composites(&mut golden, &format!("{name}/translucent"), &document)?;
        assert_eq!(document.dimensions(), (width, height));

        merge_variants(&mut golden, name, || open(name))?;
    }
    golden.check(PDN_GOLDEN);
    Ok(())
}

/// Deterministic pseudo-random pixels, with many fully opaque and clear ones.
fn noise(seed: u32) -> impl FnMut() -> Pixel {
    let mut state = seed;
    move || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let mut bytes = state.rotate_left(13).to_le_bytes();
        match state >> 29 {
            0 => bytes[3] = 0,
            1 | 2 => bytes[3] = 255,
            _ => {}
        }
        Pixel(bytes)
    }
}

/// One layer per blend mode over a noisy background.
fn synthetic(width: u32, height: u32) -> io::Result<Document> {
    let mut document = Document::new(width, height)?;
    let mut next = noise(width * 31 + height);
    for blend in 0..14_u8 {
        if blend > 0 {
            let layer = document.add_layer(format!("Layer {blend}"))?;
            document.set_layer_blend_mode(layer, blend)?;
            document.set_layer_properties(layer, blend != 5, 255 - blend * 11)?;
        }
        let mut pixels: Vec<Pixel> = (0..width * height).map(|_| next()).collect();
        document.swap_layer_pixels(usize::from(blend), &mut pixels)?;
    }
    Ok(document)
}

#[test]
fn every_blend_mode_composites_and_merges_identically_at_every_width() -> io::Result<()> {
    let mut golden = Golden::default();
    // Widths around the SIMD vector sizes exercise the scalar tails.
    for width in (1..=17).chain([31, 33, 64, 65]) {
        let label = format!("w{width}");
        composites(&mut golden, &label, &synthetic(width, 7)?)?;
        merge_variants(&mut golden, &label, || synthetic(width, 7))?;
    }
    golden.check(SYNTHETIC_GOLDEN);
    Ok(())
}

#[test]
fn hidden_or_clear_layers_composite_to_transparency() -> io::Result<()> {
    let mut document = synthetic(9, 5)?;
    for layer in 0..document.layer_count() {
        document.set_layer_properties(layer, layer % 2 == 0, 0)?;
    }
    assert_eq!(document.composite()?, vec![Pixel::default(); 45]);
    for (min, max) in [((2, 0), (1, 0)), ((0, 0), (9, 0)), ((0, 5), (0, 5))] {
        let error = document.composite_rect(min, max).expect_err("accepted");
        assert_eq!(error.to_string(), "Invalid compositing region.");
    }
    Ok(())
}

#[test]
fn rgba_byte_conversions_keep_channel_order_and_ignore_a_partial_pixel() -> io::Result<()> {
    let bytes: Vec<u8> = (0..=u8::MAX).collect();
    let pixels = Pixel::from_rgba_bytes(&bytes)?;
    assert_eq!(pixels.len(), 64);
    assert_eq!(pixels[1], Pixel([4, 5, 6, 7]));
    assert_eq!(Pixel::to_rgba_bytes(&pixels)?, bytes);
    // Any alignment, and trailing bytes that don't make a pixel are dropped.
    for start in 0..4 {
        for end in [start, start + 3, start + 4, start + 7, 255] {
            let pixels = Pixel::from_rgba_bytes(&bytes[start..end])?;
            assert_eq!(pixels.len(), (end - start) / 4, "{start}..{end}");
            assert_eq!(
                Pixel::to_rgba_bytes(&pixels)?,
                bytes[start..start + pixels.len() * 4]
            );
        }
    }
    assert!(Pixel::to_rgba_bytes(&[])?.is_empty());
    Ok(())
}

/// A PNG of the given format filled with deterministic samples.
fn png_input(color: ColorType, depth: BitDepth, transparency: bool) -> io::Result<Vec<u8>> {
    let (width, height) = (13_u32, 5_u32);
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes, width, height);
    encoder.set_color(color);
    encoder.set_depth(depth);
    encoder.set_pixel_dims(Some(png::PixelDimensions {
        xppu: 3780,
        yppu: 2835,
        unit: png::Unit::Meter,
    }));
    if color == ColorType::Indexed {
        encoder.set_palette(
            (0..=u8::MAX)
                .flat_map(|i| [i, !i, i ^ 0x5a])
                .collect::<Vec<_>>(),
        );
    }
    if transparency {
        let value = match color {
            ColorType::Indexed => vec![0, 40, 80, 255, 7],
            ColorType::Grayscale => vec![0, 0],
            _ => vec![0; 6],
        };
        encoder.set_trns(value);
    }
    let bits = u32::from(color.samples() as u8) * u32::from(depth as u8);
    let row = (width * bits).div_ceil(8);
    let data: Vec<u8> = (0..row * height)
        .map(|i| {
            if i % 11 == 0 {
                0
            } else {
                (i * 73 + i / 7) as u8
            }
        })
        .collect();
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&data)?;
    writer.finish()?;
    Ok(bytes)
}

/// The imported pixels and everything else the new document holds.
fn imported(golden: &mut Golden, label: &str, document: &Document) -> io::Result<()> {
    assert_eq!(document.layer_count(), 1);
    assert_eq!(document.layer_name(0), Some("Background"));
    assert_eq!(document.layer_properties(0), Some((true, 255)));
    assert_eq!(document.layer_blend_id(0), Some(0));
    let (width, height) = document.dimensions();
    let (x, y) = document.resolution_dpi();
    let mut pdn = Vec::new();
    document.write_pdn(&mut pdn)?;
    let mut hash = Hash::new();
    hash.bytes(&width.to_le_bytes())
        .bytes(&height.to_le_bytes())
        .bytes(&x.to_le_bytes())
        .bytes(&y.to_le_bytes())
        .pixels(document.layer_pixels(0).expect("layer"));
    golden.record(label.to_owned(), hash.0);
    golden.record(format!("{label}/pdn"), Hash::new().bytes(&pdn).0);
    Ok(())
}

#[test]
fn png_import_produces_identical_documents() -> io::Result<()> {
    use BitDepth::{Eight, One, Sixteen, Two};
    use ColorType::{Grayscale, GrayscaleAlpha, Indexed, Rgb, Rgba};

    let mut golden = Golden::default();
    for (color, depth, transparency) in [
        (Grayscale, One, false),
        (Grayscale, Eight, false),
        (Grayscale, Eight, true),
        (Grayscale, Sixteen, false),
        (GrayscaleAlpha, Eight, false),
        (GrayscaleAlpha, Sixteen, false),
        (Rgb, Eight, false),
        (Rgb, Eight, true),
        (Rgb, Sixteen, false),
        (Rgba, Eight, false),
        (Rgba, Sixteen, false),
        (Indexed, Two, true),
        (Indexed, Eight, false),
        (Indexed, Eight, true),
    ] {
        let bytes = png_input(color, depth, transparency)?;
        let label = format!("{color:?}-{depth:?}-{transparency}");
        imported(
            &mut golden,
            &label,
            &Document::read_png(Cursor::new(bytes))?,
        )?;
    }
    for name in [
        "FlattenBlendTest-pdn52.png",
        "oldPDN3510-reference.png",
        "pdn52_blend_mosaic-pdn52.png",
        "pdn52_roundtrip_layers-pdn52.png",
    ] {
        let file = BufReader::new(File::open(fixture(name))?);
        imported(&mut golden, name, &Document::read_png(file)?)?;
    }
    golden.check(PNG_GOLDEN);
    Ok(())
}

const PDN_GOLDEN: &[(&str, u64)] = &[
    ("FlattenBlendTest.pdn/layers", 0xfd5091e7be036d5d),
    ("FlattenBlendTest.pdn/composite", 0x146abb09fc0060ab),
    ("FlattenBlendTest.pdn/regions", 0xc77d6a133664ddc4),
    ("FlattenBlendTest.pdn/pam", 0xe46cd87961bf3566),
    ("FlattenBlendTest.pdn/flattened", 0x83caba5745b241db),
    ("FlattenBlendTest.pdn/thumbnails", 0xc976eee67bc06bac),
    ("FlattenBlendTest.pdn/png-export", 0x87fe27338f2b20a3),
    ("FlattenBlendTest.pdn/pdn-export", 0xdb0b539e72bbc108),
    (
        "FlattenBlendTest.pdn/translucent/composite",
        0x6b812050a869cdfe,
    ),
    (
        "FlattenBlendTest.pdn/translucent/regions",
        0xf4bb14ddbb853c84,
    ),
    ("FlattenBlendTest.pdn/merge", 0x887e1d162a8744a6),
    ("FlattenBlendTest.pdn/merge-variants", 0x38cb39bc3b0dd6d4),
    ("Untitled.pdn/layers", 0xc700f2d82858469c),
    ("Untitled.pdn/composite", 0x285ac779d5b8c06d),
    ("Untitled.pdn/regions", 0xbcbecf91b7a4ab15),
    ("Untitled.pdn/pam", 0xc74bd7abc27e0a70),
    ("Untitled.pdn/flattened", 0xc31b969fade936dd),
    ("Untitled.pdn/thumbnails", 0x38beb15033c54b7b),
    ("Untitled.pdn/png-export", 0xe4e215ffc9c0aea7),
    ("Untitled.pdn/pdn-export", 0x6371699adbac5005),
    ("Untitled.pdn/translucent/composite", 0x94ff0419b31784fd),
    ("Untitled.pdn/translucent/regions", 0xe83da9360e968be3),
    ("Untitled.pdn/merge", 0xcbf29ce484222325),
    ("Untitled.pdn/merge-variants", 0xcbf29ce484222325),
    ("Untitled3.pdn/layers", 0x85fb5c44a0137573),
    ("Untitled3.pdn/composite", 0x429f249614032334),
    ("Untitled3.pdn/regions", 0x96b9e4fddcebce97),
    ("Untitled3.pdn/pam", 0xf8a86a3c08911c81),
    ("Untitled3.pdn/flattened", 0x7724f5676b0b1d24),
    ("Untitled3.pdn/thumbnails", 0x42d01661ddd2e67f),
    ("Untitled3.pdn/png-export", 0x912c721696594740),
    ("Untitled3.pdn/pdn-export", 0xb1182e4b08b3bae6),
    ("Untitled3.pdn/translucent/composite", 0x48e68e7797476934),
    ("Untitled3.pdn/translucent/regions", 0xdb9f3c1105ba0922),
    ("Untitled3.pdn/merge", 0x57af153134a6ebeb),
    ("Untitled3.pdn/merge-variants", 0xe407d564ecced263),
    ("oldPDN3510.pdn/layers", 0xe7ab14ed1ee2be03),
    ("oldPDN3510.pdn/composite", 0x90ba291154724741),
    ("oldPDN3510.pdn/regions", 0xe3e42a028c2cc171),
    ("oldPDN3510.pdn/pam", 0x528ade619ef8e340),
    ("oldPDN3510.pdn/flattened", 0x128e7ca0f15f7c91),
    ("oldPDN3510.pdn/thumbnails", 0x41bd9914ba1bcc2f),
    ("oldPDN3510.pdn/png-export", 0xf7535953dc04dfbb),
    ("oldPDN3510.pdn/pdn-export", 0xbb3afbb50d899816),
    ("oldPDN3510.pdn/translucent/composite", 0x30bb4f949f139771),
    ("oldPDN3510.pdn/translucent/regions", 0x78693da1356c35ff),
    ("oldPDN3510.pdn/merge", 0x8576bdf74a0f32c6),
    ("oldPDN3510.pdn/merge-variants", 0x84dac8782b3e5dac),
    ("pdn52_blend_mosaic.pdn/layers", 0x73caa80e88c36253),
    ("pdn52_blend_mosaic.pdn/composite", 0xe020e04662e8dc6f),
    ("pdn52_blend_mosaic.pdn/regions", 0xba0ef85e7adeaaf7),
    ("pdn52_blend_mosaic.pdn/pam", 0xbac37eb6352bb293),
    ("pdn52_blend_mosaic.pdn/flattened", 0x94c1e69bbc69509f),
    ("pdn52_blend_mosaic.pdn/thumbnails", 0x10f763de110bf79b),
    ("pdn52_blend_mosaic.pdn/png-export", 0x35e49b8c9ec60df8),
    ("pdn52_blend_mosaic.pdn/pdn-export", 0xd3f88f06f16c0031),
    (
        "pdn52_blend_mosaic.pdn/translucent/composite",
        0x8d0a5cc9bc3227f5,
    ),
    (
        "pdn52_blend_mosaic.pdn/translucent/regions",
        0x01859756d2474567,
    ),
    ("pdn52_blend_mosaic.pdn/merge", 0xe11908997db3d700),
    ("pdn52_blend_mosaic.pdn/merge-variants", 0xf8a0b1999b4e040f),
    ("pdn52_roundtrip_layers.pdn/layers", 0xa07d466a90345a07),
    ("pdn52_roundtrip_layers.pdn/composite", 0x6cf96b2dae5d6a91),
    ("pdn52_roundtrip_layers.pdn/regions", 0x1fc9f732a793e470),
    ("pdn52_roundtrip_layers.pdn/pam", 0x03358ca15dbc5eb9),
    ("pdn52_roundtrip_layers.pdn/flattened", 0x6f9613ac1a0f9221),
    ("pdn52_roundtrip_layers.pdn/thumbnails", 0x5331caeb97e58de5),
    ("pdn52_roundtrip_layers.pdn/png-export", 0x4db319fcea6f92c1),
    ("pdn52_roundtrip_layers.pdn/pdn-export", 0x16564768bee6bcc2),
    (
        "pdn52_roundtrip_layers.pdn/translucent/composite",
        0x835ffe5f8620ba71,
    ),
    (
        "pdn52_roundtrip_layers.pdn/translucent/regions",
        0xdcfdf08a62f169b8,
    ),
    ("pdn52_roundtrip_layers.pdn/merge", 0x302c593a09a23caa),
    (
        "pdn52_roundtrip_layers.pdn/merge-variants",
        0x71359cd2d282a213,
    ),
    ("pdn52_roundtrip_resaved.pdn/layers", 0xa07d466a90345a07),
    ("pdn52_roundtrip_resaved.pdn/composite", 0x6cf96b2dae5d6a91),
    ("pdn52_roundtrip_resaved.pdn/regions", 0x1fc9f732a793e470),
    ("pdn52_roundtrip_resaved.pdn/pam", 0x03358ca15dbc5eb9),
    ("pdn52_roundtrip_resaved.pdn/flattened", 0x6f9613ac1a0f9221),
    ("pdn52_roundtrip_resaved.pdn/thumbnails", 0x5331caeb97e58de5),
    ("pdn52_roundtrip_resaved.pdn/png-export", 0x4db319fcea6f92c1),
    ("pdn52_roundtrip_resaved.pdn/pdn-export", 0xf8d4fd56a385f1fb),
    (
        "pdn52_roundtrip_resaved.pdn/translucent/composite",
        0x835ffe5f8620ba71,
    ),
    (
        "pdn52_roundtrip_resaved.pdn/translucent/regions",
        0xdcfdf08a62f169b8,
    ),
    ("pdn52_roundtrip_resaved.pdn/merge", 0x302c593a09a23caa),
    (
        "pdn52_roundtrip_resaved.pdn/merge-variants",
        0x71359cd2d282a213,
    ),
];

const SYNTHETIC_GOLDEN: &[(&str, u64)] = &[
    ("w1/composite", 0x3faba2627767d86d),
    ("w1/regions", 0x1f0246c27570f474),
    ("w1/merge", 0xfe042aa00763ce0a),
    ("w1/merge-variants", 0xb5018c04003b9510),
    ("w2/composite", 0xfcbc992649539cf9),
    ("w2/regions", 0xc7871121b9239480),
    ("w2/merge", 0x04d8120c0f8f8afb),
    ("w2/merge-variants", 0x997be23eeb133198),
    ("w3/composite", 0x2917c94aaef79f3e),
    ("w3/regions", 0xb97797b68da85af0),
    ("w3/merge", 0xd29df56eae316c34),
    ("w3/merge-variants", 0xcb40fecf5cd21e53),
    ("w4/composite", 0xbf794c0b3b543e65),
    ("w4/regions", 0xdd32a6a9077d6902),
    ("w4/merge", 0x4a75e17e025f4441),
    ("w4/merge-variants", 0x1c0a517ab6fc65ee),
    ("w5/composite", 0x4c5a0fe5388347b2),
    ("w5/regions", 0x77c838eda282cbf0),
    ("w5/merge", 0xef116b4ff8bf5073),
    ("w5/merge-variants", 0x25723f3c1c2de141),
    ("w6/composite", 0x63928e5664434259),
    ("w6/regions", 0xf4a468cd3f74c9c0),
    ("w6/merge", 0x4bb3b15d677ece3f),
    ("w6/merge-variants", 0xad37cb5ae3c8b16b),
    ("w7/composite", 0x03766488bcd829df),
    ("w7/regions", 0xc4a525ad745bcabe),
    ("w7/merge", 0xd49d6933a02431ab),
    ("w7/merge-variants", 0x4c8440734451a239),
    ("w8/composite", 0xf8ed84b71b403b68),
    ("w8/regions", 0x0006dcbb4bfac8f1),
    ("w8/merge", 0x69c87ed5a4245216),
    ("w8/merge-variants", 0x7a318cd9dccb0015),
    ("w9/composite", 0x7bf88a3e34c14a9b),
    ("w9/regions", 0x4d9482d45737825b),
    ("w9/merge", 0x86b02c97f2a5f68a),
    ("w9/merge-variants", 0xf218e719075109f1),
    ("w10/composite", 0x34f1128e51b92106),
    ("w10/regions", 0x7d3458c7659d6227),
    ("w10/merge", 0x29e17da257105c9f),
    ("w10/merge-variants", 0x6f48bbc20e487352),
    ("w11/composite", 0x79388648202ff2f8),
    ("w11/regions", 0xa58ad3b5bb30fefd),
    ("w11/merge", 0xbfc6b4f756b46118),
    ("w11/merge-variants", 0x6e27750cd1aa3c9d),
    ("w12/composite", 0xf8c790e155ad4c04),
    ("w12/regions", 0xf586adfaa334c052),
    ("w12/merge", 0xf3076be387408ee3),
    ("w12/merge-variants", 0x1f0b47a84d22d3c7),
    ("w13/composite", 0x550a620c2e8a0c7e),
    ("w13/regions", 0xac8195680cfcc712),
    ("w13/merge", 0xe49758776235b613),
    ("w13/merge-variants", 0x8ada3392f92907f8),
    ("w14/composite", 0x5f614c437347485f),
    ("w14/regions", 0x0f5109cf47546adb),
    ("w14/merge", 0x82713787a2db6f51),
    ("w14/merge-variants", 0xcd19e177d8ed1a04),
    ("w15/composite", 0x9dce797e78ff5316),
    ("w15/regions", 0x4481d17e7604d58c),
    ("w15/merge", 0xdb28dcdbbf0520c9),
    ("w15/merge-variants", 0x4fc573110d538386),
    ("w16/composite", 0xb074716118830263),
    ("w16/regions", 0x8d26fdf881222066),
    ("w16/merge", 0xe4ab983077e7c3f4),
    ("w16/merge-variants", 0x4e8c9c30fb3be0a8),
    ("w17/composite", 0xa2d55184a93d73ec),
    ("w17/regions", 0x769f5e039cc27128),
    ("w17/merge", 0x6e3ff244f07fd2f1),
    ("w17/merge-variants", 0x4143537abb56f46b),
    ("w31/composite", 0x1a2b9858ff5d549c),
    ("w31/regions", 0x34ea7ef0c0f2244f),
    ("w31/merge", 0x9089fd613039c1eb),
    ("w31/merge-variants", 0xf8113116f369680b),
    ("w33/composite", 0xf0168962ee5caf9c),
    ("w33/regions", 0xa2ab659db11df4de),
    ("w33/merge", 0xfcdd5797158dee32),
    ("w33/merge-variants", 0x702af2bce0d54c62),
    ("w64/composite", 0x0a2bcf6f03a2d72a),
    ("w64/regions", 0x401d97973f38845b),
    ("w64/merge", 0x7a719b20267cebcc),
    ("w64/merge-variants", 0x3e42b16eecdaf0c5),
    ("w65/composite", 0x50301f7303ac6b2a),
    ("w65/regions", 0x4391423036ad952e),
    ("w65/merge", 0x9e4b451ffb188403),
    ("w65/merge-variants", 0x1c05b2f8d2347524),
];

const PNG_GOLDEN: &[(&str, u64)] = &[
    ("Grayscale-One-false", 0x5ba0ee88a206f82a),
    ("Grayscale-One-false/pdn", 0xa03da2c36b389337),
    ("Grayscale-Eight-false", 0xcc38551a2484ae2c),
    ("Grayscale-Eight-false/pdn", 0xf743a6e0e6051e0c),
    ("Grayscale-Eight-true", 0xdf909bef754350e9),
    ("Grayscale-Eight-true/pdn", 0x5339ecdff8bfb921),
    ("Grayscale-Sixteen-false", 0x9d01d5e6d31013be),
    ("Grayscale-Sixteen-false/pdn", 0x1f14282283afa404),
    ("GrayscaleAlpha-Eight-false", 0x19c8991bdc046eac),
    ("GrayscaleAlpha-Eight-false/pdn", 0x7f3a46d139acca4f),
    ("GrayscaleAlpha-Sixteen-false", 0xd8c71158282d52ea),
    ("GrayscaleAlpha-Sixteen-false/pdn", 0xa1ef78245a6c7164),
    ("Rgb-Eight-false", 0x0e572308fa3c1234),
    ("Rgb-Eight-false/pdn", 0x277f39dee8d729a7),
    ("Rgb-Eight-true", 0x0e572308fa3c1234),
    ("Rgb-Eight-true/pdn", 0x277f39dee8d729a7),
    ("Rgb-Sixteen-false", 0x70ab8fc02a9a4c51),
    ("Rgb-Sixteen-false/pdn", 0xc1fdf79cd65757d8),
    ("Rgba-Eight-false", 0x831b6d57b3ba4102),
    ("Rgba-Eight-false/pdn", 0x9f9a88fd819f54ad),
    ("Rgba-Sixteen-false", 0x5190728b33cc219e),
    ("Rgba-Sixteen-false/pdn", 0x0e9d0e761a740e82),
    ("Indexed-Two-true", 0x80bc9dca052cc07f),
    ("Indexed-Two-true/pdn", 0xfac6ca9a39439652),
    ("Indexed-Eight-false", 0xcf43f17ac042dd23),
    ("Indexed-Eight-false/pdn", 0xf2aa201b8c790651),
    ("Indexed-Eight-true", 0xcf1fb0e6fab0eb02),
    ("Indexed-Eight-true/pdn", 0x1d5fbaa980b28f32),
    ("FlattenBlendTest-pdn52.png", 0x20b83d969dd13b54),
    ("FlattenBlendTest-pdn52.png/pdn", 0x0d0ec69ea7f0e463),
    ("oldPDN3510-reference.png", 0xc433b7d1ab3326ea),
    ("oldPDN3510-reference.png/pdn", 0xb38d13930b6b75c5),
    ("pdn52_blend_mosaic-pdn52.png", 0x7e5da085c5415ae3),
    ("pdn52_blend_mosaic-pdn52.png/pdn", 0xbf601070fce03e95),
    ("pdn52_roundtrip_layers-pdn52.png", 0x15aedaf6539c9075),
    ("pdn52_roundtrip_layers-pdn52.png/pdn", 0x4202b550a972101b),
];
