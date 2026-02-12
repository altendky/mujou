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

use clap::{Parser, ValueEnum};

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
    #[arg(long, default_value_t = 1.4)]
    blur_sigma: f32,

    /// Canny low threshold.
    #[arg(long, default_value_t = 30.0)]
    canny_low: f32,

    /// Canny high threshold.
    #[arg(long, default_value_t = 80.0)]
    canny_high: f32,

    /// RDP simplification tolerance in pixels.
    #[arg(long, default_value_t = 2.0)]
    simplify_tolerance: f64,

    /// Path joining strategy.
    #[arg(long, value_enum, default_value_t = Joiner::Retrace)]
    joiner: Joiner,

    /// Disable circular mask.
    #[arg(long)]
    no_mask: bool,

    /// Mask diameter as fraction of image extent (0.0-1.0).
    #[arg(long, default_value_t = 1.0)]
    mask_diameter: f64,

    /// Invert edge map before contour tracing.
    #[arg(long)]
    invert: bool,

    /// Write SVG output to file.
    #[arg(long)]
    svg: Option<PathBuf>,

    /// Number of runs for averaging.
    #[arg(long, default_value_t = 1)]
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

    if cli.runs == 0 {
        eprintln!("Error: --runs must be at least 1");
        return ExitCode::FAILURE;
    }

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

        match mujou_pipeline::process_staged_with_diagnostics(&image_bytes, &config) {
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

/// Function pointer type for extracting a stage duration from diagnostics.
type StageExtractor = fn(&mujou_pipeline::PipelineDiagnostics) -> Option<std::time::Duration>;

/// Print aggregated statistics across multiple runs.
#[allow(clippy::cast_precision_loss)]
fn print_multi_run_summary(all_diagnostics: &[mujou_pipeline::PipelineDiagnostics]) {
    println!();
    println!(
        "Summary ({} runs)\n{}",
        all_diagnostics.len(),
        "=".repeat(60),
    );

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
