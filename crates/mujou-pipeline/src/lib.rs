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
pub mod downsample;
pub mod edge;
pub mod grayscale;
pub mod join;
pub mod mask;
pub mod optimize;
pub mod simplify;
pub mod types;

pub use contour::{ContourTracer, ContourTracerKind};
pub use diagnostics::PipelineDiagnostics;
pub use downsample::DownsampleFilter;
pub use edge::max_gradient_magnitude;
pub use join::{PathJoiner, PathJoinerKind};
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
/// 1. Decode image
/// 2. Downsample to working resolution
/// 3. Convert to grayscale
/// 4. Gaussian blur (noise reduction)
/// 5. Canny edge detection
/// 6. Optional edge map inversion
/// 7. Contour tracing (pluggable strategy)
/// 8. Path simplification (Ramer-Douglas-Peucker)
/// 9. Optional circular mask
/// 10. Path ordering + joining into single continuous path (pluggable strategy;
///     each joiner handles its own ordering internally)
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
    use web_time::Instant;

    let pipeline_start = Instant::now();

    // 0. Decode the source image.
    let t = Instant::now();
    let decoded = grayscale::decode(image_bytes)?;
    let original = grayscale::to_rgba(&decoded);
    let decode_diag = StageDiagnostics {
        duration: t.elapsed(),
        metrics: StageMetrics::Decode {
            input_bytes: image_bytes.len(),
            width: original.width(),
            height: original.height(),
            pixel_count: u64::from(original.width()) * u64::from(original.height()),
        },
    };

    // 1. Downsample to working resolution.
    let t = Instant::now();
    let (downsampled_dynamic, downsample_applied) = downsample::downsample(
        &decoded,
        config.working_resolution,
        config.downsample_filter,
    );
    let downsampled = grayscale::to_rgba(&downsampled_dynamic);
    let downsample_diag = StageDiagnostics {
        duration: t.elapsed(),
        metrics: StageMetrics::Downsample {
            original_width: original.width(),
            original_height: original.height(),
            width: downsampled.width(),
            height: downsampled.height(),
            max_dimension: config.working_resolution,
            filter: config.downsample_filter.to_string(),
            applied: downsample_applied,
        },
    };

    // 2. Convert to grayscale (at working resolution).
    let t = Instant::now();
    let grayscale_img = grayscale::to_grayscale(&downsampled_dynamic);
    let dimensions = Dimensions {
        width: grayscale_img.width(),
        height: grayscale_img.height(),
    };
    let grayscale_diag = StageDiagnostics {
        duration: t.elapsed(),
        metrics: StageMetrics::Grayscale {
            width: dimensions.width,
            height: dimensions.height,
        },
    };

    // 2. Gaussian blur.
    let t = Instant::now();
    let blurred = blur::gaussian_blur(&grayscale_img, config.blur_sigma);
    let blur_diag = StageDiagnostics {
        duration: t.elapsed(),
        metrics: StageMetrics::Blur {
            sigma: config.blur_sigma,
        },
    };

    // 3. Canny edge detection.
    let t = Instant::now();
    let edges_raw = edge::canny(&blurred, config.canny_low, config.canny_high);
    let edge_duration = t.elapsed();
    let edge_pixel_count = count_edge_pixels(&edges_raw);
    let total_pixel_count = u64::from(edges_raw.width()) * u64::from(edges_raw.height());
    let edge_diag = StageDiagnostics {
        duration: edge_duration,
        metrics: StageMetrics::EdgeDetection {
            low_threshold: config.canny_low.max(edge::MIN_THRESHOLD),
            high_threshold: config
                .canny_high
                .max(edge::MIN_THRESHOLD)
                .max(config.canny_low.max(edge::MIN_THRESHOLD)),
            edge_pixel_count,
            total_pixel_count,
        },
    };

    // 4. Optional inversion of the edge map.
    let t = Instant::now();
    let (edges, invert_diag) = if config.invert {
        let inverted = edge::invert_edge_map(&edges_raw);
        let inv_edge_count = count_edge_pixels(&inverted);
        (
            inverted,
            Some(StageDiagnostics {
                duration: t.elapsed(),
                metrics: StageMetrics::Invert {
                    edge_pixel_count: inv_edge_count,
                },
            }),
        )
    } else {
        (edges_raw, None)
    };

    // 5. Contour tracing.
    let t = Instant::now();
    let contours = config.contour_tracer.trace(&edges);
    let contour_duration = t.elapsed();
    if contours.is_empty() {
        return Err(PipelineError::NoContours);
    }
    let (ct_total, ct_min, ct_max, ct_mean) = contour_stats(&contours);
    let contour_diag = StageDiagnostics {
        duration: contour_duration,
        metrics: StageMetrics::ContourTracing {
            contour_count: contours.len(),
            total_point_count: ct_total,
            min_contour_points: ct_min,
            max_contour_points: ct_max,
            mean_contour_points: ct_mean,
        },
    };

    // 6. Path simplification (RDP).
    let points_before_simplify = total_points(&contours);
    let t = Instant::now();
    let simplified = simplify::simplify_paths(&contours, config.simplify_tolerance);
    let simplify_duration = t.elapsed();
    let points_after_simplify = total_points(&simplified);
    #[allow(clippy::cast_precision_loss)]
    let reduction_ratio = if points_before_simplify > 0 {
        1.0 - (points_after_simplify as f64 / points_before_simplify as f64)
    } else {
        0.0
    };
    let simplify_diag = StageDiagnostics {
        duration: simplify_duration,
        metrics: StageMetrics::Simplification {
            tolerance: config.simplify_tolerance,
            polyline_count: simplified.len(),
            points_before: points_before_simplify,
            points_after: points_after_simplify,
            reduction_ratio,
        },
    };

    // 7. Optional circular mask.
    let t = Instant::now();
    let (masked, mask_diag) = if config.circular_mask {
        let center = Point::new(
            f64::from(dimensions.width) / 2.0,
            f64::from(dimensions.height) / 2.0,
        );
        let extent = dimensions.width.min(dimensions.height);
        let radius = f64::from(extent) * config.mask_diameter / 2.0;
        let pts_before = total_points(&simplified);
        let result = mask::apply_circular_mask(&simplified, center, radius);
        let pts_after = total_points(&result);
        let polys_after = result.len();
        (
            Some(result),
            Some(StageDiagnostics {
                duration: t.elapsed(),
                metrics: StageMetrics::Mask {
                    diameter: config.mask_diameter,
                    radius_px: radius,
                    polylines_before: simplified.len(),
                    polylines_after: polys_after,
                    points_before: pts_before,
                    points_after: pts_after,
                },
            }),
        )
    } else {
        (None, None)
    };

    // 8. Path ordering + joining into a single continuous path.
    let join_input = masked.as_deref().unwrap_or(&simplified);
    let join_input_polyline_count = join_input.len();
    let join_input_point_count = total_points(join_input);
    let t = Instant::now();
    let joined = config.path_joiner.join(join_input);
    let join_duration = t.elapsed();
    let join_output_point_count = joined.len();
    #[allow(clippy::cast_precision_loss)]
    let expansion_ratio = if join_input_point_count > 0 {
        join_output_point_count as f64 / join_input_point_count as f64
    } else {
        0.0
    };
    let join_diag = StageDiagnostics {
        duration: join_duration,
        metrics: StageMetrics::Join {
            strategy: format!("{:?}", config.path_joiner),
            input_polyline_count: join_input_polyline_count,
            input_point_count: join_input_point_count,
            output_point_count: join_output_point_count,
            expansion_ratio,
        },
    };

    let total_duration = pipeline_start.elapsed();

    let pipeline_diagnostics = PipelineDiagnostics {
        decode: decode_diag,
        downsample: downsample_diag,
        grayscale: grayscale_diag,
        blur: blur_diag,
        edge_detection: edge_diag,
        invert: invert_diag,
        contour_tracing: contour_diag,
        simplification: simplify_diag,
        mask: mask_diag,
        join: join_diag,
        total_duration,
        summary: PipelineSummary {
            image_width: dimensions.width,
            image_height: dimensions.height,
            pixel_count: u64::from(dimensions.width) * u64::from(dimensions.height),
            contour_count: contours.len(),
            final_point_count: joined.len(),
        },
    };

    Ok((
        StagedResult {
            original,
            downsampled,
            grayscale: grayscale_img,
            blurred,
            edges,
            contours,
            simplified,
            masked,
            joined,
            dimensions,
        },
        pipeline_diagnostics,
    ))
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
