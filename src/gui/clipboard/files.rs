use std::{
    io::{self, BufReader},
    path::Path,
};

use image::{DynamicImage, ImageDecoder as _, ImageError, ImageFormat, ImageReader, Limits};
use openpaintr::{MAX_IMAGE_SIDE, MAX_PIXELS, Pixel, check_image_dimensions, image_too_large};
use url::Url;

use super::{
    super::{selection::Point, storage::open_nonblocking},
    MAX_IMAGE_BYTES, tr,
};

pub(super) fn read_uri_list(bytes: &[u8]) -> io::Result<Option<(Point, Vec<Pixel>)>> {
    let Ok(list) = str::from_utf8(bytes) else {
        return Ok(None);
    };
    let mut entries = list
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let Some(uri) = entries.next().and_then(|entry| Url::parse(entry).ok()) else {
        return Ok(None);
    };
    if entries.next().is_some() || uri.scheme() != "file" {
        return Ok(None);
    }
    let Ok(path) = uri.to_file_path() else {
        return Ok(None);
    };
    read_image(&path)
}

pub(super) fn read_image(path: &Path) -> io::Result<Option<(Point, Vec<Pixel>)>> {
    if !path.is_absolute() {
        return Ok(None);
    }
    // Open once, then inspect the open file, so it can't be swapped in between.
    let Ok(file) = open_nonblocking(path) else {
        return Ok(None);
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Ok(None);
    }
    if metadata.len() > MAX_IMAGE_BYTES {
        return Err(io::Error::other(tr("clipboard-image-too-large")));
    }
    let mut reader = ImageReader::new(BufReader::new(file)).with_guessed_format()?;
    if !matches!(reader.format(), Some(ImageFormat::Jpeg | ImageFormat::Png)) {
        return Ok(None);
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    // Room for 16-bit RGBA.
    limits.max_alloc = Some(MAX_PIXELS * 8);
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(decode_error)?;
    let (width, height) = decoder.dimensions();
    check_image_dimensions(width, height, 1)?;
    let rgba = DynamicImage::from_decoder(decoder)
        .map_err(decode_error)?
        .into_rgba8();
    let size = (rgba.width(), rgba.height());
    let pixels = Pixel::from_rgba_bytes(&rgba.into_raw())?;
    Ok(Some((size, pixels)))
}

fn decode_error(error: ImageError) -> io::Error {
    match error {
        ImageError::Limits(_) => image_too_large(),
        _ => io::Error::new(io::ErrorKind::InvalidData, tr("clipboard-image-invalid")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copied_local_image_uri_decodes_jpeg_and_png() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        for extension in ["jpg", "png"] {
            let path = directory.path().join(format!("example #1.{extension}"));
            let color = [124, 62, 10];
            let rgb = color.repeat(6);
            image::save_buffer(&path, &rgb, 3, 2, image::ColorType::Rgb8)
                .map_err(io::Error::other)?;
            let uri = Url::from_file_path(&path).unwrap();
            let list = format!("# Copied file\r\n{uri}\r\n");
            let (size, pixels) = read_uri_list(list.as_bytes())?.unwrap();
            assert_eq!(size, (3, 2));
            assert_eq!(pixels.len(), 6);
            for pixel in pixels {
                assert!(
                    pixel.0[..3]
                        .iter()
                        .zip(color)
                        .all(|(&a, b)| a.abs_diff(b) <= 2)
                );
                assert_eq!(pixel.0[3], 255);
            }
            assert!(read_uri_list(format!("{uri}\n{uri}\n").as_bytes())?.is_none());
        }
        assert!(read_uri_list(b"file://remote.example/tmp/image.png\r\n")?.is_none());
        assert!(read_uri_list(b"https://example.com/image.png\n")?.is_none());
        Ok(())
    }

    #[test]
    fn oversized_images_are_rejected_from_files_and_from_bytes() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        for (width, height) in [(32768, 32768), (65535, 1)] {
            let mut bytes = Vec::new();
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_color(png::ColorType::Rgba);
            let mut writer = encoder.write_header()?;
            writer.write_chunk(png::chunk::IDAT, &[0x78, 0x9c, 0x03, 0, 0, 0, 0, 1])?;
            writer.finish()?;
            let path = directory.path().join(format!("{width}x{height}.png"));
            std::fs::write(&path, &bytes)?;
            let uri = Url::from_file_path(&path).unwrap();

            for error in [
                read_uri_list(uri.as_str().as_bytes()).map(drop),
                openpaintr::decode_png_rgba(io::Cursor::new(&bytes)).map(drop),
            ] {
                let error = error.expect_err("oversized image accepted");
                assert_eq!(
                    super::super::super::i18n::error_message(&error),
                    tr("error.image-too-large"),
                    "{width}x{height}"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn a_fifo_is_ignored_without_blocking() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("pipe.png");
        let status = std::process::Command::new("mkfifo").arg(&path).status()?;
        assert!(status.success());
        assert!(read_image(&path)?.is_none());
        Ok(())
    }
}
