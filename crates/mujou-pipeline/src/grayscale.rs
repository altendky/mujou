//! Image decoding and grayscale conversion.
//!
//! Accepts raw image bytes (PNG, JPEG, BMP, WebP) and produces a
//! single-channel grayscale image suitable for the processing pipeline.
//!
//! This is the first step in the pipeline: raw bytes in, `GrayImage` out.

use image::GrayImage;

use crate::types::PipelineError;

/// Decode raw image bytes and convert to grayscale.
///
/// Supports PNG, JPEG, BMP, and WebP formats (whatever the `image` crate
/// can decode). The standard luminance formula is used for RGB-to-gray
/// conversion: `0.299*R + 0.587*G + 0.114*B`.
///
/// # Errors
///
/// Returns [`PipelineError::EmptyInput`] if `bytes` is empty.
/// Returns [`PipelineError::ImageDecode`] if the image format is
/// unrecognized or the data is corrupt.
#[must_use = "returns the decoded grayscale image"]
pub fn decode_and_grayscale(bytes: &[u8]) -> Result<GrayImage, PipelineError> {
    if bytes.is_empty() {
        return Err(PipelineError::EmptyInput);
    }

    let img = image::load_from_memory(bytes)?;
    Ok(img.to_luma8())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_error() {
        let result = decode_and_grayscale(&[]);
        assert!(matches!(result, Err(PipelineError::EmptyInput)));
    }

    #[test]
    fn corrupt_bytes_returns_image_decode_error() {
        let result = decode_and_grayscale(&[0xFF, 0xFE, 0x00, 0x01]);
        assert!(matches!(result, Err(PipelineError::ImageDecode(_))));
    }

    #[test]
    fn valid_png_decodes_to_grayscale() {
        // Create a minimal 2x2 white PNG in memory.
        let img = image::RgbaImage::from_fn(2, 2, |_, _| image::Rgba([255, 255, 255, 255]));
        let mut buf = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut buf);
        image::ImageEncoder::write_image(
            encoder,
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .ok();

        let gray = decode_and_grayscale(&buf).unwrap();
        // All pixels should be white (255) in grayscale.
        for pixel in gray.pixels() {
            assert_eq!(pixel.0[0], 255);
        }
    }

    #[test]
    fn output_dimensions_match_input() {
        let img = image::RgbaImage::from_fn(17, 31, |_, _| image::Rgba([128, 64, 32, 255]));
        let mut buf = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut buf);
        image::ImageEncoder::write_image(
            encoder,
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .ok();

        let gray = decode_and_grayscale(&buf).unwrap();
        assert_eq!(gray.width(), 17);
        assert_eq!(gray.height(), 31);
    }

    #[test]
    fn grayscale_conversion_produces_distinct_channel_values() {
        // Verify that different RGB channels produce different grayscale
        // values, confirming a weighted luminance conversion (not a
        // simple average).
        let red = encode_rgba_pixel(255, 0, 0);
        let green = encode_rgba_pixel(0, 255, 0);
        let blue = encode_rgba_pixel(0, 0, 255);

        let r_val = decode_and_grayscale(&red).unwrap().get_pixel(0, 0).0[0];
        let g_val = decode_and_grayscale(&green).unwrap().get_pixel(0, 0).0[0];
        let b_val = decode_and_grayscale(&blue).unwrap().get_pixel(0, 0).0[0];

        // Green should produce the brightest value (highest luminance weight).
        assert!(
            g_val > r_val && r_val > b_val,
            "expected green > red > blue luminance, got R={r_val} G={g_val} B={b_val}",
        );
    }

    /// Helper: encode a single 1x1 RGBA pixel as a PNG byte buffer.
    fn encode_rgba_pixel(r: u8, g: u8, b: u8) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(1, 1, |_, _| image::Rgba([r, g, b, 255]));
        let mut buf = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut buf);
        image::ImageEncoder::write_image(
            encoder,
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .ok();
        buf
    }
}
