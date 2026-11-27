//! Oversized PNG headers are rejected before pixels are allocated. This file
//! holds a single test so that the process's peak memory belongs to it.

use std::io::{self, Cursor};

use openpaintr::{Diagnostic, Document};
use png::{ColorType, Encoder, chunk};

/// A tiny PNG whose header declares the given dimensions.
fn crafted(width: u32, height: u32) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes, width, height);
    encoder.set_color(ColorType::Rgba);
    let mut writer = encoder.write_header()?;
    writer.write_chunk(chunk::IDAT, &[0x78, 0x9c, 0x03, 0, 0, 0, 0, 1])?;
    writer.finish()?;
    Ok(bytes)
}

fn peak_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmHWM:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

#[test]
fn oversized_png_headers_are_rejected_without_allocating() -> io::Result<()> {
    let files = [crafted(32768, 32768)?, crafted(65535, 1)?];
    let before = peak_kib();
    for bytes in files {
        assert!(bytes.len() < 1024);
        let error = Document::read_png(Cursor::new(bytes))
            .err()
            .expect("oversized image accepted");
        let diagnostic = error
            .get_ref()
            .and_then(|source| source.downcast_ref::<Diagnostic>())
            .expect("localized error");
        assert_eq!(diagnostic.key(), "error.image-too-large");
    }
    if let (Some(before), Some(after)) = (before, peak_kib()) {
        println!("peak memory grew by {} KiB", after - before);
        assert!(after - before < 10 * 1024);
    }
    Ok(())
}
