//! Encodes a rendered image as PNG or JPG and writes it to disk.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};

use crate::cli::ImageFormat;
use crate::color::Rgb;
use crate::render::Canvas;

/// JPEG quality from `1` to `100`. High enough that colored text keeps clean edges.
const JPEG_QUALITY: u8 = 92;

/// Bytes per pixel in straight alpha RGBA.
pub const RGBA_BYTES: usize = 4;

/// A finished picture as straight alpha RGBA, the form encoders and clipboards take.
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `RGBA_BYTES` per pixel, row by row from the top.
    pub rgba: Vec<u8>,
}

impl From<Canvas> for Image {
    fn from(canvas: Canvas) -> Self {
        let (width, height) = (canvas.width(), canvas.height());
        Self {
            width,
            height,
            rgba: canvas.into_straight_rgba(),
        }
    }
}

/// Encodes `image` in `format` and writes it to `path`, replacing an existing file.
///
/// A JPG has no transparency, so transparent and translucent pixels are laid over white.
///
/// # Errors
///
/// Fails when the file cannot be created or written, or when encoding fails.
pub fn write(image: &Image, format: ImageFormat, path: &Path) -> Result<()> {
    let file = File::create(path).with_context(|| format!("cannot create {}", path.display()))?;
    let mut writer = BufWriter::new(file);
    encode(image, format, &mut writer)
        .with_context(|| format!("cannot write {}", path.display()))?;
    writer
        .flush()
        .with_context(|| format!("cannot write {}", path.display()))
}

/// Encodes `image` in `format` into `writer`.
///
/// # Errors
///
/// Fails when the encoder or the writer fails.
pub fn encode(image: &Image, format: ImageFormat, writer: &mut impl Write) -> Result<()> {
    let Image {
        width,
        height,
        ref rgba,
    } = *image;
    match format {
        ImageFormat::Png => {
            PngEncoder::new(writer).write_image(rgba, width, height, ExtendedColorType::Rgba8)?
        }
        ImageFormat::Jpg => {
            let rgb = flatten(rgba, Rgb::WHITE);
            JpegEncoder::new_with_quality(writer, JPEG_QUALITY).write_image(
                &rgb,
                width,
                height,
                ExtendedColorType::Rgb8,
            )?;
        }
    }
    Ok(())
}

/// Lays straight alpha RGBA pixels over an opaque `underlay` and returns RGB bytes.
fn flatten(rgba: &[u8], underlay: Rgb) -> Vec<u8> {
    const CHANNEL_MAX: u32 = 255;
    let underlay = [underlay.red, underlay.green, underlay.blue];
    let mut rgb = Vec::with_capacity(rgba.len() / RGBA_BYTES * 3);
    for pixel in rgba.as_chunks::<RGBA_BYTES>().0 {
        let alpha = u32::from(pixel[3]);
        for (value, below) in pixel[..3].iter().zip(underlay) {
            rgb.push(
                ((u32::from(*value) * alpha
                    + u32::from(below) * (CHANNEL_MAX - alpha)
                    + CHANNEL_MAX / 2)
                    / CHANNEL_MAX) as u8,
            );
        }
    }
    rgb
}

#[cfg(test)]
mod tests {
    use super::*;

    /// File signatures from the PNG and JPEG specifications.
    const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    const JPEG_SIGNATURE: &[u8] = &[0xff, 0xd8, 0xff];

    #[test]
    fn flatten_lays_pixels_over_the_underlay() {
        let rgba = [0, 0, 0, 0, 0, 0, 0, 255, 0, 0, 0, 128];
        assert_eq!(
            flatten(&rgba, Rgb::WHITE),
            [255, 255, 255, 0, 0, 0, 127, 127, 127]
        );
    }

    #[test]
    fn encodes_both_formats() {
        for (format, signature) in [
            (ImageFormat::Png, PNG_SIGNATURE),
            (ImageFormat::Jpg, JPEG_SIGNATURE),
        ] {
            let mut bytes = Vec::new();
            encode(&Image::from(Canvas::new(3, 2).unwrap()), format, &mut bytes).unwrap();
            assert!(bytes.starts_with(signature), "{format:?}");
        }
    }
}
