//! Gaussian blur for noise reduction before edge detection.
//!
//! Wraps [`imageproc::filter::gaussian_blur_f32`] to smooth the grayscale
//! image, reducing high-frequency noise that would produce spurious edges
//! in the Canny detector.

use image::GrayImage;

/// Apply Gaussian blur to a grayscale image.
///
/// Higher `sigma` values produce more smoothing. Non-positive sigma values
/// (zero or negative) return the image unchanged, since `imageproc`'s
/// underlying function panics on `sigma <= 0.0`.
///
/// This is step 3 in the pipeline, between grayscale conversion and
/// Canny edge detection.
#[must_use = "returns the blurred image"]
pub fn gaussian_blur(image: &GrayImage, sigma: f32) -> GrayImage {
    if sigma <= 0.0 {
        return image.clone();
    }

    imageproc::filter::gaussian_blur_f32(image, sigma)
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
}
