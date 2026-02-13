//! mujou-pipeline: Pure image processing pipeline (sans-IO).
//!
//! Converts raster images into vector polylines through:
//! grayscale -> blur -> edge detection -> contour tracing ->
//! simplification -> optional mask -> ordering + joining.
//!
//! This crate has **no I/O dependencies** -- it operates on in-memory
//! byte slices and returns structured data. All browser/filesystem
//! interaction lives in `mujou-io`.

pub mod blur;
mod canny;
pub mod contour;
pub mod diagnostics;
pub mod edge;
pub mod grayscale;
pub mod join;
pub mod mask;
pub mod optimize;
pub mod pipeline;
pub mod simplify;
pub mod types;

pub use contour::{ContourTracer, ContourTracerKind};
pub use diagnostics::PipelineDiagnostics;
pub use edge::max_gradient_magnitude;
pub use join::{PathJoiner, PathJoinerKind};
pub use pipeline::Pipeline;
pub use types::{
    Dimensions, GrayImage, PipelineConfig, PipelineError, Point, Polyline, ProcessResult,
    RgbaImage, StagedResult,
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
/// 1. Decode image and convert to grayscale
/// 2. Gaussian blur (noise reduction)
/// 3. Canny edge detection
/// 4. Optional edge map inversion
/// 5. Contour tracing (pluggable strategy)
/// 6. Path simplification (Ramer-Douglas-Peucker)
/// 7. Optional circular mask
/// 8. Path ordering + joining into single continuous path (pluggable strategy;
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
    let (staged, _diagnostics) = process_staged_with_diagnostics(image_bytes, config)?;
    Ok(staged)
}

/// Run the full pipeline and return both the staged result and diagnostics.
///
/// This is the instrumented entry point. Diagnostics include per-stage
/// wall-clock durations, item counts, and derived statistics useful for
/// algorithm tuning.
///
/// # Errors
///
/// Same as [`process_staged`].
#[allow(clippy::too_many_lines)]
pub fn process_staged_with_diagnostics(
    image_bytes: &[u8],
    config: &PipelineConfig,
) -> Result<(StagedResult, PipelineDiagnostics), PipelineError> {
    use diagnostics::{
        PipelineSummary, StageDiagnostics, StageMetrics, contour_stats, count_edge_pixels,
        total_points,
    };
    use pipeline::{Advance, Stage};
    use web_time::Instant;

    let pipeline_start = Instant::now();
    let mut stage: Stage = Pipeline::new(image_bytes.to_vec(), config.clone()).into();

    // We collect diagnostics for each stage transition by timing each
    // advance() call and inspecting the output of the resulting stage.
    let mut decode_diag = None;
    let mut grayscale_diag = None;
    let mut blur_diag = None;
    let mut edge_diag = None;
    let mut invert_diag = None;
    let mut contour_diag = None;
    let mut simplify_diag = None;
    let mut mask_diag = None;
    let mut join_diag = None;

    loop {
        let t = Instant::now();
        match stage.advance()? {
            Advance::Next(next) => {
                let elapsed = t.elapsed();
                match &next {
                    Stage::Decoded(s) => {
                        let orig = s.original();
                        decode_diag = Some(StageDiagnostics {
                            duration: elapsed,
                            metrics: StageMetrics::Decode {
                                input_bytes: image_bytes.len(),
                                width: orig.width(),
                                height: orig.height(),
                                pixel_count: u64::from(orig.width()) * u64::from(orig.height()),
                            },
                        });
                    }
                    Stage::Grayscaled(s) => {
                        let dim = s.dimensions();
                        grayscale_diag = Some(StageDiagnostics {
                            duration: elapsed,
                            metrics: StageMetrics::Grayscale {
                                width: dim.width,
                                height: dim.height,
                            },
                        });
                    }
                    Stage::Blurred(_) => {
                        blur_diag = Some(StageDiagnostics {
                            duration: elapsed,
                            metrics: StageMetrics::Blur {
                                sigma: config.blur_sigma,
                            },
                        });
                    }
                    Stage::EdgesDetected(s) => {
                        // Edge detection includes optional inversion, but we
                        // want to separate the two for diagnostics. The Pipeline
                        // stage combines canny + invert into one detect_edges()
                        // call, so we report them together here and split invert
                        // out if applicable.
                        let edges = s.edges();
                        let edge_pixel_count = count_edge_pixels(edges);
                        let total_pixel_count =
                            u64::from(edges.width()) * u64::from(edges.height());
                        let (effective_low, effective_high) =
                            edge::clamp_thresholds(config.canny_low, config.canny_high);

                        edge_diag = Some(StageDiagnostics {
                            duration: elapsed,
                            metrics: StageMetrics::EdgeDetection {
                                low_threshold: effective_low,
                                high_threshold: effective_high,
                                edge_pixel_count,
                                total_pixel_count,
                            },
                        });

                        if config.invert {
                            // When invert is active, we can't separate the
                            // timings perfectly since detect_edges() does both.
                            // We attribute the full duration to edge detection
                            // and record invert with zero duration but real counts.
                            invert_diag = Some(StageDiagnostics {
                                duration: std::time::Duration::ZERO,
                                metrics: StageMetrics::Invert { edge_pixel_count },
                            });
                        }
                    }
                    Stage::ContoursTraced(s) => {
                        let contours = s.contours();
                        let ct_stats = contour_stats(contours);
                        contour_diag = Some(StageDiagnostics {
                            duration: elapsed,
                            metrics: StageMetrics::ContourTracing {
                                contour_count: contours.len(),
                                total_point_count: ct_stats.total,
                                min_contour_points: ct_stats.min,
                                max_contour_points: ct_stats.max,
                                mean_contour_points: ct_stats.mean,
                            },
                        });
                    }
                    Stage::Simplified(s) => {
                        let simplified = s.simplified();
                        let points_after = total_points(simplified);
                        // points_before comes from contour_diag
                        let points_before = contour_diag
                            .as_ref()
                            .and_then(|d| match &d.metrics {
                                StageMetrics::ContourTracing {
                                    total_point_count, ..
                                } => Some(*total_point_count),
                                _ => None,
                            })
                            .unwrap_or(0);
                        #[allow(clippy::cast_precision_loss)]
                        let reduction_ratio = if points_before > 0 {
                            1.0 - (points_after as f64 / points_before as f64)
                        } else {
                            0.0
                        };
                        simplify_diag = Some(StageDiagnostics {
                            duration: elapsed,
                            metrics: StageMetrics::Simplification {
                                tolerance: config.simplify_tolerance,
                                polyline_count: simplified.len(),
                                points_before,
                                points_after,
                                reduction_ratio,
                            },
                        });
                    }
                    Stage::Masked(s) => {
                        let masked_data = s.masked();
                        let simplified_pts = simplify_diag
                            .as_ref()
                            .and_then(|d| match &d.metrics {
                                StageMetrics::Simplification {
                                    polyline_count,
                                    points_after,
                                    ..
                                } => Some((*polyline_count, *points_after)),
                                _ => None,
                            })
                            .unwrap_or((0, 0));

                        if let Some(masked_polys) = masked_data {
                            let dim = grayscale_diag
                                .as_ref()
                                .and_then(|d| match &d.metrics {
                                    StageMetrics::Grayscale { width, height } => {
                                        Some((*width, *height))
                                    }
                                    _ => None,
                                })
                                .unwrap_or((0, 0));
                            let extent = dim.0.min(dim.1);
                            let radius = f64::from(extent) * config.mask_diameter / 2.0;
                            mask_diag = Some(StageDiagnostics {
                                duration: elapsed,
                                metrics: StageMetrics::Mask {
                                    diameter: config.mask_diameter,
                                    radius_px: radius,
                                    polylines_before: simplified_pts.0,
                                    polylines_after: masked_polys.len(),
                                    points_before: simplified_pts.1,
                                    points_after: total_points(masked_polys),
                                },
                            });
                        } else {
                            // Mask disabled — no-op stage, no diagnostics
                        }
                    }
                    Stage::Joined(s) => {
                        let joined = s.joined();
                        // Determine join input counts from masked or simplified
                        let (input_polyline_count, input_point_count) =
                            mask_diag.as_ref().map_or_else(
                                || {
                                    simplify_diag
                                        .as_ref()
                                        .and_then(|d| match &d.metrics {
                                            StageMetrics::Simplification {
                                                polyline_count,
                                                points_after,
                                                ..
                                            } => Some((*polyline_count, *points_after)),
                                            _ => None,
                                        })
                                        .unwrap_or((0, 0))
                                },
                                |md| match &md.metrics {
                                    StageMetrics::Mask {
                                        polylines_after,
                                        points_after,
                                        ..
                                    } => (*polylines_after, *points_after),
                                    _ => (0, 0),
                                },
                            );
                        #[allow(clippy::cast_precision_loss)]
                        let expansion_ratio = if input_point_count > 0 {
                            joined.len() as f64 / input_point_count as f64
                        } else {
                            0.0
                        };
                        join_diag = Some(StageDiagnostics {
                            duration: elapsed,
                            metrics: StageMetrics::Join {
                                strategy: config.path_joiner.to_string(),
                                input_polyline_count,
                                input_point_count,
                                output_point_count: joined.len(),
                                expansion_ratio,
                            },
                        });
                    }
                    Stage::Pending(_) => {}
                }
                stage = next;
            }
            Advance::Complete(done) => {
                let total_duration = pipeline_start.elapsed();
                let result = done.complete()?;

                let summary = PipelineSummary {
                    image_width: result.dimensions.width,
                    image_height: result.dimensions.height,
                    pixel_count: u64::from(result.dimensions.width)
                        * u64::from(result.dimensions.height),
                    contour_count: result.contours.len(),
                    final_point_count: result.joined.len(),
                };

                // These unwraps are safe: the pipeline loop always
                // visits every mandatory stage before reaching Complete.
                let diag_missing = |stage: &str| {
                    PipelineError::InvalidConfig(format!(
                        "diagnostics bug: {stage} diagnostics missing"
                    ))
                };
                let pipeline_diagnostics = PipelineDiagnostics {
                    decode: decode_diag.ok_or_else(|| diag_missing("decode"))?,
                    grayscale: grayscale_diag.ok_or_else(|| diag_missing("grayscale"))?,
                    blur: blur_diag.ok_or_else(|| diag_missing("blur"))?,
                    edge_detection: edge_diag.ok_or_else(|| diag_missing("edge detection"))?,
                    invert: invert_diag,
                    contour_tracing: contour_diag.ok_or_else(|| diag_missing("contour tracing"))?,
                    simplification: simplify_diag.ok_or_else(|| diag_missing("simplification"))?,
                    mask: mask_diag,
                    join: join_diag.ok_or_else(|| diag_missing("join"))?,
                    total_duration,
                    summary,
                };

                break Ok((result, pipeline_diagnostics));
            }
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

    // --- process_staged tests ---

    #[test]
    fn process_staged_populates_all_intermediates() {
        let png = sharp_edge_png(40, 40);
        let staged = process_staged(&png, &PipelineConfig::default()).unwrap();

        // Original RGBA has correct dimensions.
        assert_eq!(staged.original.width(), 40);
        assert_eq!(staged.original.height(), 40);

        // Raster stages have correct dimensions.
        assert_eq!(staged.grayscale.width(), 40);
        assert_eq!(staged.grayscale.height(), 40);
        assert_eq!(staged.blurred.width(), 40);
        assert_eq!(staged.blurred.height(), 40);
        assert_eq!(staged.edges.width(), 40);
        assert_eq!(staged.edges.height(), 40);

        // Vector stages are non-empty.
        assert!(!staged.contours.is_empty(), "expected contours");
        assert!(!staged.simplified.is_empty(), "expected simplified paths");
        assert!(!staged.joined.is_empty(), "expected joined path");

        // Mask enabled by default.
        assert!(staged.masked.is_some());

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
            circular_mask: true,
            mask_diameter: 1.0,
            ..PipelineConfig::default()
        };
        let staged = process_staged(&png, &config).unwrap();

        assert!(
            staged.masked.is_some(),
            "expected Some masked polylines when circular_mask=true"
        );
        // With mask_diameter=1.0 (full extent), the vertical edge at
        // x=20 on a 40x40 image should survive clipping.
        assert!(
            !staged.masked.as_ref().unwrap().is_empty(),
            "expected non-empty masked polylines with full-extent mask"
        );
    }

    #[test]
    fn process_staged_final_polyline_returns_joined_with_mask() {
        let png = sharp_edge_png(40, 40);
        let config = PipelineConfig {
            circular_mask: true,
            mask_diameter: 0.8,
            ..PipelineConfig::default()
        };
        let staged = process_staged(&png, &config).unwrap();

        // final_polyline always returns the joined path (mask is applied
        // before joining, so joined is always the final result).
        assert_eq!(staged.final_polyline(), &staged.joined);
    }

    #[test]
    fn process_staged_final_polyline_returns_joined_without_mask() {
        let png = sharp_edge_png(40, 40);
        let staged = process_staged(&png, &PipelineConfig::default()).unwrap();

        // final_polyline should return the joined path when no mask.
        assert_eq!(staged.final_polyline(), &staged.joined);
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
