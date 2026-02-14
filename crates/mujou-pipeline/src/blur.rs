//! Gaussian blur for noise reduction before edge detection.
//!
//! Wraps [`imageproc::filter::gaussian_blur_f32`] to smooth images,
//! reducing high-frequency noise that would produce spurious edges
//! in the Canny detector.
//!
//! [`gaussian_blur`] operates on a single grayscale channel.
//! [`gaussian_blur_rgba`] applies the same blur independently to each
//! R/G/B/A channel of a color image, preserving color information for
//! the UI preview while preparing all channels for edge detection.

use image::GrayImage;

use crate::types::RgbaImage;

/// Apply Gaussian blur to a grayscale image.
///
/// Higher `sigma` values produce more smoothing. Non-positive sigma values
/// (zero or negative) return the image unchanged, since `imageproc`'s
/// underlying function panics on `sigma <= 0.0`.
#[must_use = "returns the blurred image"]
pub fn gaussian_blur(image: &GrayImage, sigma: f32) -> GrayImage {
    if sigma <= 0.0 {
        return image.clone();
    }

    imageproc::filter::gaussian_blur_f32(image, sigma)
}

/// Apply Gaussian blur to an RGBA image by blurring each channel
/// independently.
///
/// `imageproc::filter::gaussian_blur_f32` only accepts `GrayImage`, so
/// this function splits the RGBA image into four single-channel images,
/// blurs each, and reassembles. The result is mathematically equivalent
/// to blurring in color space (Gaussian blur is a linear, per-channel
/// operation).
///
/// Non-positive sigma values return the image unchanged.
///
/// This is step 3 in the pipeline, between downsampling and edge
/// detection.  Operating on the full RGBA image means the blur preview
/// shows color (not grayscale), and downstream edge detection can
/// extract already-blurred channels without redundant per-channel blur.
#[must_use = "returns the blurred RGBA image"]
pub fn gaussian_blur_rgba(image: &RgbaImage, sigma: f32) -> RgbaImage {
    if sigma <= 0.0 {
        return image.clone();
    }

    let (w, h) = (image.width(), image.height());

    // Split into four grayscale channels.
    let channels: [GrayImage; 4] = std::array::from_fn(|c| {
        GrayImage::from_fn(w, h, |x, y| image::Luma([image.get_pixel(x, y).0[c]]))
    });

    // Blur each channel independently.
    let blurred: [GrayImage; 4] =
        std::array::from_fn(|c| imageproc::filter::gaussian_blur_f32(&channels[c], sigma));

    // Reassemble into RGBA.
    RgbaImage::from_fn(w, h, |x, y| {
        image::Rgba([
            blurred[0].get_pixel(x, y).0[0],
            blurred[1].get_pixel(x, y).0[0],
            blurred[2].get_pixel(x, y).0[0],
            blurred[3].get_pixel(x, y).0[0],
        ])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a test image with a sharp black-to-white boundary at x=5.
    fn sharp_edge_image() -> GrayImage {
        GrayImage::from_fn(10, 10, |x, _y| {
            if x < 5 {
                image::Luma([0])
            } else {
                image::Luma([255])
            }
        })
    }

    #[test]
    fn zero_sigma_returns_identical_image() {
        let img = sharp_edge_image();
        let blurred = gaussian_blur(&img, 0.0);
        assert_eq!(img, blurred);
    }

    #[test]
    fn negative_sigma_returns_identical_image() {
        let img = sharp_edge_image();
        let blurred = gaussian_blur(&img, -1.0);
        assert_eq!(img, blurred);
    }

    #[test]
    fn output_dimensions_preserved() {
        let img = GrayImage::new(17, 31);
        let blurred = gaussian_blur(&img, 1.4);
        assert_eq!(blurred.width(), 17);
        assert_eq!(blurred.height(), 31);
    }

    #[test]
    fn blur_smooths_sharp_edge() {
        let img = sharp_edge_image();
        let blurred = gaussian_blur(&img, 2.0);

        // At the boundary (x=4 and x=5), the blurred image should have
        // intermediate values rather than a sharp 0-to-255 jump.
        let left_of_edge = blurred.get_pixel(4, 5).0[0];
        let right_of_edge = blurred.get_pixel(5, 5).0[0];

        // The originally-black side should be brighter than 0.
        assert!(
            left_of_edge > 0,
            "expected blur to raise left-of-edge above 0, got {left_of_edge}",
        );
        // The originally-white side should be darker than 255.
        assert!(
            right_of_edge < 255,
            "expected blur to lower right-of-edge below 255, got {right_of_edge}",
        );
    }

    #[test]
    fn uniform_image_unchanged_by_blur() {
        // Blurring a uniform image should produce (approximately) the same image.
        let img = GrayImage::from_fn(10, 10, |_, _| image::Luma([128]));
        let blurred = gaussian_blur(&img, 1.4);
        for pixel in blurred.pixels() {
            let diff = i16::from(pixel.0[0]) - 128;
            assert!(
                diff.abs() <= 1,
                "expected uniform image to stay near 128 after blur, got {}",
                pixel.0[0],
            );
        }
    }

    // ─────── gaussian_blur_rgba tests ────────────────────────────

    #[test]
    fn rgba_zero_sigma_returns_identical_image() {
        let img = RgbaImage::from_fn(4, 4, |_, _| image::Rgba([100, 150, 200, 255]));
        let blurred = gaussian_blur_rgba(&img, 0.0);
        assert_eq!(img, blurred);
    }

    #[test]
    fn rgba_negative_sigma_returns_identical_image() {
        let img = RgbaImage::from_fn(4, 4, |_, _| image::Rgba([100, 150, 200, 255]));
        let blurred = gaussian_blur_rgba(&img, -1.0);
        assert_eq!(img, blurred);
    }

    #[test]
    fn rgba_output_dimensions_preserved() {
        let img = RgbaImage::new(17, 31);
        let blurred = gaussian_blur_rgba(&img, 1.4);
        assert_eq!(blurred.width(), 17);
        assert_eq!(blurred.height(), 31);
    }

    #[test]
    fn rgba_blur_smooths_sharp_color_edge() {
        // Left half red, right half blue — sharp boundary at x=5.
        let img = RgbaImage::from_fn(10, 10, |x, _y| {
            if x < 5 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            }
        });
        let blurred = gaussian_blur_rgba(&img, 2.0);

        // Near the boundary, red channel should be intermediate.
        let left = blurred.get_pixel(4, 5).0[0]; // red channel
        let right = blurred.get_pixel(5, 5).0[0];
        assert!(
            left < 255,
            "expected red to decrease near boundary, got {left}",
        );
        assert!(
            right > 0,
            "expected red to increase near boundary, got {right}",
        );
    }

    #[test]
    fn rgba_uniform_unchanged_by_blur() {
        let img = RgbaImage::from_fn(10, 10, |_, _| image::Rgba([100, 150, 200, 250]));
        let blurred = gaussian_blur_rgba(&img, 1.4);
        let expected: [u8; 4] = [100, 150, 200, 250];
        for pixel in blurred.pixels() {
            for (c, &exp) in expected.iter().enumerate() {
                let diff = i16::from(pixel.0[c]) - i16::from(exp);
                assert!(
                    diff.abs() <= 1,
                    "channel {c}: expected ~{exp}, got {}",
                    pixel.0[c],
                );
            }
        }
    }

    #[test]
    #[allow(clippy::cast_possible_truncation)]
    fn rgba_blur_matches_per_channel_gray_blur() {
        // Verify that gaussian_blur_rgba produces the same result as
        // splitting into channels, blurring each with gaussian_blur,
        // and reassembling.
        let img = RgbaImage::from_fn(10, 10, |x, y| {
            image::Rgba([
                ((x * 25) % 256) as u8,
                ((y * 30) % 256) as u8,
                (((x + y) * 20) % 256) as u8,
                255,
            ])
        });
        let sigma = 1.4;
        let rgba_blurred = gaussian_blur_rgba(&img, sigma);

        // Manual per-channel blur.
        let (w, h) = (img.width(), img.height());
        for c in 0..4 {
            let chan = GrayImage::from_fn(w, h, |x, y| image::Luma([img.get_pixel(x, y).0[c]]));
            let chan_blurred = gaussian_blur(&chan, sigma);
            for y in 0..h {
                for x in 0..w {
                    assert_eq!(
                        rgba_blurred.get_pixel(x, y).0[c],
                        chan_blurred.get_pixel(x, y).0[0],
                        "mismatch at ({x},{y}) channel {c}",
                    );
                }
            }
        }
    }
}
