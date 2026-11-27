use std::{fs, io, path::Path};

use image::{ImageFormat, ImageReader};
use openpaintr::Pixel;
use url::Url;

use super::{MAX_IMAGE_BYTES, super::selection::Point, tr};

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
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(None);
    };
    if !metadata.is_file() {
        return Ok(None);
    }
    if metadata.len() > MAX_IMAGE_BYTES {
        return Err(io::Error::other(tr("clipboard-image-too-large")));
    }
    let reader = ImageReader::open(path)?.with_guessed_format()?;
    if !matches!(reader.format(), Some(ImageFormat::Jpeg | ImageFormat::Png)) {
        return Ok(None);
    }
    let image = reader
        .decode()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, tr("clipboard-image-invalid")))?;
    let rgba = image.into_rgba8();
    let size = (rgba.width(), rgba.height());
    let pixels = Pixel::from_rgba_bytes(&rgba.into_raw())?;
    Ok(Some((size, pixels)))
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
}
