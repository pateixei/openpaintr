//! Golden tests that pin, byte for byte, the vector rasterization that
//! architecture plan item 2.4 rewrites: every shape, freeform paths, lines with
//! dashes and caps, and lasso selections, in both antialiasing modes.
//!
//! They check what the rasterizer's consumers produce, so they don't depend on
//! how coverage is stored. Drawing an opaque color over a transparent layer
//! makes the output alpha equal the coverage.

use eframe::egui::{Vec2, vec2};

use super::{
    super::{
        selection::{Selection, Shape},
        tolerance::FillPattern,
        vector_tools::{CurveKind, DashPreset, LineCapStyle},
    },
    *,
};

const CANVAS: (u32, u32) = (61, 47);
const AA_MODES: [AaMode; 2] = [AaMode::Analytical, AaMode::Ssaa4x4];
const OUTLINE: Pixel = Pixel([210, 40, 30, 255]);
const FILL: Pixel = Pixel([20, 90, 230, 255]);

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
        self.bytes(pixels.iter().flat_map(|pixel| pixel.0));
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

fn transparent() -> Vec<Pixel> {
    vec![Pixel::default(); (CANVAS.0 * CANVAS.1) as usize]
}

/// A translucent, non-uniform layer, to pin blending too.
fn painted() -> Vec<Pixel> {
    (0..CANVAS.0 * CANVAS.1)
        .map(|i| Pixel(i.wrapping_mul(2_654_435_761).rotate_left(7).to_le_bytes()))
        .collect()
}

fn mask() -> Selection {
    Selection::new_with_aa(Shape::Ellipse, (4, 3), (50, 40), AaMode::Ssaa4x4)
}

/// Two corners and a rotation in degrees.
type Placement = ((f32, f32), (f32, f32), f64);

/// Bounds and rotation: inside, rotated, across the edges, outside, tiny, and
/// larger than the canvas.
const PLACEMENTS: [Placement; 7] = [
    ((6.0, 5.0), (52.0, 41.0), 0.0),
    ((10.5, 8.25), (47.75, 33.5), 33.0),
    ((-14.0, -9.0), (30.0, 25.0), 0.0),
    ((40.0, 30.0), (80.0, 62.0), -20.0),
    ((100.0, 100.0), (140.0, 130.0), 0.0),
    ((20.2, 20.2), (21.4, 21.1), 0.0),
    ((-40.0, -30.0), (110.0, 90.0), 12.0),
];

/// A self-intersecting path that leaves the canvas.
fn freeform() -> Vec<Vec2> {
    (0..48)
        .map(|step| {
            let angle = step as f32 * 0.9;
            let radius = 4.0 + step as f32 * 0.8;
            vec2(28.0 + radius * angle.cos(), 22.0 + radius * angle.sin())
        })
        .collect()
}

fn render_shape(session: &ShapeSession, aa_mode: AaMode, hash: &mut Hash) {
    hash.pixels(&session.render_preview(&transparent(), CANVAS, None, aa_mode));
    hash.pixels(&session.render_preview(&painted(), CANVAS, Some(&mask()), aa_mode));
}

#[test]
fn every_shape_rasterizes_identically() {
    let mut results = Vec::new();
    for aa_mode in AA_MODES {
        for kind in ShapeKind::ALL {
            let mut hash = Hash::EMPTY;
            for fill_mode in ShapeFillMode::ALL {
                for thickness in [1, 7] {
                    let params = ShapeParams {
                        kind,
                        fill_mode,
                        thickness,
                        corner_radius: 8.0,
                        pattern: FillPattern::Solid,
                        blend: StrokeBlend::Normal,
                    };
                    if kind == ShapeKind::Freeform {
                        let points = freeform();
                        for count in [1, 2, 3, points.len()] {
                            let mut session =
                                ShapeSession::new(points[0], points[0], OUTLINE, FILL, params);
                            session.freeform_pts = points[..count].to_vec();
                            render_shape(&session, aa_mode, &mut hash);
                            session.nudge(-31.5, 12.25);
                            render_shape(&session, aa_mode, &mut hash);
                        }
                        continue;
                    }
                    for ((x0, y0), (x1, y1), degrees) in PLACEMENTS {
                        let mut session =
                            ShapeSession::new(vec2(x0, y0), vec2(x1, y1), OUTLINE, FILL, params);
                        session.affine.degrees = degrees;
                        render_shape(&session, aa_mode, &mut hash);
                    }
                }
            }
            results.push((format!("{aa_mode:?}/{kind:?}"), hash));
        }
        // Patterns sample by pixel position, and Overwrite replaces alpha.
        let mut hash = Hash::EMPTY;
        for pattern in FillPattern::ALL {
            for blend in StrokeBlend::ALL {
                let session = ShapeSession::new(
                    vec2(3.0, 2.0),
                    vec2(55.0, 44.0),
                    Pixel([210, 40, 30, 130]),
                    Pixel([20, 90, 230, 60]),
                    ShapeParams {
                        kind: ShapeKind::Ellipse,
                        fill_mode: ShapeFillMode::Both,
                        thickness: 5,
                        corner_radius: 0.0,
                        pattern,
                        blend,
                    },
                );
                render_shape(&session, aa_mode, &mut hash);
            }
        }
        results.push((format!("{aa_mode:?}/patterns"), hash));
    }
    check(&results, SHAPE_GOLDEN);
}

#[test]
fn lines_with_every_dash_and_cap_rasterize_identically() {
    let curves = [
        [(5.0, 6.0), (20.0, 40.0), (38.0, -4.0), (56.0, 38.0)],
        [(-20.0, 23.0), (10.0, 23.0), (40.0, 23.0), (90.0, 23.0)],
        [(30.0, 24.0), (75.0, 60.0), (-15.0, 60.0), (30.0, 24.0)],
        [(12.0, 12.0), (12.0, 12.0), (12.0, 12.0), (12.0, 12.0)],
        [(200.0, 5.0), (220.0, 9.0), (240.0, 15.0), (260.0, 20.0)],
    ];
    let mut results = Vec::new();
    for aa_mode in AA_MODES {
        for dash in DashPreset::ALL {
            let mut hash = Hash::EMPTY;
            for kind in CurveKind::ALL {
                for start_cap in LineCapStyle::ALL {
                    for end_cap in LineCapStyle::ALL {
                        for thickness in [1, 3, 11] {
                            for points in curves {
                                let mut session = LineCurveSession::new(
                                    Vec2::ZERO,
                                    Vec2::ZERO,
                                    OUTLINE,
                                    LineCurveParams {
                                        kind,
                                        thickness,
                                        dash,
                                        start_cap,
                                        end_cap,
                                        blend: StrokeBlend::Normal,
                                    },
                                );
                                session.points = points.map(|(x, y)| vec2(x, y));
                                hash.pixels(&session.render_preview(
                                    &transparent(),
                                    CANVAS,
                                    None,
                                    aa_mode,
                                ));
                            }
                        }
                    }
                }
            }
            results.push((format!("{aa_mode:?}/{dash:?}"), hash));
        }
    }
    check(&results, LINE_GOLDEN);
}

#[test]
fn lasso_selections_are_identical() -> io::Result<()> {
    let paths: [&[(u32, u32)]; 6] = [
        &[(5, 5), (50, 9), (30, 40)],
        // Self-intersecting.
        &[(4, 4), (55, 40), (55, 4), (4, 40)],
        &[(0, 0), (60, 0), (60, 46), (0, 46)],
        // Partly and wholly outside the canvas.
        &[(30, 20), (200, 25), (35, 180)],
        &[(100, 100), (120, 100), (110, 130)],
        &[(10, 10), (11, 10), (10, 11)],
    ];
    let mut results = Vec::new();
    for aa_mode in AA_MODES {
        let mut hash = Hash::EMPTY;
        for path in paths {
            let selection = Selection::lasso_with_aa(CANVAS, path, aa_mode)?;
            hash.bytes((0..CANVAS.1).flat_map(|y| {
                let selection = &selection;
                (0..CANVAS.0).map(move |x| selection.coverage_u8((x, y)))
            }));
            let bounds = selection.bounds(CANVAS).unwrap_or(((99, 99), (99, 99)));
            hash.bytes([bounds.0.0, bounds.0.1, bounds.1.0, bounds.1.1].map(|v| v as u8));
        }
        results.push((format!("{aa_mode:?}/lasso"), hash));
    }
    check(&results, LASSO_GOLDEN);
    Ok(())
}

const SHAPE_GOLDEN: &[(&str, u64)] = &[
    ("Analytical/Rectangle", 0xe804060b4d1f1b57),
    ("Analytical/RoundedRect", 0xec1ccbd8be558c6b),
    ("Analytical/Ellipse", 0x21a0cc6b6eb43573),
    ("Analytical/Diamond", 0x6d7fcbb85423713a),
    ("Analytical/Trapezoid", 0x8662c25d56fa7771),
    ("Analytical/Parallelogram", 0x47b3e19d5474b2ed),
    ("Analytical/Triangle", 0x2bbd38a571d06bc7),
    ("Analytical/RightTriangle", 0xe730124060f4b821),
    ("Analytical/Pentagon", 0xf40e78a212af0c7f),
    ("Analytical/Hexagon", 0x9968fcab5c26e7d0),
    ("Analytical/Star4", 0x25357f4883b06f9d),
    ("Analytical/Star5", 0xbbafbb100be6fccc),
    ("Analytical/Star6", 0x9cad5c60030f69ac),
    ("Analytical/ArrowRight", 0x5b82e1863ddd275d),
    ("Analytical/ArrowLeft", 0x0a584120113a45c6),
    ("Analytical/ArrowUp", 0xf0061388d0298ba1),
    ("Analytical/ArrowDown", 0x9207b8b63750a013),
    ("Analytical/DoubleArrow", 0x272488355eefc12c),
    ("Analytical/Chevron", 0xe528385d8b37ce63),
    ("Analytical/Heart", 0x9a9ce1d1ae652494),
    ("Analytical/Lightning", 0x6abdd9c1f7d4ca9f),
    ("Analytical/Cross", 0x81943d56693e02b9),
    ("Analytical/Checkmark", 0x2d292138fb421ab7),
    ("Analytical/CalloutRect", 0x85b000645f7354a9),
    ("Analytical/CalloutOval", 0xb540ff8a3615ed12),
    ("Analytical/Freeform", 0x91d2e4515a0155ca),
    ("Analytical/patterns", 0xdcaa4281e7e59837),
    ("Ssaa4x4/Rectangle", 0x0b207e69c8b0aff5),
    ("Ssaa4x4/RoundedRect", 0x91ffd1b4a87b3440),
    ("Ssaa4x4/Ellipse", 0x3009c49fce50b579),
    ("Ssaa4x4/Diamond", 0x9bcbe3dfed5f40ed),
    ("Ssaa4x4/Trapezoid", 0x7f3cd7a5373f20d5),
    ("Ssaa4x4/Parallelogram", 0x5abb2bf435057e56),
    ("Ssaa4x4/Triangle", 0x524d20c2cefe3a4f),
    ("Ssaa4x4/RightTriangle", 0x7862abacb5e5beaf),
    ("Ssaa4x4/Pentagon", 0x26e1f500db8773a3),
    ("Ssaa4x4/Hexagon", 0xe8ea3fb4d49b4631),
    ("Ssaa4x4/Star4", 0x56dec3eef43e7d9f),
    ("Ssaa4x4/Star5", 0x917e8f6f46f9d32b),
    ("Ssaa4x4/Star6", 0x2fc63b23c435c89c),
    ("Ssaa4x4/ArrowRight", 0xed313a3d37c61836),
    ("Ssaa4x4/ArrowLeft", 0x0adff18348f5a9ba),
    ("Ssaa4x4/ArrowUp", 0xe4b7a79a85d0c29e),
    ("Ssaa4x4/ArrowDown", 0x72252f397bcf3923),
    ("Ssaa4x4/DoubleArrow", 0x47abe8f57abde41c),
    ("Ssaa4x4/Chevron", 0x5b5fb0c8cf787c47),
    ("Ssaa4x4/Heart", 0xea0cca5788f90ad4),
    ("Ssaa4x4/Lightning", 0x6c9bd04e5b7c96e1),
    ("Ssaa4x4/Cross", 0x39758f55a11815ae),
    ("Ssaa4x4/Checkmark", 0x9cbdb5840b98d209),
    ("Ssaa4x4/CalloutRect", 0x045e53177c59d3f8),
    ("Ssaa4x4/CalloutOval", 0xbcd399229271ec65),
    ("Ssaa4x4/Freeform", 0x4a5965bfe55b7e44),
    ("Ssaa4x4/patterns", 0x204d526d4a2d7f80),
];

const LINE_GOLDEN: &[(&str, u64)] = &[
    ("Analytical/Solid", 0xf0eac66e6ec11932),
    ("Analytical/Dash", 0xffa39d66413f55a9),
    ("Analytical/Dot", 0xa2e40cb6c7d6c4e7),
    ("Analytical/DashDot", 0xb5f78096f98d4d9d),
    ("Analytical/DashDotDot", 0x03f68da59bf882d1),
    ("Ssaa4x4/Solid", 0x0fb8a24b13250638),
    ("Ssaa4x4/Dash", 0x11eb60d75247a448),
    ("Ssaa4x4/Dot", 0x3e484ace97ad86f1),
    ("Ssaa4x4/DashDot", 0x3ed264e0493143d1),
    ("Ssaa4x4/DashDotDot", 0x0056bb5350802888),
];

const LASSO_GOLDEN: &[(&str, u64)] = &[
    ("Analytical/lasso", 0x503e7c07bb43f213),
    ("Ssaa4x4/lasso", 0x0b7f919632a0481b),
];
