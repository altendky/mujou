//! Pipeline diagnostics: timing, counts, and other metrics for each stage.
//!
//! These types describe pipeline instrumentation for algorithm tuning
//! and parameter experimentation. Each pipeline stage reports its own
//! metrics via [`PipelineStage::metrics()`](crate::pipeline::PipelineStage::metrics).
//!
//! This crate is sans-IO and does not read the system clock.
//! [`process_staged_with_diagnostics`] is generic over a [`Clock`]
//! trait so callers can supply any instant type — `std::time::Instant`
//! on native, `web_time::Instant` on WASM, or a fake clock in tests.
//!
//! Duration fields use [`std::time::Duration`] and are serialized as
//! fractional seconds (`f64`) for JSON compatibility.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::mst_join::JoinQualityMetrics;

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
    /// Stage 1: downsampling to working resolution.
    pub downsample: StageDiagnostics,
    /// Stage 2: Gaussian blur (RGBA, preserves color for UI preview).
    pub blur: StageDiagnostics,
    /// Stage 3: Canny edge detection.
    pub edge_detection: StageDiagnostics,
    /// Stage 4: edge map inversion (only when `config.invert == true`).
    ///
    /// **Note:** The invert operation runs inside the edge-detection stage
    /// transition, so its `duration` is always `Duration::ZERO`. The
    /// actual inversion cost is included in `edge_detection.duration`.
    /// This entry exists to report the post-inversion edge pixel count.
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
    /// Image downsampling metrics.
    Downsample {
        /// Original image width before downsampling.
        original_width: u32,
        /// Original image height before downsampling.
        original_height: u32,
        /// Image width after downsampling.
        width: u32,
        /// Image height after downsampling.
        height: u32,
        /// Target max dimension.
        max_dimension: u32,
        /// Resampling filter used.
        filter: String,
        /// Whether downsampling was actually applied.
        applied: bool,
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
        /// Number of edge channels that contributed to this edge map.
        channel_count: usize,
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
        /// Mask diameter as fraction of image diagonal.
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
        /// Quality metrics from the MST joiner (issue #89 evaluation criteria).
        ///
        /// `None` for non-MST joiners.
        quality: Option<JoinQualityMetrics>,
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
                ("Downsample", &self.downsample),
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
#[allow(clippy::too_many_lines)]
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
        StageMetrics::Downsample {
            original_width,
            original_height,
            width,
            height,
            max_dimension,
            filter,
            applied,
        } => {
            if *applied {
                format!(
                    "{original_width}x{original_height} -> {width}x{height} (target={max_dimension}, {filter})",
                )
            } else {
                format!("{original_width}x{original_height} (no change, <= {max_dimension})",)
            }
        }
        StageMetrics::Blur { sigma } => format!("sigma={sigma:.2}"),
        StageMetrics::EdgeDetection {
            low_threshold,
            high_threshold,
            edge_pixel_count,
            total_pixel_count,
            channel_count,
        } => {
            #[allow(clippy::cast_precision_loss)]
            let density = if *total_pixel_count > 0 {
                *edge_pixel_count as f64 / *total_pixel_count as f64 * 100.0
            } else {
                0.0
            };
            format!(
                "low={low_threshold:.1} high={high_threshold:.1} edges={edge_pixel_count} ({density:.1}%) channels={channel_count}",
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
            quality,
        } => {
            let base = format!(
                "{strategy} {input_polyline_count} polys, {input_point_count}->{output_point_count} pts (x{expansion_ratio:.2})",
            );
            if let Some(q) = quality {
                format!(
                    "{base} | mst={} edges, conn={:.1}px max={:.1}px retrace={:.1}px path={:.1}px odd={}->{}",
                    q.mst_edge_count,
                    q.total_mst_edge_weight,
                    q.max_mst_edge_weight,
                    q.total_retrace_distance,
                    q.total_path_length,
                    q.odd_vertices_before_fix,
                    q.odd_vertices_after_fix,
                )
            } else {
                base
            }
        }
    }
}

/// Count edge pixels (value == 255) in a grayscale image.
pub(crate) fn count_edge_pixels(image: &image::GrayImage) -> u64 {
    image.pixels().filter(|p| p.0[0] == 255).count() as u64
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

/// Abstraction over a monotonic clock.
///
/// Implement this for your platform's instant type so that
/// [`process_staged_with_diagnostics`] can measure stage durations
/// without depending on a specific time crate.
///
/// # Examples
///
/// ```rust
/// use std::time::{Duration, Instant};
/// use mujou_pipeline::diagnostics::Clock;
///
/// struct StdClock;
///
/// impl Clock for StdClock {
///     type Instant = Instant;
///     fn now(&self) -> Instant { Instant::now() }
///     fn elapsed(&self, since: &Instant) -> Duration { since.elapsed() }
/// }
/// ```
pub trait Clock {
    /// The instant type returned by [`now`](Self::now).
    type Instant;

    /// Capture the current instant.
    fn now(&self) -> Self::Instant;

    /// Measure the duration elapsed since `since`.
    fn elapsed(&self, since: &Self::Instant) -> Duration;
}

/// Run the full pipeline with per-stage timing instrumentation.
///
/// Each [`Stage::advance()`](crate::pipeline::Stage::advance) call is
/// timed using the supplied [`Clock`], and the elapsed duration is
/// paired with the stage's own
/// [`metrics()`](crate::pipeline::PipelineStage::metrics) to build a
/// [`PipelineDiagnostics`].
///
/// # Errors
///
/// Returns [`PipelineError`](crate::PipelineError) if any pipeline
/// stage fails.
pub fn process_staged_with_diagnostics<C: Clock>(
    image_bytes: &[u8],
    config: &crate::PipelineConfig,
    clock: &C,
) -> Result<(crate::StagedResult, PipelineDiagnostics), crate::PipelineError> {
    use crate::pipeline::{
        Advance, Blurred, ContoursTraced, Decoded, Downsampled, EdgesDetected, Joined, Masked,
        PipelineStage as _, STAGE_COUNT, Simplified, Stage,
    };

    let pipeline_start = clock.now();
    let mut stage: Stage = crate::Pipeline::new(image_bytes.to_vec(), config.clone()).into();

    let mut stage_diags: [Option<StageDiagnostics>; STAGE_COUNT] = std::array::from_fn(|_| None);
    let mut invert_diag = None;

    loop {
        let t = clock.now();
        match stage.advance()? {
            Advance::Next(next) => {
                let elapsed = clock.elapsed(&t);
                if let Some(metrics) = next.metrics() {
                    stage_diags[next.index()] = Some(StageDiagnostics {
                        duration: elapsed,
                        metrics,
                    });
                }
                if let Some(inv) = next.invert_metrics() {
                    invert_diag = Some(StageDiagnostics {
                        duration: Duration::ZERO,
                        metrics: inv,
                    });
                }
                stage = next;
            }
            Advance::Complete(done) => {
                let total_duration = clock.elapsed(&pipeline_start);
                let result = done.complete()?;

                let summary = PipelineSummary {
                    image_width: result.dimensions.width,
                    image_height: result.dimensions.height,
                    pixel_count: u64::from(result.dimensions.width)
                        * u64::from(result.dimensions.height),
                    contour_count: result.contours.len(),
                    final_point_count: result.joined.len(),
                };

                let diag_missing = |name: &str| {
                    crate::PipelineError::InvalidConfig(format!(
                        "diagnostics bug: {name} diagnostics missing"
                    ))
                };
                let pipeline_diagnostics = PipelineDiagnostics {
                    decode: stage_diags[Decoded::INDEX]
                        .take()
                        .ok_or_else(|| diag_missing(Decoded::NAME))?,
                    downsample: stage_diags[Downsampled::INDEX]
                        .take()
                        .ok_or_else(|| diag_missing(Downsampled::NAME))?,
                    blur: stage_diags[Blurred::INDEX]
                        .take()
                        .ok_or_else(|| diag_missing(Blurred::NAME))?,
                    edge_detection: stage_diags[EdgesDetected::INDEX]
                        .take()
                        .ok_or_else(|| diag_missing(EdgesDetected::NAME))?,
                    invert: invert_diag,
                    contour_tracing: stage_diags[ContoursTraced::INDEX]
                        .take()
                        .ok_or_else(|| diag_missing(ContoursTraced::NAME))?,
                    simplification: stage_diags[Simplified::INDEX]
                        .take()
                        .ok_or_else(|| diag_missing(Simplified::NAME))?,
                    mask: stage_diags[Masked::INDEX].take(),
                    join: stage_diags[Joined::INDEX]
                        .take()
                        .ok_or_else(|| diag_missing(Joined::NAME))?,
                    total_duration,
                    summary,
                };

                break Ok((result, pipeline_diagnostics));
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
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

    /// A deterministic clock for testing [`process_staged_with_diagnostics`].
    ///
    /// Each [`now()`](Clock::now) call returns a monotonically increasing
    /// tick value.  [`elapsed()`](Clock::elapsed) returns
    /// `10ms * (current_tick - since)` so that every stage gets a
    /// predictable, distinguishable duration.
    struct FakeClock {
        tick: std::cell::Cell<u64>,
    }

    impl FakeClock {
        const MS_PER_TICK: u64 = 10;

        fn new() -> Self {
            Self {
                tick: std::cell::Cell::new(0),
            }
        }
    }

    impl Clock for FakeClock {
        type Instant = u64;

        fn now(&self) -> u64 {
            let t = self.tick.get();
            self.tick.set(t + 1);
            t
        }

        fn elapsed(&self, since: &u64) -> Duration {
            let current = self.tick.get();
            Duration::from_millis((current - since) * Self::MS_PER_TICK)
        }
    }

    /// Generate a small PNG with a sharp vertical edge (left half black,
    /// right half white) that reliably produces contours.
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
    fn fake_clock_diagnostics_without_invert() {
        let png = sharp_edge_png(40, 40);
        let config = crate::PipelineConfig {
            circular_mask: false,
            invert: false,
            ..crate::PipelineConfig::default()
        };
        let clock = FakeClock::new();

        let (_result, diag) = process_staged_with_diagnostics(&png, &config, &clock)
            .expect("pipeline should succeed");

        // Call pattern (no invert, no mask):
        //   now#0: pipeline_start=0, tick→1
        //   now#1: t=1, tick→2; advance Pending->Decoded; elapsed(&1) at tick 2 => 10ms
        //   now#2: t=2, tick→3; advance Decoded->Downsampled; elapsed(&2) at tick 3 => 10ms
        //   ...each stage gets 10ms...
        //   now#7: t=7, tick→8; advance Masked->Joined; elapsed(&7) at tick 8 => 10ms
        //   now#8: t=8, tick→9; advance Joined->Complete; elapsed(&0) at tick 9 => 90ms

        let ten_ms = Duration::from_millis(10);

        assert_eq!(diag.decode.duration, ten_ms);
        assert_eq!(diag.downsample.duration, ten_ms);
        assert_eq!(diag.blur.duration, ten_ms);
        assert_eq!(diag.edge_detection.duration, ten_ms);
        assert!(diag.invert.is_none());
        assert_eq!(diag.contour_tracing.duration, ten_ms);
        assert_eq!(diag.simplification.duration, ten_ms);
        // mask disabled -> None
        assert!(diag.mask.is_none());
        assert_eq!(diag.join.duration, ten_ms);
        assert_eq!(diag.total_duration, Duration::from_millis(100));

        // Summary should reflect the 40x40 image.
        assert_eq!(diag.summary.image_width, 40);
        assert_eq!(diag.summary.image_height, 40);
        assert_eq!(diag.summary.pixel_count, 1600);
        assert!(diag.summary.contour_count > 0);
        assert!(diag.summary.final_point_count > 0);

        // Report should contain key sections.
        let report = diag.report();
        assert!(report.contains("Pipeline Diagnostics Report"));
        assert!(report.contains("Edge Detection"));
    }

    #[test]
    fn fake_clock_diagnostics_with_invert_and_mask() {
        let png = sharp_edge_png(40, 40);
        let config = crate::PipelineConfig {
            circular_mask: true,
            invert: true,
            ..crate::PipelineConfig::default()
        };
        let clock = FakeClock::new();

        let (_result, diag) = process_staged_with_diagnostics(&png, &config, &clock)
            .expect("pipeline should succeed");

        let ten_ms = Duration::from_millis(10);

        assert_eq!(diag.decode.duration, ten_ms);
        assert_eq!(diag.downsample.duration, ten_ms);
        assert_eq!(diag.blur.duration, ten_ms);
        assert_eq!(diag.edge_detection.duration, ten_ms);

        // Invert should be present with Duration::ZERO.
        let invert = diag
            .invert
            .as_ref()
            .expect("invert diagnostics should be Some");
        assert_eq!(invert.duration, Duration::ZERO);
        assert!(
            matches!(invert.metrics, StageMetrics::Invert { .. }),
            "invert metrics should be Invert variant"
        );

        assert_eq!(diag.contour_tracing.duration, ten_ms);
        assert_eq!(diag.simplification.duration, ten_ms);

        // Mask enabled -> Some with timing.
        let mask = diag.mask.as_ref().expect("mask diagnostics should be Some");
        assert_eq!(mask.duration, ten_ms);

        assert_eq!(diag.join.duration, ten_ms);
        assert_eq!(diag.total_duration, Duration::from_millis(100));

        // Summary should reflect the 40x40 image.
        assert_eq!(diag.summary.image_width, 40);
        assert_eq!(diag.summary.image_height, 40);
        assert_eq!(diag.summary.pixel_count, 1600);
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
            downsample: StageDiagnostics {
                duration: Duration::from_millis(0),
                metrics: StageMetrics::Downsample {
                    original_width: 100,
                    original_height: 100,
                    width: 100,
                    height: 100,
                    max_dimension: 256,
                    filter: "Triangle".to_string(),
                    applied: false,
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
                    channel_count: 1,
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
                    quality: None,
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

    #[test]
    fn report_downsample_applied_true() {
        let diag = PipelineDiagnostics {
            decode: StageDiagnostics {
                duration: Duration::from_millis(10),
                metrics: StageMetrics::Decode {
                    input_bytes: 5000,
                    width: 800,
                    height: 600,
                    pixel_count: 480_000,
                },
            },
            downsample: StageDiagnostics {
                duration: Duration::from_millis(8),
                metrics: StageMetrics::Downsample {
                    original_width: 800,
                    original_height: 600,
                    width: 256,
                    height: 192,
                    max_dimension: 256,
                    filter: "Triangle".to_string(),
                    applied: true,
                },
            },
            blur: StageDiagnostics {
                duration: Duration::from_millis(10),
                metrics: StageMetrics::Blur { sigma: 1.4 },
            },
            edge_detection: StageDiagnostics {
                duration: Duration::from_millis(20),
                metrics: StageMetrics::EdgeDetection {
                    low_threshold: 30.0,
                    high_threshold: 80.0,
                    edge_pixel_count: 1200,
                    total_pixel_count: 49152,
                    channel_count: 1,
                },
            },
            invert: None,
            contour_tracing: StageDiagnostics {
                duration: Duration::from_millis(10),
                metrics: StageMetrics::ContourTracing {
                    contour_count: 8,
                    total_point_count: 150,
                    min_contour_points: 3,
                    max_contour_points: 40,
                    mean_contour_points: 18.75,
                },
            },
            simplification: StageDiagnostics {
                duration: Duration::from_millis(4),
                metrics: StageMetrics::Simplification {
                    tolerance: 2.0,
                    polyline_count: 8,
                    points_before: 150,
                    points_after: 80,
                    reduction_ratio: 0.467,
                },
            },
            mask: None,
            join: StageDiagnostics {
                duration: Duration::from_millis(15),
                metrics: StageMetrics::Join {
                    strategy: "Retrace".to_string(),
                    input_polyline_count: 8,
                    input_point_count: 80,
                    output_point_count: 120,
                    expansion_ratio: 1.5,
                    quality: None,
                },
            },
            total_duration: Duration::from_millis(80),
            summary: PipelineSummary {
                image_width: 256,
                image_height: 192,
                pixel_count: 49152,
                contour_count: 8,
                final_point_count: 120,
            },
        };

        let report = diag.report();
        assert!(!report.is_empty());
        // Verify the applied=true formatting path: "AxB -> CxD (target=N, Filter)"
        assert!(
            report.contains("800x600 -> 256x192"),
            "report should contain downsample resize info, got:\n{report}",
        );
        assert!(
            report.contains("Triangle"),
            "report should mention the filter, got:\n{report}",
        );
    }
}
