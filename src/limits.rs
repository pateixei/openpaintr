//! Size limits for documents and decoded images.

use std::io;

use crate::{
    Diagnostic,
    diagnostic::{Message, msg},
};

/// Largest number of pixels in an image, counted across all of its layers.
pub const MAX_PIXELS: u64 = 64 * 1024 * 1024;
/// Largest width or height of a new, resized, or imported image.
pub const MAX_IMAGE_SIDE: u32 = 32 * 1024;

/// Checks image dimensions before pixels are allocated or decoded, and returns
/// the number of pixels per layer.
pub fn check_image_dimensions(width: u32, height: u32, layers: usize) -> io::Result<u64> {
    if width == 0 || height == 0 {
        return Err(Diagnostic::io(
            io::ErrorKind::InvalidData,
            msg::EMPTY_CANVAS,
        ));
    }
    if width > MAX_IMAGE_SIDE || height > MAX_IMAGE_SIDE {
        return Err(image_too_large());
    }
    check_layer_pixel_limit(
        width,
        height,
        1,
        io::ErrorKind::InvalidData,
        msg::IMAGE_TOO_LARGE,
    )?;
    check_layer_pixel_limit(
        width,
        height,
        layers,
        io::ErrorKind::InvalidData,
        msg::TOTAL_LAYER_LIMIT,
    )
}

/// The localized error for an image over the size limits.
pub fn image_too_large() -> io::Error {
    Diagnostic::io(io::ErrorKind::InvalidData, msg::IMAGE_TOO_LARGE)
}

pub(crate) fn check_layer_pixel_limit(
    width: u32,
    height: u32,
    layers: usize,
    kind: io::ErrorKind,
    message: Message,
) -> io::Result<u64> {
    let pixel_count = u64::from(width) * u64::from(height);
    let divisor = (layers as u64).max(1);
    if pixel_count > MAX_PIXELS / divisor {
        return Err(Diagnostic::io(kind, message));
    }
    Ok(pixel_count)
}
