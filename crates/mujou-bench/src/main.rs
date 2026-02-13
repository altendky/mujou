//! mujou-bench: CLI tool for pipeline parameter experimentation and diagnostics.
//!
//! Runs the image processing pipeline on a given image file with configurable
//! parameters, printing detailed per-stage diagnostics. Useful for:
//!
//! - Comparing algorithm strategies (`StraightLine` vs `Retrace`)
//! - Tuning Canny thresholds, blur sigma, simplification tolerance
//! - Measuring per-stage durations to identify bottlenecks
//! - Understanding how parameter changes affect contour/point counts
//!
//! # Usage
//!
//! ```text
//! cargo run --release --bin mujou-bench -- [OPTIONS] <IMAGE_PATH>
//! ```

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use clap::{Parser, ValueEnum};
use mujou_pipeline::diagnostics::{PipelineDiagnostics, PipelineSummary, StageDiagnostics};
use mujou_pipeline::pipeline::{
    Advance, Blurred, ContoursTraced, Decoded, EdgesDetected, Grayscaled, Joined, Masked,
    PipelineStage as _, STAGE_COUNT, Simplified, Stage,
};

/// Pipeline parameter experimentation and diagnostics for mujou.
///
/// Runs the image processing pipeline on a given image with configurable
/// parameters and prints detailed per-stage timing and count diagnostics.
#[derive(Parser)]
#[command(name = "mujou-bench", version)]
struct Cli {
    /// Path to the input image (PNG, JPEG, BMP, WebP).
    image_path: PathBuf,

    /// Gaussian blur sigma.
    #[arg(long, default_value_t = mujou_pipeline::PipelineConfig::DEFAULT_BLUR_SIGMA)]
    blur_sigma: f32,

    /// Canny low threshold.
    #[arg(long, default_value_t = mujou_pipeline::PipelineConfig::DEFAULT_CANNY_LOW)]
    canny_low: f32,

    /// Canny high threshold.
    #[arg(long, default_value_t = mujou_pipeline::PipelineConfig::DEFAULT_CANNY_HIGH)]
    canny_high: f32,

    /// RDP simplification tolerance in pixels.
    #[arg(long, default_value_t = mujou_pipeline::PipelineConfig::DEFAULT_SIMPLIFY_TOLERANCE)]
    simplify_tolerance: f64,

    /// Path joining strategy.
    #[arg(long, value_enum, default_value_t = Joiner::Retrace)]
    joiner: Joiner,

    /// Disable circular mask.
    #[arg(long)]
    no_mask: bool,

    /// Mask diameter as fraction of image extent (0.0-1.0).
    #[arg(long, default_value_t = mujou_pipeline::PipelineConfig::DEFAULT_MASK_DIAMETER)]
    mask_diameter: f64,

    /// Invert edge map before contour tracing.
    #[arg(long)]
    invert: bool,

    /// Write SVG output to file.
    #[arg(long)]
    svg: Option<PathBuf>,

    /// Number of runs for averaging.
    #[arg(long, default_value_t = 1, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..))]
    runs: usize,

    /// Output diagnostics as JSON instead of human-readable report.
    #[arg(long)]
    json: bool,
}

/// Path joining strategy selection.
#[derive(Clone, Copy, ValueEnum)]
enum Joiner {
    /// Nearest-neighbor ordering + straight-line concatenation.
    Straight,
    /// Full-history retrace with integrated ordering.
    Retrace,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let config = mujou_pipeline::PipelineConfig {
        blur_sigma: cli.blur_sigma,
        canny_low: cli.canny_low,
        canny_high: cli.canny_high,
        simplify_tolerance: cli.simplify_tolerance,
        path_joiner: match cli.joiner {
            Joiner::Straight => mujou_pipeline::PathJoinerKind::StraightLine,
            Joiner::Retrace => mujou_pipeline::PathJoinerKind::Retrace,
        },
        circular_mask: !cli.no_mask,
        mask_diameter: cli.mask_diameter,
        invert: cli.invert,
        ..mujou_pipeline::PipelineConfig::default()
    };

    let image_bytes = match std::fs::read(&cli.image_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("Error reading {}: {e}", cli.image_path.display());
            return ExitCode::FAILURE;
        }
    };

    eprintln!(
        "Image: {} ({} bytes)",
        cli.image_path.display(),
        image_bytes.len(),
    );
    eprintln!("Config: {config:#?}");
    eprintln!("Runs: {}", cli.runs);
    eprintln!();

    let mut all_diagnostics = Vec::with_capacity(cli.runs);

    for run in 0..cli.runs {
        if cli.runs > 1 {
            eprintln!("--- Run {}/{} ---", run + 1, cli.runs);
        }

        match run_with_diagnostics(&image_bytes, &config) {
            Ok((staged, diagnostics)) => {
                if cli.json {
                    match serde_json::to_string_pretty(&diagnostics) {
                        Ok(json) => println!("{json}"),
                        Err(e) => {
                            eprintln!("Error serializing diagnostics: {e}");
                            return ExitCode::FAILURE;
                        }
                    }
                } else {
                    println!("{}", diagnostics.report());
                }

                // Write SVG on the first run only.
                if run == 0
                    && let Some(ref svg_path) = cli.svg
                {
                    let svg =
                        mujou_export::to_svg(&[staged.final_polyline().clone()], staged.dimensions);
                    match std::fs::write(svg_path, &svg) {
                        Ok(()) => {
                            eprintln!(
                                "SVG written to {} ({} bytes)",
                                svg_path.display(),
                                svg.len(),
                            );
                        }
                        Err(e) => {
                            eprintln!("Error writing SVG to {}: {e}", svg_path.display());
                        }
                    }
                }

                all_diagnostics.push(diagnostics);
            }
            Err(e) => {
                eprintln!("Pipeline error: {e}");
                return ExitCode::FAILURE;
            }
        }

        if cli.runs > 1 {
            eprintln!();
        }
    }

    // Print summary when multiple runs.
    if cli.runs > 1 {
        print_multi_run_summary(&all_diagnostics);
    }

    ExitCode::SUCCESS
}

/// Run the full pipeline with per-stage timing instrumentation.
///
/// Each [`Stage::advance()`] call is timed with [`std::time::Instant`],
/// and the elapsed duration is paired with the stage's own
/// [`metrics()`](PipelineStage::metrics) to build a
/// [`PipelineDiagnostics`].
fn run_with_diagnostics(
    image_bytes: &[u8],
    config: &mujou_pipeline::PipelineConfig,
) -> Result<(mujou_pipeline::StagedResult, PipelineDiagnostics), mujou_pipeline::PipelineError> {
    let pipeline_start = Instant::now();
    let mut stage: Stage =
        mujou_pipeline::Pipeline::new(image_bytes.to_vec(), config.clone()).into();

    let mut stage_diags: [Option<StageDiagnostics>; STAGE_COUNT] = std::array::from_fn(|_| None);
    let mut invert_diag = None;

    loop {
        let t = Instant::now();
        match stage.advance()? {
            Advance::Next(next) => {
                let elapsed = t.elapsed();
                if let Some(metrics) = next.metrics() {
                    stage_diags[next.index()] = Some(StageDiagnostics {
                        duration: elapsed,
                        metrics,
                    });
                }
                if let Some(inv) = next.invert_metrics() {
                    invert_diag = Some(StageDiagnostics {
                        duration: std::time::Duration::ZERO,
                        metrics: inv,
                    });
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

                let diag_missing = |name: &str| {
                    mujou_pipeline::PipelineError::InvalidConfig(format!(
                        "diagnostics bug: {name} diagnostics missing"
                    ))
                };
                let pipeline_diagnostics = PipelineDiagnostics {
                    decode: stage_diags[Decoded::INDEX]
                        .take()
                        .ok_or_else(|| diag_missing(Decoded::NAME))?,
                    grayscale: stage_diags[Grayscaled::INDEX]
                        .take()
                        .ok_or_else(|| diag_missing(Grayscaled::NAME))?,
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

/// Function pointer type for extracting a stage duration from diagnostics.
type StageExtractor = fn(&PipelineDiagnostics) -> Option<std::time::Duration>;

/// Print aggregated statistics across multiple runs.
#[allow(clippy::cast_precision_loss)]
fn print_multi_run_summary(all_diagnostics: &[PipelineDiagnostics]) {
    debug_assert!(!all_diagnostics.is_empty(), "no diagnostics to summarize");

    println!();
    println!(
        "Summary ({} runs)\n{}",
        all_diagnostics.len(),
        "=".repeat(60),
    );

    if all_diagnostics.is_empty() {
        println!("Warning: no diagnostics to summarize");
        return;
    }

    let durations: Vec<f64> = all_diagnostics
        .iter()
        .map(|d| d.total_duration.as_secs_f64() * 1000.0)
        .collect();

    let min = durations.iter().copied().reduce(f64::min).unwrap_or(0.0);
    let max = durations.iter().copied().reduce(f64::max).unwrap_or(0.0);
    let mean = durations.iter().sum::<f64>() / durations.len() as f64;

    println!("Total duration: min={min:.3}ms  mean={mean:.3}ms  max={max:.3}ms");

    // Per-stage means.
    println!();
    println!("{:<24} {:>12}", "Stage", "Mean (ms)");
    println!("{}", "-".repeat(40));

    let stage_extractors: &[(&str, StageExtractor)] = &[
        ("Decode", |d| Some(d.decode.duration)),
        ("Grayscale", |d| Some(d.grayscale.duration)),
        ("Blur", |d| Some(d.blur.duration)),
        ("Edge Detection", |d| Some(d.edge_detection.duration)),
        ("Invert", |d| d.invert.as_ref().map(|s| s.duration)),
        ("Contour Tracing", |d| Some(d.contour_tracing.duration)),
        ("Simplification", |d| Some(d.simplification.duration)),
        ("Mask", |d| d.mask.as_ref().map(|s| s.duration)),
        ("Join", |d| Some(d.join.duration)),
    ];

    for (name, extractor) in stage_extractors {
        let stage_durations: Vec<f64> = all_diagnostics
            .iter()
            .filter_map(extractor)
            .map(|dur| dur.as_secs_f64() * 1000.0)
            .collect();

        if stage_durations.is_empty() {
            continue;
        }

        let stage_mean = stage_durations.iter().sum::<f64>() / stage_durations.len() as f64;
        println!("{name:<24} {stage_mean:>10.3}ms");
    }
}
