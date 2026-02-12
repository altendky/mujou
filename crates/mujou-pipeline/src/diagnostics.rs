//! Pipeline diagnostics: timing, counts, and other metrics for each stage.
//!
//! These diagnostics are permanent instrumentation intended for
//! algorithm tuning and parameter experimentation. Every call to
//! [`process_staged`](crate::process_staged) collects diagnostics
//! alongside the pipeline results.
//!
//! Duration measurements use [`std::time::Duration`] (platform-agnostic).
//! Timestamps are captured internally via the `web-time` crate, which
//! uses `performance.now()` on WASM and `std::time::Instant` on native.
//!
//! Durations are serialized as fractional seconds (`f64`) for JSON
//! compatibility, since `std::time::Duration` does not implement serde
//! traits.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Serde support for `std::time::Duration` as fractional seconds.
mod duration_serde {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// Serialize a `Duration` as fractional seconds (`f64`).
    pub fn serialize<S: Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
        duration.as_secs_f64().serialize(serializer)
    }

    /// Deserialize a `Duration` from fractional seconds (`f64`).
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
        let secs = f64::deserialize(deserializer)?;
        Duration::try_from_secs_f64(secs).map_err(|_| {
            serde::de::Error::custom(
                "duration seconds must be finite, non-negative, and representable as a Duration",
            )
        })
    }
}

/// Diagnostics collected from a single pipeline run.
///
/// Each field captures metrics for one logical stage of the pipeline.
/// Stages that are conditionally skipped (e.g. mask, invert) have
/// `Option` fields that are `None` when the stage was not executed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineDiagnostics {
    /// Stage 0: image decoding.
    pub decode: StageDiagnostics,
    /// Stage 1: grayscale conversion.
    pub grayscale: StageDiagnostics,
    /// Stage 2: Gaussian blur.
    pub blur: StageDiagnostics,
    /// Stage 3: Canny edge detection.
    pub edge_detection: StageDiagnostics,
    /// Stage 4: edge map inversion (only when `config.invert == true`).
    pub invert: Option<StageDiagnostics>,
    /// Stage 5: contour tracing.
    pub contour_tracing: StageDiagnostics,
    /// Stage 6: RDP path simplification.
    pub simplification: StageDiagnostics,
    /// Stage 7: circular mask (only when `config.circular_mask == true`).
    pub mask: Option<StageDiagnostics>,
    /// Stage 8: path ordering + joining.
    pub join: StageDiagnostics,
    /// Total wall-clock duration of the entire pipeline (seconds).
    #[serde(with = "duration_serde")]
    pub total_duration: Duration,
    /// Summary counts across all stages.
    pub summary: PipelineSummary,
}

/// Diagnostics for a single pipeline stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageDiagnostics {
    /// Wall-clock duration of this stage (seconds).
    #[serde(with = "duration_serde")]
    pub duration: Duration,
    /// Stage-specific metrics (counts, sizes, etc.).
    pub metrics: StageMetrics,
}

/// Stage-specific metrics that vary by pipeline stage.
///
/// Each variant captures the counts and sizes meaningful for that
/// particular processing step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StageMetrics {
    /// Image decoding metrics.
    Decode {
        /// Size of the input image bytes.
        input_bytes: usize,
        /// Decoded image width in pixels.
        width: u32,
        /// Decoded image height in pixels.
        height: u32,
        /// Total pixel count (`width * height`).
        pixel_count: u64,
    },
    /// Grayscale conversion metrics.
    Grayscale {
        /// Image width in pixels.
        width: u32,
        /// Image height in pixels.
        height: u32,
    },
    /// Gaussian blur metrics.
    Blur {
        /// Sigma value used for the blur kernel.
        sigma: f32,
    },
    /// Canny edge detection metrics.
    EdgeDetection {
        /// Low threshold (after clamping).
        low_threshold: f32,
        /// High threshold (after clamping).
        high_threshold: f32,
        /// Number of edge pixels (value == 255) in the output.
        edge_pixel_count: u64,
        /// Total pixel count for computing edge density.
        total_pixel_count: u64,
    },
    /// Edge map inversion metrics.
    Invert {
        /// Number of edge pixels after inversion.
        edge_pixel_count: u64,
    },
    /// Contour tracing metrics.
    ContourTracing {
        /// Number of contours found.
        contour_count: usize,
        /// Total number of points across all contours.
        total_point_count: usize,
        /// Minimum points in any single contour.
        min_contour_points: usize,
        /// Maximum points in any single contour.
        max_contour_points: usize,
        /// Mean points per contour.
        mean_contour_points: f64,
    },
    /// Path simplification metrics.
    Simplification {
        /// RDP tolerance in pixels.
        tolerance: f64,
        /// Number of polylines after simplification.
        polyline_count: usize,
        /// Total points before simplification.
        points_before: usize,
        /// Total points after simplification.
        points_after: usize,
        /// Reduction ratio: `1.0 - (after / before)`.
        reduction_ratio: f64,
    },
    /// Circular mask metrics.
    Mask {
        /// Mask diameter as fraction of image extent.
        diameter: f64,
        /// Mask radius in pixels.
        radius_px: f64,
        /// Number of polylines before masking.
        polylines_before: usize,
        /// Number of polylines after masking (may increase due to splits).
        polylines_after: usize,
        /// Total points before masking.
        points_before: usize,
        /// Total points after masking.
        points_after: usize,
    },
    /// Path joining metrics.
    Join {
        /// Which joiner strategy was used.
        strategy: String,
        /// Number of input polylines.
        input_polyline_count: usize,
        /// Total input points.
        input_point_count: usize,
        /// Points in the final joined path.
        output_point_count: usize,
        /// Ratio of output to input points (> 1.0 means retrace added points).
        expansion_ratio: f64,
    },
}

/// High-level summary counts for the entire pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineSummary {
    /// Source image width in pixels.
    pub image_width: u32,
    /// Source image height in pixels.
    pub image_height: u32,
    /// Total pixel count.
    pub pixel_count: u64,
    /// Number of contours found.
    pub contour_count: usize,
    /// Points in the final output path.
    pub final_point_count: usize,
}

impl PipelineDiagnostics {
    /// Format diagnostics as a human-readable report.
    #[must_use]
    pub fn report(&self) -> String {
        let mut lines = Vec::new();

        lines.push(format!("Pipeline Diagnostics Report\n{}", "=".repeat(60)));
        lines.push(format!(
            "Image: {}x{} ({} pixels)",
            self.summary.image_width, self.summary.image_height, self.summary.pixel_count,
        ));
        lines.push(format!(
            "Total duration: {:.3}ms",
            duration_ms(self.total_duration),
        ));
        lines.push(String::new());

        // Per-stage breakdown.
        lines.push(format!(
            "{:<24} {:>10} {:>10}  {}",
            "Stage", "Duration", "% Total", "Details"
        ));
        lines.push("-".repeat(80));

        let total_ms = duration_ms(self.total_duration);

        let stages: Vec<(&str, &StageDiagnostics)> = {
            let mut s = vec![
                ("Decode", &self.decode),
                ("Grayscale", &self.grayscale),
                ("Blur", &self.blur),
                ("Edge Detection", &self.edge_detection),
            ];
            if let Some(ref inv) = self.invert {
                s.push(("Invert", inv));
            }
            s.push(("Contour Tracing", &self.contour_tracing));
            s.push(("Simplification", &self.simplification));
            if let Some(ref m) = self.mask {
                s.push(("Mask", m));
            }
            s.push(("Join", &self.join));
            s
        };

        for (name, diag) in &stages {
            let ms = duration_ms(diag.duration);
            let pct = if total_ms > 0.0 {
                ms / total_ms * 100.0
            } else {
                0.0
            };
            let details = format_metrics(&diag.metrics);
            lines.push(format!("{name:<24} {ms:>8.3}ms {pct:>9.1}%  {details}"));
        }

        lines.push(String::new());
        lines.push(format!(
            "Contours: {}  |  Final path points: {}",
            self.summary.contour_count, self.summary.final_point_count,
        ));

        lines.join("\n")
    }
}

/// Convert a `Duration` to milliseconds as `f64`.
fn duration_ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Format stage metrics into a compact detail string.
fn format_metrics(metrics: &StageMetrics) -> String {
    match metrics {
        StageMetrics::Decode {
            input_bytes,
            width,
            height,
            ..
        } => {
            format!("{input_bytes} bytes -> {width}x{height}")
        }
        StageMetrics::Grayscale { width, height } => format!("{width}x{height}"),
        StageMetrics::Blur { sigma } => format!("sigma={sigma:.2}"),
        StageMetrics::EdgeDetection {
            low_threshold,
            high_threshold,
            edge_pixel_count,
            total_pixel_count,
        } => {
            #[allow(clippy::cast_precision_loss)]
            let density = if *total_pixel_count > 0 {
                *edge_pixel_count as f64 / *total_pixel_count as f64 * 100.0
            } else {
                0.0
            };
            format!(
                "low={low_threshold:.1} high={high_threshold:.1} edges={edge_pixel_count} ({density:.1}%)",
            )
        }
        StageMetrics::Invert { edge_pixel_count } => {
            format!("edges_after={edge_pixel_count}")
        }
        StageMetrics::ContourTracing {
            contour_count,
            total_point_count,
            min_contour_points,
            max_contour_points,
            mean_contour_points,
        } => {
            format!(
                "{contour_count} contours, {total_point_count} pts (min={min_contour_points} max={max_contour_points} mean={mean_contour_points:.1})",
            )
        }
        StageMetrics::Simplification {
            tolerance,
            points_before,
            points_after,
            reduction_ratio,
            ..
        } => {
            format!(
                "tol={tolerance:.2} {points_before}->{points_after} pts ({:.1}% reduction)",
                reduction_ratio * 100.0,
            )
        }
        StageMetrics::Mask {
            diameter,
            radius_px,
            polylines_before,
            polylines_after,
            points_before,
            points_after,
        } => {
            format!(
                "d={diameter:.2} r={radius_px:.1}px polys={polylines_before}->{polylines_after} pts={points_before}->{points_after}",
            )
        }
        StageMetrics::Join {
            strategy,
            input_polyline_count,
            input_point_count,
            output_point_count,
            expansion_ratio,
        } => {
            format!(
                "{strategy} {input_polyline_count} polys, {input_point_count}->{output_point_count} pts (x{expansion_ratio:.2})",
            )
        }
    }
}

/// Count edge pixels (value == 255) in a grayscale image.
pub(crate) fn count_edge_pixels(image: &image::GrayImage) -> u64 {
    image
        .pixels()
        .map(|p| u64::from(u8::from(p.0[0] == 255)))
        .sum()
}

/// Statistics for a set of contour polylines.
pub(crate) struct ContourStats {
    /// Total number of points across all contours.
    pub total: usize,
    /// Minimum number of points in any single contour.
    pub min: usize,
    /// Maximum number of points in any single contour.
    pub max: usize,
    /// Mean number of points per contour.
    pub mean: f64,
}

/// Compute contour statistics from a set of polylines.
pub(crate) fn contour_stats(contours: &[crate::Polyline]) -> ContourStats {
    let total: usize = contours.iter().map(crate::Polyline::len).sum();
    let min = contours.iter().map(crate::Polyline::len).min().unwrap_or(0);
    let max = contours.iter().map(crate::Polyline::len).max().unwrap_or(0);
    #[allow(clippy::cast_precision_loss)]
    let mean = if contours.is_empty() {
        0.0
    } else {
        total as f64 / contours.len() as f64
    };
    ContourStats {
        total,
        min,
        max,
        mean,
    }
}

/// Total points across a slice of polylines.
pub(crate) fn total_points(polylines: &[crate::Polyline]) -> usize {
    polylines.iter().map(crate::Polyline::len).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_ms_converts_correctly() {
        let d = Duration::from_millis(1234);
        let ms = duration_ms(d);
        assert!((ms - 1234.0).abs() < 0.01);
    }

    #[test]
    fn count_edge_pixels_works() {
        let mut img = image::GrayImage::new(10, 10);
        // Set 5 pixels to edge (255)
        for i in 0..5 {
            img.put_pixel(i, 0, image::Luma([255]));
        }
        assert_eq!(count_edge_pixels(&img), 5);
    }

    #[test]
    fn contour_stats_empty() {
        let stats = contour_stats(&[]);
        assert_eq!(stats.total, 0);
        assert_eq!(stats.min, 0);
        assert_eq!(stats.max, 0);
        assert!((stats.mean - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn contour_stats_computes() {
        let contours = vec![
            crate::Polyline::new(vec![
                crate::Point::new(0.0, 0.0),
                crate::Point::new(1.0, 0.0),
            ]),
            crate::Polyline::new(vec![
                crate::Point::new(0.0, 0.0),
                crate::Point::new(1.0, 0.0),
                crate::Point::new(2.0, 0.0),
                crate::Point::new(3.0, 0.0),
            ]),
        ];
        let stats = contour_stats(&contours);
        assert_eq!(stats.total, 6);
        assert_eq!(stats.min, 2);
        assert_eq!(stats.max, 4);
        assert!((stats.mean - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn report_produces_nonempty_string() {
        let diag = PipelineDiagnostics {
            decode: StageDiagnostics {
                duration: Duration::from_millis(10),
                metrics: StageMetrics::Decode {
                    input_bytes: 1000,
                    width: 100,
                    height: 100,
                    pixel_count: 10000,
                },
            },
            grayscale: StageDiagnostics {
                duration: Duration::from_millis(5),
                metrics: StageMetrics::Grayscale {
                    width: 100,
                    height: 100,
                },
            },
            blur: StageDiagnostics {
                duration: Duration::from_millis(20),
                metrics: StageMetrics::Blur { sigma: 1.4 },
            },
            edge_detection: StageDiagnostics {
                duration: Duration::from_millis(30),
                metrics: StageMetrics::EdgeDetection {
                    low_threshold: 30.0,
                    high_threshold: 80.0,
                    edge_pixel_count: 500,
                    total_pixel_count: 10000,
                },
            },
            invert: None,
            contour_tracing: StageDiagnostics {
                duration: Duration::from_millis(15),
                metrics: StageMetrics::ContourTracing {
                    contour_count: 10,
                    total_point_count: 200,
                    min_contour_points: 5,
                    max_contour_points: 50,
                    mean_contour_points: 20.0,
                },
            },
            simplification: StageDiagnostics {
                duration: Duration::from_millis(5),
                metrics: StageMetrics::Simplification {
                    tolerance: 2.0,
                    polyline_count: 10,
                    points_before: 200,
                    points_after: 100,
                    reduction_ratio: 0.5,
                },
            },
            mask: None,
            join: StageDiagnostics {
                duration: Duration::from_millis(25),
                metrics: StageMetrics::Join {
                    strategy: "Retrace".to_string(),
                    input_polyline_count: 10,
                    input_point_count: 100,
                    output_point_count: 150,
                    expansion_ratio: 1.5,
                },
            },
            total_duration: Duration::from_millis(110),
            summary: PipelineSummary {
                image_width: 100,
                image_height: 100,
                pixel_count: 10000,
                contour_count: 10,
                final_point_count: 150,
            },
        };

        let report = diag.report();
        assert!(!report.is_empty());
        assert!(report.contains("Pipeline Diagnostics Report"));
        assert!(report.contains("Edge Detection"));
        assert!(report.contains("Retrace"));
    }
}
