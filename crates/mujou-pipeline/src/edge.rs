//! Canny edge detection and edge map inversion.
//!
//! Wraps [`imageproc::edges::canny`] to detect edges in a blurred grayscale
//! image. Returns a binary image where white pixels (255) are edges and
//! black pixels (0) are background.
//!
//! The optional [`invert_edge_map`] function flips the binary image so that
//! dark regions become traced instead of light-to-dark transitions.

use image::GrayImage;

/// Minimum allowed Canny threshold.
///
/// A low threshold of zero causes every pixel with any gradient to be
/// treated as a potential edge, producing an extremely dense edge map
/// that overwhelms downstream contour tracing and path optimization.
/// See <https://github.com/altendky/mujou/issues/44>.
pub const MIN_THRESHOLD: f32 = 1.0;
const _: () = assert!(MIN_THRESHOLD > 0.0);

/// Detect edges using the Canny algorithm.
///
/// Returns a binary image: 255 for edge pixels, 0 for non-edge.
///
/// Internally, Canny performs Sobel gradient computation, non-maximum
/// suppression, and hysteresis thresholding. Pixels with gradient magnitude
/// above `high_threshold` are definite edges; those between `low_threshold`
/// and `high_threshold` are edges only if connected to a definite edge.
///
/// Both thresholds are clamped to a minimum of [`MIN_THRESHOLD`] and
/// `low_threshold` is clamped to be at most `high_threshold`. This
/// prevents degenerate edge maps that would hang the application.
///
/// This is step 4 in the pipeline, between Gaussian blur and contour tracing.
#[must_use = "returns the binary edge map"]
pub fn canny(image: &GrayImage, low_threshold: f32, high_threshold: f32) -> GrayImage {
    let high = high_threshold.max(MIN_THRESHOLD);
    let low = low_threshold.max(MIN_THRESHOLD).min(high);
    imageproc::edges::canny(image, low, high)
}

/// Invert a binary edge map (bitwise NOT).
///
/// Swaps edge pixels (255 → 0) and background pixels (0 → 255).
/// Applied between Canny edge detection and contour tracing when the
/// user enables the `invert` option.
#[must_use = "returns the inverted edge map"]
pub fn invert_edge_map(edges: &GrayImage) -> GrayImage {
    GrayImage::from_fn(edges.width(), edges.height(), |x, y| {
        image::Luma([!edges.get_pixel(x, y).0[0]])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 20x20 image with a sharp vertical boundary at x = 10.
    fn sharp_edge_image() -> GrayImage {
        GrayImage::from_fn(20, 20, |x, _y| {
            if x < 10 {
                image::Luma([0])
            } else {
                image::Luma([255])
            }
        })
    }

    #[test]
    fn blank_image_produces_no_edges() {
        let img = GrayImage::from_fn(20, 20, |_, _| image::Luma([128]));
        let edges = canny(&img, 50.0, 150.0);
        assert_eq!(edges.width(), 20);
        assert_eq!(edges.height(), 20);
        // Uniform image should have no edges.
        let edge_count: u32 = edges.pixels().map(|p| u32::from(p.0[0] > 0)).sum();
        assert_eq!(edge_count, 0, "expected no edges in uniform image");
    }

    #[test]
    fn sharp_edge_detected() {
        let img = sharp_edge_image();
        let edges = canny(&img, 50.0, 150.0);
        // There should be at least some edge pixels near x=10.
        let edge_count: u32 = edges.pixels().map(|p| u32::from(p.0[0] > 0)).sum();
        assert!(
            edge_count > 0,
            "expected edges at sharp boundary, found none"
        );
    }

    #[test]
    fn output_dimensions_match_input() {
        let img = GrayImage::new(17, 31);
        let edges = canny(&img, 50.0, 150.0);
        assert_eq!(edges.width(), 17);
        assert_eq!(edges.height(), 31);
    }

    #[test]
    fn invert_flips_all_values() {
        let mut img = GrayImage::new(5, 5);
        // Set some pixels to edge (255), leave others as background (0).
        img.put_pixel(1, 1, image::Luma([255]));
        img.put_pixel(3, 3, image::Luma([255]));

        let inverted = invert_edge_map(&img);

        // Originally-white pixels should now be black.
        assert_eq!(inverted.get_pixel(1, 1).0[0], 0);
        assert_eq!(inverted.get_pixel(3, 3).0[0], 0);
        // Originally-black pixels should now be white.
        assert_eq!(inverted.get_pixel(0, 0).0[0], 255);
        assert_eq!(inverted.get_pixel(2, 2).0[0], 255);
    }

    #[test]
    fn invert_preserves_dimensions() {
        let img = GrayImage::new(13, 29);
        let inverted = invert_edge_map(&img);
        assert_eq!(inverted.width(), 13);
        assert_eq!(inverted.height(), 29);
    }

    #[test]
    fn double_invert_is_identity() {
        let mut img = GrayImage::new(5, 5);
        img.put_pixel(2, 2, image::Luma([255]));
        let double_inverted = invert_edge_map(&invert_edge_map(&img));
        assert_eq!(img, double_inverted);
    }

    #[test]
    fn zero_low_threshold_is_clamped_to_min() {
        let img = sharp_edge_image();
        let edges_zero = canny(&img, 0.0, 150.0);
        let edges_min = canny(&img, MIN_THRESHOLD, 150.0);
        // canny(0.0, ...) should produce the same result as canny(MIN_THRESHOLD, ...)
        // because 0.0 gets clamped up to MIN_THRESHOLD.
        assert_eq!(edges_zero, edges_min);
    }

    #[test]
    fn low_above_high_is_clamped() {
        let img = sharp_edge_image();
        let edges_inverted = canny(&img, 200.0, 100.0);
        let edges_equal = canny(&img, 100.0, 100.0);
        // canny(200, 100) should produce the same result as canny(100, 100)
        // because low gets clamped down to high.
        assert_eq!(edges_inverted, edges_equal);
    }
}
