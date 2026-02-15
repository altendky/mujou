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
use std::time::{Duration, Instant};

use clap::{Parser, ValueEnum};
use mujou_pipeline::diagnostics::{Clock, PipelineDiagnostics};

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
    #[arg(long, value_enum, default_value_t = Joiner::Mst)]
    joiner: Joiner,

    /// Parity-fixing strategy for MST joiner.
    #[arg(long, value_enum, default_value_t = Parity::Greedy)]
    parity_strategy: Parity,

    /// Disable circular mask.
    #[arg(long)]
    no_mask: bool,

    /// Mask diameter as fraction of image diagonal (0.0-1.5).
    #[arg(long, default_value_t = mujou_pipeline::PipelineConfig::DEFAULT_MASK_DIAMETER)]
    mask_diameter: f64,

    /// Invert edge map before contour tracing.
    #[arg(long)]
    invert: bool,

    /// Working resolution (max dimension in pixels after downsampling).
    #[arg(long, default_value_t = mujou_pipeline::PipelineConfig::DEFAULT_WORKING_RESOLUTION, value_parser = clap::builder::RangedU64ValueParser::<u32>::new().range(1..))]
    working_resolution: u32,

    /// Downsample filter (nearest, triangle, catmull-rom, gaussian, lanczos3).
    #[arg(long, value_enum, default_value_t = CLI_DEFAULT_FILTER)]
    downsample_filter: Filter,

    /// Write SVG output to file.
    #[arg(long)]
    svg: Option<PathBuf>,

    /// Number of runs for averaging.
    #[arg(long, default_value_t = 1, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..))]
    runs: usize,

    /// Output diagnostics as JSON instead of human-readable report.
    #[arg(long)]
    json: bool,

    /// Full pipeline config as a JSON string.
    ///
    /// When provided, all other pipeline parameter flags are ignored.
    /// The JSON must be a valid `PipelineConfig` serialization.
    #[arg(long)]
    config_json: Option<String>,
}

/// Path joining strategy selection.
#[derive(Clone, Copy, ValueEnum)]
enum Joiner {
    /// Nearest-neighbor ordering + straight-line concatenation.
    Straight,
    /// Full-history retrace with integrated ordering.
    Retrace,
    /// MST-based segment-to-segment join with Eulerian path.
    Mst,
}

/// Parity-fixing strategy selection.
#[derive(Clone, Copy, ValueEnum)]
enum Parity {
    /// Greedy nearest-neighbor pairing by Euclidean distance.
    Greedy,
    /// Optimal matching by graph distance (DP for small n, greedy fallback).
    Optimal,
}

/// Downsample resampling filter selection.
#[derive(Clone, Copy, ValueEnum)]
enum Filter {
    /// Disabled: skip downsampling regardless of image size.
    Disabled,
    /// Nearest-neighbor (fastest, blocky).
    Nearest,
    /// Bilinear interpolation (fast, decent quality).
    Triangle,
    /// Bicubic Catmull-Rom (moderate, good quality).
    CatmullRom,
    /// Gaussian (moderate, smooth).
    Gaussian,
    /// Lanczos with 3 lobes (slowest, sharpest).
    Lanczos3,
}

/// Maps a [`mujou_pipeline::DownsampleFilter`] to the local CLI [`Filter`] enum.
const fn filter_from_pipeline(f: mujou_pipeline::DownsampleFilter) -> Filter {
    match f {
        mujou_pipeline::DownsampleFilter::Disabled => Filter::Disabled,
        mujou_pipeline::DownsampleFilter::Nearest => Filter::Nearest,
        mujou_pipeline::DownsampleFilter::Triangle => Filter::Triangle,
        mujou_pipeline::DownsampleFilter::CatmullRom => Filter::CatmullRom,
        mujou_pipeline::DownsampleFilter::Gaussian => Filter::Gaussian,
        mujou_pipeline::DownsampleFilter::Lanczos3 => Filter::Lanczos3,
    }
}

/// The CLI default filter, derived from [`PipelineConfig::DEFAULT_DOWNSAMPLE_FILTER`]
/// so the two cannot silently diverge.
const CLI_DEFAULT_FILTER: Filter =
    filter_from_pipeline(mujou_pipeline::PipelineConfig::DEFAULT_DOWNSAMPLE_FILTER);

/// Build a [`PipelineConfig`](mujou_pipeline::PipelineConfig) from CLI arguments.
///
/// If `--config-json` is provided, the JSON is parsed directly and all
/// individual parameter flags are ignored.  Otherwise, a config is
/// assembled from the individual flags.
fn config_from_cli(cli: &Cli) -> Result<mujou_pipeline::PipelineConfig, String> {
    if let Some(ref json) = cli.config_json {
        return serde_json::from_str(json).map_err(|e| format!("Error parsing --config-json: {e}"));
    }

    Ok(mujou_pipeline::PipelineConfig {
        blur_sigma: cli.blur_sigma,
        canny_low: cli.canny_low,
        canny_high: cli.canny_high,
        simplify_tolerance: cli.simplify_tolerance,
        path_joiner: match cli.joiner {
            Joiner::Straight => mujou_pipeline::PathJoinerKind::StraightLine,
            Joiner::Retrace => mujou_pipeline::PathJoinerKind::Retrace,
            Joiner::Mst => mujou_pipeline::PathJoinerKind::Mst,
        },
        parity_strategy: match cli.parity_strategy {
            Parity::Greedy => mujou_pipeline::ParityStrategy::Greedy,
            Parity::Optimal => mujou_pipeline::ParityStrategy::Optimal,
        },
        circular_mask: !cli.no_mask,
        mask_diameter: cli.mask_diameter,
        invert: cli.invert,
        working_resolution: cli.working_resolution,
        downsample_filter: match cli.downsample_filter {
            Filter::Disabled => mujou_pipeline::DownsampleFilter::Disabled,
            Filter::Nearest => mujou_pipeline::DownsampleFilter::Nearest,
            Filter::Triangle => mujou_pipeline::DownsampleFilter::Triangle,
            Filter::CatmullRom => mujou_pipeline::DownsampleFilter::CatmullRom,
            Filter::Gaussian => mujou_pipeline::DownsampleFilter::Gaussian,
            Filter::Lanczos3 => mujou_pipeline::DownsampleFilter::Lanczos3,
        },
        ..mujou_pipeline::PipelineConfig::default()
    })
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let config = match config_from_cli(&cli) {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::FAILURE;
        }
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

        match mujou_pipeline::diagnostics::process_staged_with_diagnostics(
            &image_bytes,
            &config,
            &StdClock,
        ) {
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
                    let title = cli
                        .image_path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("bench");
                    let desc = format!("{config:#?}");
                    let metadata = mujou_export::SvgMetadata {
                        title: Some(title),
                        description: Some(&desc),
                    };
                    let svg = mujou_export::to_svg(
                        &[staged.final_polyline().clone()],
                        staged.dimensions,
                        &metadata,
                    );
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

/// [`Clock`] implementation backed by [`std::time::Instant`].
struct StdClock;

impl Clock for StdClock {
    type Instant = Instant;

    fn now(&self) -> Instant {
        Instant::now()
    }

    fn elapsed(&self, since: &Instant) -> Duration {
        since.elapsed()
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
        ("Downsample", |d| Some(d.downsample.duration)),
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
