//! Canny edge detection and edge map inversion.
//!
//! Wraps [`crate::canny::canny`] (a vendored + patched copy of
//! `imageproc::edges::canny`) to detect edges in a blurred grayscale
//! image. Returns a binary image where white pixels (255) are edges and
//! black pixels (0) are background.
//!
//! The optional [`invert_edge_map`] function flips the binary image so that
//! dark regions become traced instead of light-to-dark transitions.

use image::GrayImage;

use crate::types::{EdgeChannels, RgbaImage};

/// Minimum allowed Canny threshold.
///
/// A low threshold of zero causes every pixel with any gradient to be
/// treated as a potential edge, producing an extremely dense edge map
/// that overwhelms downstream contour tracing and path optimization.
/// See <https://github.com/altendky/mujou/issues/44>.
pub const MIN_THRESHOLD: f32 = 1.0;
const _: () = assert!(MIN_THRESHOLD > 0.0);

/// Clamp Canny thresholds to valid ranges.
///
/// Both thresholds are raised to at least [`MIN_THRESHOLD`] and
/// `low` is clamped to be at most `high`. Returns `(low, high)`.
#[must_use]
pub const fn clamp_thresholds(low_threshold: f32, high_threshold: f32) -> (f32, f32) {
    let high = high_threshold.max(MIN_THRESHOLD);
    let low = low_threshold.max(MIN_THRESHOLD).min(high);
    (low, high)
}

/// Detect edges using the Canny algorithm.
///
/// Returns a binary image: 255 for edge pixels, 0 for non-edge.
///
/// Internally, Canny performs Sobel gradient computation, non-maximum
/// suppression, and hysteresis thresholding. Pixels with gradient magnitude
/// above `high_threshold` are definite edges; those between `low_threshold`
/// and `high_threshold` are edges only if connected to a definite edge.
///
/// Both thresholds are clamped via [`clamp_thresholds`] to a minimum of
/// [`MIN_THRESHOLD`] and `low_threshold` is clamped to be at most
/// `high_threshold`. This prevents degenerate edge maps that would hang
/// the application.
///
/// This is step 4 in the pipeline, between Gaussian blur and contour tracing.
#[must_use = "returns the binary edge map"]
pub fn canny(image: &GrayImage, low_threshold: f32, high_threshold: f32) -> GrayImage {
    let (low, high) = clamp_thresholds(low_threshold, high_threshold);
    crate::canny::canny(image, low, high)
}

/// Maximum possible gradient magnitude for the Sobel 3×3 kernels used
/// by [`imageproc::edges::canny`].
///
/// Computed by brute-forcing all 2⁹ = 512 possible binary (0 / 255)
/// 3×3 pixel neighborhoods and returning the maximum
/// `sqrt(gx² + gy²)`.  This exactly mirrors the gradient norm used
/// internally by `imageproc`.
///
/// The result for Sobel 3×3 is `sqrt(5) × 2 × 255 ≈ 1140.39`, as
/// documented upstream.
///
/// When the gradient method becomes configurable, this function should
/// accept the kernel pair as parameters.
#[must_use]
pub fn max_gradient_magnitude() -> f32 {
    // Sobel 3×3 kernels (matching imageproc::kernel::{SOBEL_HORIZONTAL_3X3, SOBEL_VERTICAL_3X3}).
    const SOBEL_H: [i32; 9] = [-1, 0, 1, -2, 0, 2, -1, 0, 1];
    const SOBEL_V: [i32; 9] = [-1, -2, -1, 0, 0, 0, 1, 2, 1];

    let mut max_mag: f32 = 0.0;
    // Enumerate every possible binary 3×3 neighborhood (pixel = 0 or 255).
    for bits in 0_u32..512 {
        let mut gx: i32 = 0;
        let mut gy: i32 = 0;
        for i in 0..9 {
            let pixel = if bits & (1 << i) != 0 { 255_i32 } else { 0 };
            gx += pixel * SOBEL_H[i];
            gy += pixel * SOBEL_V[i];
        }
        #[allow(clippy::cast_precision_loss)]
        let mag = (gx as f32).hypot(gy as f32);
        if mag > max_mag {
            max_mag = mag;
        }
    }
    max_mag
}

/// Extract a single color channel from an RGBA image as a grayscale image.
///
/// `channel` selects which byte of the RGBA pixel to extract:
/// 0 = Red, 1 = Green, 2 = Blue, 3 = Alpha.
#[must_use]
fn extract_channel(rgba: &RgbaImage, channel: usize) -> GrayImage {
    GrayImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        image::Luma([rgba.get_pixel(x, y).0[channel]])
    })
}

/// Compute the HSV saturation channel from an RGBA image.
///
/// Saturation is defined as `(max(R,G,B) - min(R,G,B)) / max(R,G,B)`,
/// scaled to 0–255. When `max(R,G,B)` is zero (pure black), saturation
/// is zero.
#[must_use]
fn extract_saturation(rgba: &RgbaImage) -> GrayImage {
    GrayImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [r, g, b, _] = rgba.get_pixel(x, y).0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        if max == 0 {
            image::Luma([0])
        } else {
            // (max - min) / max, scaled to 0–255.
            let sat = u16::from(max - min) * 255 / u16::from(max);
            #[allow(clippy::cast_possible_truncation)]
            image::Luma([sat as u8])
        }
    })
}

/// Combine two binary edge maps via pixel-wise maximum.
///
/// Both images must have the same dimensions (caller must guarantee
/// this). Edge pixels (255) in either image appear in the output.
fn combine_edge_maps(a: &GrayImage, b: &GrayImage) -> GrayImage {
    GrayImage::from_fn(a.width(), a.height(), |x, y| {
        image::Luma([a.get_pixel(x, y).0[0].max(b.get_pixel(x, y).0[0])])
    })
}

/// Run Canny edge detection on multiple image channels and combine
/// the results via pixel-wise maximum.
///
/// `blurred_rgba` must already be Gaussian-blurred by the pipeline's
/// blur stage. For each enabled channel in `channels`:
/// 1. Extract the grayscale channel from the already-blurred RGBA.
/// 2. Run Canny with the given thresholds.
///
/// All per-channel edge maps are then combined by taking the maximum
/// value at each pixel, so edges detected in *any* channel appear in
/// the final output.
///
/// # Panics
///
/// Panics if no channels are enabled. Callers should validate via
/// [`EdgeChannels::any_enabled`] or [`PipelineConfig::validate`].
#[must_use = "returns the combined binary edge map"]
pub fn canny_combined(
    blurred_rgba: &RgbaImage,
    channels: &EdgeChannels,
    low_threshold: f32,
    high_threshold: f32,
) -> GrayImage {
    assert!(
        channels.any_enabled(),
        "at least one edge channel must be enabled"
    );

    let (low, high) = clamp_thresholds(low_threshold, high_threshold);

    // Collect grayscale channel images extracted from the already-blurred RGBA.
    let mut channel_images: Vec<GrayImage> = Vec::with_capacity(channels.count());

    if channels.luminance {
        channel_images.push(crate::grayscale::to_grayscale(
            &image::DynamicImage::ImageRgba8(blurred_rgba.clone()),
        ));
    }
    if channels.red {
        channel_images.push(extract_channel(blurred_rgba, 0));
    }
    if channels.green {
        channel_images.push(extract_channel(blurred_rgba, 1));
    }
    if channels.blue {
        channel_images.push(extract_channel(blurred_rgba, 2));
    }
    if channels.saturation {
        channel_images.push(extract_saturation(blurred_rgba));
    }

    let mut combined: Option<GrayImage> = None;

    for img in &channel_images {
        let edges = crate::canny::canny(img, low, high);
        combined = Some(match combined {
            Some(acc) => combine_edge_maps(&acc, &edges),
            None => edges,
        });
    }

    // Safety: we asserted at least one channel is enabled, so combined is Some.
    #[allow(clippy::unwrap_used)]
    combined.unwrap()
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
#[allow(clippy::unwrap_used)]
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
    fn max_gradient_magnitude_matches_imageproc_docs() {
        // imageproc documents the greatest possible edge strength as
        // `sqrt(5) * 2 * 255`, approximately 1140.39.
        let expected = 5_f32.sqrt() * 2.0 * 255.0;
        let actual = max_gradient_magnitude();
        assert!(
            (actual - expected).abs() < 0.01,
            "expected ≈{expected}, got {actual}",
        );
    }

    #[test]
    fn max_gradient_magnitude_is_positive() {
        assert!(max_gradient_magnitude() > 0.0);
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

    // ─────── Edge channel tests ──────────────────────────────────

    /// Helper: count edge pixels (value > 0) in a grayscale image.
    fn count_edges(img: &GrayImage) -> u32 {
        img.pixels().map(|p| u32::from(p.0[0] > 0)).sum()
    }

    /// 20x20 RGBA image with a sharp vertical luminance boundary at x = 10.
    fn sharp_edge_rgba() -> RgbaImage {
        RgbaImage::from_fn(20, 20, |x, _y| {
            if x < 10 {
                image::Rgba([0, 0, 0, 255])
            } else {
                image::Rgba([255, 255, 255, 255])
            }
        })
    }

    /// 20x20 RGBA image with an isoluminant hue boundary at x = 10.
    ///
    /// Left half: saturated red (R=180, G=70, B=70)
    /// Right half: saturated cyan (R=70, G=180, B=180)
    ///
    /// Both halves have nearly identical BT.601 luminance:
    ///   Left:  0.299*180 + 0.587*70 + 0.114*70 = 53.82 + 41.09 + 7.98 = 102.89
    ///   Right: 0.299*70 + 0.587*180 + 0.114*180 = 20.93 + 105.66 + 20.52 = 147.11
    ///
    /// Actually these aren't perfectly isoluminant, but close enough
    /// that with moderate blur the luminance edge is weak. A tighter
    /// pair can be used if needed.
    ///
    /// For a truly isoluminant pair we use:
    ///   Left:  R=200, G=100, B=100 → luma = 0.299*200 + 0.587*100 + 0.114*100 = 59.8 + 58.7 + 11.4 = 129.9
    ///   Right: R=100, G=140, B=130 → luma = 0.299*100 + 0.587*140 + 0.114*130 = 29.9 + 82.18 + 14.82 = 126.9
    ///
    /// Difference: ~3 gray levels — below typical Canny thresholds after blur.
    fn isoluminant_hue_boundary_rgba() -> RgbaImage {
        RgbaImage::from_fn(20, 20, |x, _y| {
            if x < 10 {
                image::Rgba([200, 100, 100, 255])
            } else {
                image::Rgba([100, 140, 130, 255])
            }
        })
    }

    #[test]
    fn extract_channel_red() {
        let rgba = RgbaImage::from_fn(2, 2, |_, _| image::Rgba([100, 150, 200, 255]));
        let red = extract_channel(&rgba, 0);
        assert_eq!(red.get_pixel(0, 0).0[0], 100);
    }

    #[test]
    fn extract_channel_green() {
        let rgba = RgbaImage::from_fn(2, 2, |_, _| image::Rgba([100, 150, 200, 255]));
        let green = extract_channel(&rgba, 1);
        assert_eq!(green.get_pixel(0, 0).0[0], 150);
    }

    #[test]
    fn extract_channel_blue() {
        let rgba = RgbaImage::from_fn(2, 2, |_, _| image::Rgba([100, 150, 200, 255]));
        let blue = extract_channel(&rgba, 2);
        assert_eq!(blue.get_pixel(0, 0).0[0], 200);
    }

    #[test]
    fn extract_saturation_pure_red() {
        // Pure red: R=255, G=0, B=0 → max=255, min=0, S = 255/255*255 = 255
        let rgba = RgbaImage::from_fn(1, 1, |_, _| image::Rgba([255, 0, 0, 255]));
        let sat = extract_saturation(&rgba);
        assert_eq!(sat.get_pixel(0, 0).0[0], 255);
    }

    #[test]
    fn extract_saturation_gray() {
        // Gray: R=G=B=128 → max=128, min=128, S = 0/128*255 = 0
        let rgba = RgbaImage::from_fn(1, 1, |_, _| image::Rgba([128, 128, 128, 255]));
        let sat = extract_saturation(&rgba);
        assert_eq!(sat.get_pixel(0, 0).0[0], 0);
    }

    #[test]
    fn extract_saturation_black() {
        // Black: R=G=B=0 → max=0, S = 0 (special case)
        let rgba = RgbaImage::from_fn(1, 1, |_, _| image::Rgba([0, 0, 0, 255]));
        let sat = extract_saturation(&rgba);
        assert_eq!(sat.get_pixel(0, 0).0[0], 0);
    }

    #[test]
    fn extract_saturation_half_saturated() {
        // R=200, G=100, B=100 → max=200, min=100, S = 100/200*255 ≈ 127
        let rgba = RgbaImage::from_fn(1, 1, |_, _| image::Rgba([200, 100, 100, 255]));
        let sat = extract_saturation(&rgba);
        let s = sat.get_pixel(0, 0).0[0];
        // Allow ±1 for integer rounding.
        assert!((i16::from(s) - 127).abs() <= 1, "expected ~127, got {s}",);
    }

    #[test]
    fn combine_edge_maps_takes_maximum() {
        let a = GrayImage::from_fn(3, 1, |x, _| image::Luma([if x == 0 { 255 } else { 0 }]));
        let b = GrayImage::from_fn(3, 1, |x, _| image::Luma([if x == 2 { 255 } else { 0 }]));
        let combined = combine_edge_maps(&a, &b);
        assert_eq!(combined.get_pixel(0, 0).0[0], 255); // from a
        assert_eq!(combined.get_pixel(1, 0).0[0], 0); // neither
        assert_eq!(combined.get_pixel(2, 0).0[0], 255); // from b
    }

    #[test]
    fn canny_combined_luminance_only_matches_single_channel() {
        let rgba = sharp_edge_rgba();
        let blurred_rgba = crate::blur::gaussian_blur_rgba(&rgba, 1.4);

        let channels = EdgeChannels {
            luminance: true,
            ..EdgeChannels::default()
        };
        let combined = canny_combined(&blurred_rgba, &channels, 50.0, 150.0);

        // Reference: blur luma directly and run single-channel canny.
        let luma = crate::grayscale::to_grayscale(&image::DynamicImage::ImageRgba8(rgba));
        let blurred_luma = crate::blur::gaussian_blur(&luma, 1.4);
        let single = canny(&blurred_luma, 50.0, 150.0);

        // Blur-then-grayscale ≈ grayscale-then-blur (both linear), but
        // integer rounding may cause ±1 differences. Verify edge maps
        // are identical or nearly so.
        let combined_count = count_edges(&combined);
        let single_count = count_edges(&single);
        let diff = (i64::from(combined_count) - i64::from(single_count)).unsigned_abs();
        assert!(
            diff <= 5,
            "luminance-only combined should closely match single-channel canny \
             (combined={combined_count}, single={single_count}, diff={diff})",
        );
    }

    #[test]
    fn canny_combined_detects_isoluminant_hue_edges() {
        let rgba = isoluminant_hue_boundary_rgba();
        let blurred_rgba = crate::blur::gaussian_blur_rgba(&rgba, 1.4);

        // Luminance-only should find few or no edges at the boundary.
        let luma_only = EdgeChannels {
            luminance: true,
            ..EdgeChannels::default()
        };
        let luma_edges = canny_combined(&blurred_rgba, &luma_only, 15.0, 40.0);
        let luma_count = count_edges(&luma_edges);

        // Red channel should find the boundary (R=200 vs R=100).
        let with_red = EdgeChannels {
            luminance: true,
            red: true,
            ..EdgeChannels::default()
        };
        let red_edges = canny_combined(&blurred_rgba, &with_red, 15.0, 40.0);
        let red_count = count_edges(&red_edges);

        assert!(
            red_count > luma_count,
            "adding red channel should detect more edges at isoluminant boundary \
             (luma_count={luma_count}, red_count={red_count})",
        );
    }

    #[test]
    fn canny_combined_saturation_detects_saturation_boundary() {
        // Left: saturated (R=255, G=0, B=0, S=255)
        // Right: desaturated (R=128, G=128, B=128, S=0)
        // Luminance differs (76 vs 128), so both modes find edges,
        // but saturation channel should also contribute.
        let rgba = RgbaImage::from_fn(20, 20, |x, _y| {
            if x < 10 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([128, 128, 128, 255])
            }
        });
        let blurred_rgba = crate::blur::gaussian_blur_rgba(&rgba, 1.4);

        let with_sat = EdgeChannels {
            luminance: false,
            saturation: true,
            ..EdgeChannels::default()
        };
        let sat_edges = canny_combined(&blurred_rgba, &with_sat, 15.0, 40.0);
        let sat_count = count_edges(&sat_edges);

        assert!(
            sat_count > 0,
            "saturation channel should detect edges at saturated/desaturated boundary",
        );
    }

    #[test]
    #[should_panic(expected = "at least one edge channel must be enabled")]
    fn canny_combined_panics_with_no_channels() {
        let rgba = sharp_edge_rgba();
        let none = EdgeChannels {
            luminance: false,
            red: false,
            green: false,
            blue: false,
            saturation: false,
        };
        let _ = canny_combined(&rgba, &none, 50.0, 150.0);
    }

    #[test]
    fn edge_channels_default_has_luminance_only() {
        let channels = EdgeChannels::default();
        assert!(channels.luminance);
        assert!(!channels.red);
        assert!(!channels.green);
        assert!(!channels.blue);
        assert!(!channels.saturation);
        assert!(channels.any_enabled());
        assert_eq!(channels.count(), 1);
    }

    #[test]
    fn edge_channels_count_and_any_enabled() {
        let all = EdgeChannels {
            luminance: true,
            red: true,
            green: true,
            blue: true,
            saturation: true,
        };
        assert_eq!(all.count(), 5);
        assert!(all.any_enabled());

        let none = EdgeChannels {
            luminance: false,
            red: false,
            green: false,
            blue: false,
            saturation: false,
        };
        assert_eq!(none.count(), 0);
        assert!(!none.any_enabled());
    }
}
