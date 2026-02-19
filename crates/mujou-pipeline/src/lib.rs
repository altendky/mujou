//! mujou-pipeline: Pure image processing pipeline (sans-IO).
//!
//! Converts raster images into vector polylines through:
//! blur -> edge detection -> contour tracing ->
//! simplification -> optional mask -> ordering + joining.
//!
//! This crate has **no I/O dependencies** -- it operates on in-memory
//! byte slices and returns structured data. All browser/filesystem
//! interaction lives in `mujou-io`.

pub mod blur;
mod canny;
pub mod contour;
pub mod diagnostics;
pub mod downsample;
pub mod edge;
pub mod grayscale;
pub mod join;
pub mod mask;
pub mod mst_join;
pub mod optimize;
pub mod pipeline;
pub mod segment_analysis;
pub mod simplify;
pub mod subsample;
pub mod types;

pub use contour::{ContourTracer, ContourTracerKind};
pub use diagnostics::PipelineDiagnostics;
pub use downsample::DownsampleFilter;
pub use edge::max_gradient_magnitude;
pub use join::{JoinOutput, PathJoiner, PathJoinerKind};
pub use mask::{BorderPathMode, ClippedPolyline, MaskMode, MaskResult, MaskShape};
pub use mst_join::{JoinQualityMetrics, MstEdgeInfo, ParityStrategy};
pub use pipeline::{Pipeline, PipelineCache};
pub use segment_analysis::{RankedSegment, SEGMENT_COLORS, find_top_segments};
pub use types::{
    Dimensions, EdgeChannels, GrayImage, PipelineConfig, PipelineError, Point, Polyline,
    ProcessResult, RgbaImage, StagedResult, StartPointStrategy,
};

/// Run the full image processing pipeline, preserving all intermediate
/// stage outputs.
///
/// Takes raw image bytes (PNG, JPEG, BMP, WebP) and a configuration,
/// then produces a [`StagedResult`] containing every intermediate result
/// along with the source image dimensions.
///
/// # Pipeline steps
///
/// 1. Decode image
/// 2. Downsample to working resolution
/// 3. Gaussian blur (RGBA, preserves color for UI preview)
/// 4. Canny edge detection
/// 5. Optional edge map inversion
/// 6. Contour tracing (pluggable strategy)
/// 7. Path simplification (Ramer-Douglas-Peucker)
/// 8. Optional mask (circle or rectangle)
/// 9. Path ordering + joining into single continuous path (pluggable strategy;
///    each joiner handles its own ordering internally)
///
/// # Errors
///
/// Returns [`PipelineError::EmptyInput`] if `image_bytes` is empty.
/// Returns [`PipelineError::ImageDecode`] if the image format is unrecognized.
/// Returns [`PipelineError::NoContours`] if edge detection produces no contours.
pub fn process_staged(
    image_bytes: &[u8],
    config: &PipelineConfig,
) -> Result<StagedResult, PipelineError> {
    use pipeline::{Advance, Stage};

    let mut stage: Stage = Pipeline::new(image_bytes.to_vec(), config.clone()).into();
    loop {
        match stage.advance()? {
            Advance::Next(next) => stage = next,
            Advance::Complete(done) => break done.complete(),
        }
    }
}

/// Run the full image processing pipeline.
///
/// Takes raw image bytes (PNG, JPEG, BMP, WebP) and a configuration,
/// then produces a [`ProcessResult`] containing a single continuous
/// polyline and the source image dimensions. The dimensions are needed
/// by export serializers to set coordinate spaces (e.g., SVG `viewBox`).
///
/// This is a convenience wrapper around [`process_staged`] that discards
/// intermediate results and returns only the final polyline.
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
    let staged = process_staged(image_bytes, config)?;
    Ok(ProcessResult {
        polyline: staged.final_polyline().clone(),
        dimensions: staged.dimensions,
    })
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
            mask_mode: MaskMode::Circle,
            mask_scale: 0.8,
            ..PipelineConfig::default()
        };
        let result = process(&png, &config);
        assert!(result.is_ok(), "expected Ok with mask, got {result:?}");

        // All points should be within the mask circle.
        let process_result = result.unwrap();
        let center = Point::new(20.0, 20.0);
        let diagonal = (40.0_f64).hypot(40.0);
        let radius = diagonal * 0.8 / 2.0;
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
        // Non-square image: mask radius is based on the diagonal so at
        // mask_scale=1.0 the circle circumscribes the full image.
        let png = sharp_edge_png(60, 40);
        let config = PipelineConfig {
            mask_mode: MaskMode::Circle,
            mask_scale: 1.0,
            ..PipelineConfig::default()
        };
        let result = process(&png, &config);
        assert!(result.is_ok(), "expected Ok with mask, got {result:?}");

        // Radius based on diagonal: sqrt(60^2 + 40^2) / 2 ≈ 36.06
        let process_result = result.unwrap();
        let center = Point::new(30.0, 20.0);
        let diagonal = (60.0_f64).hypot(40.0);
        let radius = diagonal * 1.0 / 2.0;
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
    fn process_with_circular_mask_above_one() {
        // mask_scale > 1.0 produces a circle larger than the diagonal,
        // so all image content should survive clipping.
        let png = sharp_edge_png(60, 40);
        let config = PipelineConfig {
            mask_mode: MaskMode::Circle,
            mask_scale: 1.3,
            ..PipelineConfig::default()
        };
        let result = process(&png, &config);
        assert!(result.is_ok(), "expected Ok with mask >1.0, got {result:?}");

        let process_result = result.unwrap();
        let center = Point::new(30.0, 20.0);
        let diagonal = (60.0_f64).hypot(40.0);
        let radius = diagonal * 1.3 / 2.0;
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

    // --- process_staged tests ---

    #[test]
    fn process_staged_populates_all_intermediates() {
        let png = sharp_edge_png(40, 40);
        let staged = process_staged(&png, &PipelineConfig::default()).unwrap();

        // Original RGBA has correct dimensions.
        assert_eq!(staged.original.width(), 40);
        assert_eq!(staged.original.height(), 40);

        // Raster stages have correct dimensions.
        assert_eq!(staged.blurred.width(), 40);
        assert_eq!(staged.blurred.height(), 40);
        assert_eq!(staged.edges.width(), 40);
        assert_eq!(staged.edges.height(), 40);

        // Vector stages are non-empty.
        assert!(!staged.contours.is_empty(), "expected contours");
        assert!(!staged.simplified.is_empty(), "expected simplified paths");
        assert!(!staged.joined.is_empty(), "expected joined path");

        // Mask enabled by default.
        assert!(staged.canvas.is_some());

        // Dimensions match source.
        assert_eq!(
            staged.dimensions,
            Dimensions {
                width: 40,
                height: 40
            }
        );
    }

    #[test]
    fn process_staged_with_mask_populates_masked() {
        let png = sharp_edge_png(40, 40);
        let config = PipelineConfig {
            mask_mode: MaskMode::Circle,
            mask_scale: 1.0,
            ..PipelineConfig::default()
        };
        let staged = process_staged(&png, &config).unwrap();

        assert!(
            staged.canvas.is_some(),
            "expected Some canvas polylines when mask_mode=Circle"
        );
        // With mask_scale=1.0 (circumscribing diagonal), the vertical
        // edge at x=20 on a 40x40 image should survive clipping.
        assert!(
            !staged.canvas.as_ref().unwrap().clipped.is_empty(),
            "expected non-empty canvas polylines with full-extent mask"
        );
    }

    #[test]
    fn process_staged_final_polyline_returns_output_with_mask() {
        let png = sharp_edge_png(40, 40);
        let config = PipelineConfig {
            mask_mode: MaskMode::Circle,
            mask_scale: 0.8,
            ..PipelineConfig::default()
        };
        let staged = process_staged(&png, &config).unwrap();

        // final_polyline returns the subsampled path (the final output
        // after joining and subsampling).
        assert_eq!(staged.final_polyline(), &staged.output);
    }

    #[test]
    fn process_staged_final_polyline_returns_output_without_mask() {
        let png = sharp_edge_png(40, 40);
        let staged = process_staged(&png, &PipelineConfig::default()).unwrap();

        // final_polyline should return the output path.
        assert_eq!(staged.final_polyline(), &staged.output);
    }

    #[test]
    fn process_delegates_to_process_staged() {
        // Verify that process() and process_staged() produce the same
        // final polyline and dimensions.
        let png = sharp_edge_png(40, 40);
        let config = PipelineConfig::default();

        let process_result = process(&png, &config).unwrap();
        let staged_result = process_staged(&png, &config).unwrap();

        assert_eq!(process_result.polyline, *staged_result.final_polyline());
        assert_eq!(process_result.dimensions, staged_result.dimensions);
    }

    #[test]
    fn process_with_zero_canny_low_does_not_hang() {
        // Regression test for https://github.com/altendky/mujou/issues/44
        // canny_low=0 used to produce a degenerate edge map that caused
        // the app to hang. The pipeline now clamps it to MIN_THRESHOLD.
        let png = sharp_edge_png(40, 40);
        let config = PipelineConfig {
            canny_low: 0.0,
            ..PipelineConfig::default()
        };
        let result = process(&png, &config);
        assert!(
            result.is_ok(),
            "canny_low=0 should be clamped and produce a valid result, got {result:?}"
        );
    }

    #[test]
    fn process_with_low_above_high_does_not_hang() {
        // Regression test: canny_low > canny_high should be clamped.
        let png = sharp_edge_png(40, 40);
        let config = PipelineConfig {
            canny_low: 200.0,
            canny_high: 50.0,
            ..PipelineConfig::default()
        };
        let result = process(&png, &config);
        // Should either succeed or return NoContours -- not hang.
        assert!(
            matches!(result, Ok(_) | Err(PipelineError::NoContours)),
            "canny_low > canny_high should be clamped, got {result:?}"
        );
    }
}
