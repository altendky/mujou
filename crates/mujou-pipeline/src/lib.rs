//! mujou-pipeline: Pure image processing pipeline (sans-IO).
//!
//! Converts raster images into vector polylines through:
//! grayscale -> blur -> edge detection -> contour tracing ->
//! simplification -> optimization -> joining -> optional mask.
//!
//! This crate has **no I/O dependencies** -- it operates on in-memory
//! byte slices and returns structured data. All browser/filesystem
//! interaction lives in `mujou-io`.

pub mod blur;
pub mod contour;
pub mod edge;
pub mod grayscale;
pub mod join;
pub mod mask;
pub mod optimize;
pub mod simplify;
pub mod types;

pub use contour::{ContourTracer, ContourTracerKind};
pub use join::{PathJoiner, PathJoinerKind};
pub use types::{Dimensions, PipelineConfig, PipelineError, Point, Polyline, ProcessResult};

/// Run the full image processing pipeline.
///
/// Takes raw image bytes (PNG, JPEG, BMP, WebP) and a configuration,
/// then produces a [`ProcessResult`] containing a single continuous
/// polyline and the source image dimensions. The dimensions are needed
/// by export serializers to set coordinate spaces (e.g., SVG `viewBox`).
///
/// # Pipeline steps
///
/// 1. Decode image and convert to grayscale
/// 2. Gaussian blur (noise reduction)
/// 3. Canny edge detection
/// 4. Optional edge map inversion
/// 5. Contour tracing (pluggable strategy)
/// 6. Path simplification (Ramer-Douglas-Peucker)
/// 7. Path optimization (nearest-neighbor reordering)
/// 8. Path joining into single continuous path (pluggable strategy)
/// 9. Optional circular mask
///
/// # Errors
///
/// Returns [`PipelineError::EmptyInput`] if `image_bytes` is empty.
/// Returns [`PipelineError::ImageDecode`] if the image format is unrecognized.
/// Returns [`PipelineError::NoContours`] if edge detection produces no contours.
pub fn process(
    image_bytes: &[u8],
    config: &PipelineConfig,
) -> Result<ProcessResult, PipelineError> {
    // 1. Decode and convert to grayscale.
    let gray = grayscale::decode_and_grayscale(image_bytes)?;
    let dimensions = Dimensions {
        width: gray.width(),
        height: gray.height(),
    };

    // 2. Gaussian blur.
    let blurred = blur::gaussian_blur(&gray, config.blur_sigma);

    // 3. Canny edge detection.
    let edges = edge::canny(&blurred, config.canny_low, config.canny_high);

    // 4. Optional inversion of the edge map.
    let edges = if config.invert {
        edge::invert_edge_map(&edges)
    } else {
        edges
    };

    // 5. Contour tracing.
    let contours = config.contour_tracer.trace(&edges);
    if contours.is_empty() {
        return Err(PipelineError::NoContours);
    }

    // 6. Path simplification (RDP).
    let simplified = simplify::simplify_paths(&contours, config.simplify_tolerance);

    // 7. Path optimization (nearest-neighbor reordering).
    let optimized = optimize::optimize_path_order(&simplified);

    // 8. Path joining into a single continuous path.
    let joined = config.path_joiner.join(&optimized);

    // 9. Optional circular mask.
    if config.circular_mask {
        let center = Point::new(
            f64::from(dimensions.width) / 2.0,
            f64::from(dimensions.height) / 2.0,
        );
        let extent = dimensions.width.min(dimensions.height);
        let radius = f64::from(extent) * config.mask_diameter / 2.0;
        let clipped = mask::clip_polyline_to_circle(&joined, center, radius);

        // Re-join the clipped segments into a single path.
        let rejoined = config.path_joiner.join(&clipped);
        Ok(ProcessResult {
            polyline: rejoined,
            dimensions,
        })
    } else {
        Ok(ProcessResult {
            polyline: joined,
            dimensions,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Create a minimal PNG with a sharp black/white boundary for testing.
    ///
    /// The left half is black, the right half is white, producing a strong
    /// vertical edge that Canny will detect.
    ///
    /// NOTE: The PNG encoding pattern here is duplicated in
    /// `grayscale.rs::encode_rgba_pixel`. See #4 for consolidation.
    fn sharp_edge_png(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(width, height, |x, _y| {
            if x < width / 2 {
                image::Rgba([0, 0, 0, 255])
            } else {
                image::Rgba([255, 255, 255, 255])
            }
        });
        let mut buf = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut buf);
        image::ImageEncoder::write_image(
            encoder,
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
        buf
    }

    #[test]
    fn process_empty_input() {
        let result = process(&[], &PipelineConfig::default());
        assert!(matches!(result, Err(PipelineError::EmptyInput)));
    }

    #[test]
    fn process_corrupt_input() {
        let result = process(&[0xFF, 0x00], &PipelineConfig::default());
        assert!(matches!(result, Err(PipelineError::ImageDecode(_))));
    }

    #[test]
    fn process_uniform_image_returns_no_contours() {
        // A uniform gray image should produce no edges and thus no contours.
        let img = image::RgbaImage::from_fn(20, 20, |_, _| image::Rgba([128, 128, 128, 255]));
        let mut buf = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut buf);
        image::ImageEncoder::write_image(
            encoder,
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();

        let result = process(&buf, &PipelineConfig::default());
        assert!(matches!(result, Err(PipelineError::NoContours)));
    }

    #[test]
    fn process_sharp_edge_produces_path() {
        let png = sharp_edge_png(40, 40);
        let result = process(&png, &PipelineConfig::default());
        assert!(result.is_ok(), "expected Ok, got {result:?}");
        let process_result = result.unwrap();
        assert!(
            !process_result.polyline.is_empty(),
            "expected non-empty polyline"
        );
        assert_eq!(
            process_result.dimensions,
            Dimensions {
                width: 40,
                height: 40
            }
        );
    }

    #[test]
    fn process_with_circular_mask() {
        let png = sharp_edge_png(40, 40);
        let config = PipelineConfig {
            circular_mask: true,
            mask_diameter: 0.8,
            ..PipelineConfig::default()
        };
        let result = process(&png, &config);
        assert!(result.is_ok(), "expected Ok with mask, got {result:?}");

        // All points should be within the mask circle.
        let process_result = result.unwrap();
        let center = Point::new(20.0, 20.0);
        let radius = 40.0 * 0.8 / 2.0; // 16.0
        for p in process_result.polyline.points() {
            let dist = p.distance(center);
            assert!(
                dist <= radius + 1e-6,
                "point ({}, {}) is outside mask circle (dist={dist}, radius={radius})",
                p.x,
                p.y,
            );
        }
    }

    #[test]
    fn process_with_circular_mask_nonsquare() {
        // Non-square image: mask radius should use min(width, height)
        // so the circle fits entirely within the image.
        let png = sharp_edge_png(60, 40);
        let config = PipelineConfig {
            circular_mask: true,
            mask_diameter: 1.0,
            ..PipelineConfig::default()
        };
        let result = process(&png, &config);
        assert!(result.is_ok(), "expected Ok with mask, got {result:?}");

        // Radius should be based on min(60, 40) = 40, so radius = 20.0.
        let process_result = result.unwrap();
        let center = Point::new(30.0, 20.0);
        let radius = 40.0 / 2.0; // 20.0, based on min dimension
        for p in process_result.polyline.points() {
            let dist = p.distance(center);
            assert!(
                dist <= radius + 1e-6,
                "point ({}, {}) is outside mask circle (dist={dist}, radius={radius})",
                p.x,
                p.y,
            );
        }
    }

    #[test]
    fn process_with_invert() {
        let png = sharp_edge_png(40, 40);
        let config = PipelineConfig {
            invert: true,
            ..PipelineConfig::default()
        };
        // Inverted edge map on a sharp-edge image should still produce
        // contours (the inverted map has large white regions).
        let result = process(&png, &config).unwrap();
        assert!(
            result.polyline.len() >= 2,
            "expected non-trivial polyline with invert, got {} points",
            result.polyline.len(),
        );
    }
}
