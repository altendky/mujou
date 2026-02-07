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
pub use types::{Dimensions, PipelineConfig, PipelineError, Point, Polyline};

/// Run the full image processing pipeline.
///
/// Takes raw image bytes (PNG, JPEG, BMP, WebP) and a configuration,
/// then produces a single continuous polyline suitable for sand tables,
/// pen plotters, and similar CNC devices.
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
pub fn process(image_bytes: &[u8], config: &PipelineConfig) -> Result<Polyline, PipelineError> {
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
        let radius = f64::from(dimensions.width) * config.mask_diameter / 2.0;
        let clipped = mask::clip_polyline_to_circle(&joined, center, radius);

        // Re-join the clipped segments into a single path.
        let rejoined = config.path_joiner.join(&clipped);
        Ok(rejoined)
    } else {
        Ok(joined)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a minimal PNG with a sharp black/white boundary for testing.
    ///
    /// The left half is black, the right half is white, producing a strong
    /// vertical edge that Canny will detect.
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
        .ok();
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
        .ok();

        let result = process(&buf, &PipelineConfig::default());
        assert!(matches!(result, Err(PipelineError::NoContours)));
    }

    #[test]
    fn process_sharp_edge_produces_path() {
        let png = sharp_edge_png(40, 40);
        let result = process(&png, &PipelineConfig::default());
        assert!(result.is_ok(), "expected Ok, got {result:?}");
        let polyline = result.ok();
        assert!(
            polyline.as_ref().is_some_and(|p| !p.is_empty()),
            "expected non-empty polyline",
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
        let polyline = result.ok();
        let center = Point::new(20.0, 20.0);
        let radius = 40.0 * 0.8 / 2.0; // 16.0
        if let Some(pl) = &polyline {
            for p in pl.points() {
                let dist = p.distance(center);
                assert!(
                    dist <= radius + 1e-6,
                    "point ({}, {}) is outside mask circle (dist={dist}, radius={radius})",
                    p.x,
                    p.y,
                );
            }
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
        let result = process(&png, &config);
        assert!(result.is_ok(), "expected Ok with invert, got {result:?}");
    }
}
