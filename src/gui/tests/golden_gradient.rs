//! Pins, byte for byte, the gradient rendering that architecture plan item 2.3
//! optimizes.

use eframe::egui::vec2;

use super::{
    super::{
        gradient_tool::{GradientChannelMode, GradientRepeat},
        selection::{Selection, Shape},
    },
    *,
};

#[test]
fn gradients_are_identical_for_every_option() {
    let size = (37_u32, 23_u32);
    let base: Vec<Pixel> = (0..size.0 * size.1)
        .map(|i| Pixel(i.wrapping_mul(2_654_435_761).rotate_left(9).to_le_bytes()))
        .collect();
    let ellipse = Selection::new_with_aa(Shape::Ellipse, (3, 2), (30, 19), AaMode::Ssaa4x4);
    // 64-bit FNV-1a.
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for shape in GradientShape::ALL {
        for repeat in GradientRepeat::ALL {
            for channel_mode in GradientChannelMode::ALL {
                for dither in [false, true] {
                    for blend in [StrokeBlend::Normal, StrokeBlend::Overwrite] {
                        for (start, end, swapped) in [
                            (vec2(4.5, 3.0), vec2(28.0, 17.25), false),
                            (vec2(18.0, 11.0), vec2(18.0, 11.0), true),
                        ] {
                            let session = GradientSession::new(
                                start,
                                end,
                                Pixel([230, 40, 20, 220]),
                                Pixel([10, 90, 250, 30]),
                                swapped,
                                GradientParams {
                                    shape,
                                    repeat,
                                    channel_mode,
                                    dither,
                                    blend,
                                },
                            );
                            for selection in [None, Some(&ellipse)] {
                                for pixel in session.render_preview(&base, size, selection) {
                                    for byte in pixel.0 {
                                        hash =
                                            (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(
        hash, 0x1881_e226_1a0d_28e9,
        "output changed; actual: {hash:#018x}"
    );
}
