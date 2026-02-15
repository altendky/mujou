//! Integration test: run the cherry blossoms example image through the full pipeline and export to SVG.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

/// Locate the workspace root by searching upward from the crate directory for `Cargo.lock`.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|dir| dir.join("Cargo.lock").exists())
        .unwrap_or_else(|| {
            panic!(
                "could not find workspace root (no Cargo.lock above {:?})",
                env!("CARGO_MANIFEST_DIR"),
            )
        })
        .to_path_buf()
}

#[test]
fn cherry_blossoms_pipeline_to_svg() {
    let workspace_root = workspace_root();
    let image_path = workspace_root.join("assets/examples/cherry-blossoms.png");
    assert!(
        image_path.exists(),
        "cherry blossoms image not found at {image_path:?}"
    );

    let image_bytes = std::fs::read(&image_path).unwrap();
    eprintln!("Loaded cherry-blossoms.png: {} bytes", image_bytes.len());

    // Run the pipeline with a fast config for integration testing.
    // Uses higher Canny thresholds and StraightLine joiner to keep
    // edge count and join cost low in unoptimized debug builds.
    let config = mujou_pipeline::PipelineConfig {
        canny_low: 50.0,
        canny_high: 150.0,
        path_joiner: mujou_pipeline::PathJoinerKind::StraightLine,
        circular_mask: false,
        ..mujou_pipeline::PipelineConfig::default()
    };
    let result = mujou_pipeline::process(&image_bytes, &config).expect("pipeline should succeed");

    eprintln!(
        "Pipeline produced {} points, image {}x{}",
        result.polyline.len(),
        result.dimensions.width,
        result.dimensions.height,
    );
    assert!(
        !result.polyline.is_empty(),
        "expected non-empty polyline from cherry blossoms image"
    );

    // Export to SVG.
    let svg = mujou_export::to_svg(
        &[result.polyline],
        result.dimensions,
        &mujou_export::SvgMetadata::default(),
    );

    // Basic structural assertions.
    assert!(svg.contains("<svg"));
    assert!(svg.contains("<path"));
    assert!(svg.contains("</svg>"));

    // Write SVG to a temp location so we can inspect it.
    let output_path = workspace_root.join("target/cherry-blossoms-output.svg");
    std::fs::write(&output_path, &svg).unwrap();
    eprintln!("SVG written to {output_path:?} ({} bytes)", svg.len());
}

/// Verify the app's default config (`low=15`, `high=40`, Mst,
/// `circular_mask=true`) can process the cherry blossoms image without OOM.
/// Lower thresholds produce many more edges that can exhaust WASM memory.
#[test]
fn cherry_blossoms_default_config() {
    let workspace_root = workspace_root();
    let image_path = workspace_root.join("assets/examples/cherry-blossoms.png");
    let image_bytes = std::fs::read(&image_path).unwrap();
    eprintln!("Loaded cherry-blossoms.png: {} bytes", image_bytes.len());

    let config = mujou_pipeline::PipelineConfig::default();
    let result = mujou_pipeline::process(&image_bytes, &config)
        .expect("pipeline should succeed with default config on cherry blossoms");
    eprintln!(
        "Pipeline produced {} points, image {}x{}",
        result.polyline.len(),
        result.dimensions.width,
        result.dimensions.height,
    );
    assert!(
        !result.polyline.is_empty(),
        "expected non-empty polyline from cherry blossoms with default config"
    );
}

/// Compare Greedy vs Optimal parity strategies on the cherry blossoms
/// image and report quality metrics for both.
///
/// This test uses `process_staged_with_diagnostics` to capture
/// `JoinQualityMetrics` for both strategies on the same pipeline input.
#[test]
fn cherry_blossoms_parity_strategy_comparison() {
    use std::time::{Duration, Instant};

    struct StdClock;
    impl mujou_pipeline::diagnostics::Clock for StdClock {
        type Instant = Instant;
        fn now(&self) -> Instant {
            Instant::now()
        }
        fn elapsed(&self, since: &Instant) -> Duration {
            since.elapsed()
        }
    }

    let workspace_root = workspace_root();
    let image_path = workspace_root.join("assets/examples/cherry-blossoms.png");
    let image_bytes = std::fs::read(&image_path).unwrap();

    // Use higher Canny thresholds to keep contour count manageable in
    // debug builds while still producing enough contours for meaningful
    // parity comparison.
    let base_config = mujou_pipeline::PipelineConfig {
        canny_low: 50.0,
        canny_high: 150.0,
        path_joiner: mujou_pipeline::PathJoinerKind::Mst,
        circular_mask: false,
        ..mujou_pipeline::PipelineConfig::default()
    };

    let clock = StdClock;

    // Run with Greedy parity.
    let greedy_config = mujou_pipeline::PipelineConfig {
        parity_strategy: mujou_pipeline::ParityStrategy::Greedy,
        ..base_config.clone()
    };
    let (greedy_result, greedy_diag) =
        mujou_pipeline::diagnostics::process_staged_with_diagnostics(
            &image_bytes,
            &greedy_config,
            &clock,
        )
        .expect("greedy pipeline should succeed");

    // Run with Optimal parity.
    let optimal_config = mujou_pipeline::PipelineConfig {
        parity_strategy: mujou_pipeline::ParityStrategy::Optimal,
        ..base_config
    };
    let (optimal_result, optimal_diag) =
        mujou_pipeline::diagnostics::process_staged_with_diagnostics(
            &image_bytes,
            &optimal_config,
            &clock,
        )
        .expect("optimal pipeline should succeed");

    // Both should produce valid non-empty results.
    assert!(!greedy_result.joined.is_empty());
    assert!(!optimal_result.joined.is_empty());

    // Extract join quality metrics from diagnostics.
    let extract_quality =
        |diag: &mujou_pipeline::PipelineDiagnostics| -> mujou_pipeline::JoinQualityMetrics {
            match &diag.join.metrics {
                mujou_pipeline::diagnostics::StageMetrics::Join { quality, .. } => quality
                    .clone()
                    .expect("MST joiner should produce quality metrics"),
                other => panic!("expected Join metrics, got {other:?}"),
            }
        };

    let greedy_metrics = extract_quality(&greedy_diag);
    let optimal_metrics = extract_quality(&optimal_diag);

    // Report metrics for comparison.
    eprintln!("=== Parity Strategy Comparison (cherry blossoms) ===");
    eprintln!("                           Greedy      Optimal");
    eprintln!(
        "  MST edge weight:      {:10.2}  {:10.2}",
        greedy_metrics.total_mst_edge_weight, optimal_metrics.total_mst_edge_weight,
    );
    eprintln!(
        "  Max MST edge:         {:10.2}  {:10.2}",
        greedy_metrics.max_mst_edge_weight, optimal_metrics.max_mst_edge_weight,
    );
    eprintln!(
        "  Retrace distance:     {:10.2}  {:10.2}",
        greedy_metrics.total_retrace_distance, optimal_metrics.total_retrace_distance,
    );
    eprintln!(
        "  Total path length:    {:10.2}  {:10.2}",
        greedy_metrics.total_path_length, optimal_metrics.total_path_length,
    );
    eprintln!(
        "  Odd vertices before:  {:10}  {:10}",
        greedy_metrics.odd_vertices_before_fix, optimal_metrics.odd_vertices_before_fix,
    );
    eprintln!(
        "  Odd vertices after:   {:10}  {:10}",
        greedy_metrics.odd_vertices_after_fix, optimal_metrics.odd_vertices_after_fix,
    );

    // MST edge weight should be identical (same MST, different parity fix).
    assert!(
        (greedy_metrics.total_mst_edge_weight - optimal_metrics.total_mst_edge_weight).abs() < 1e-6,
        "MST weight should be identical across parity strategies",
    );

    // Optimal should not produce worse retrace than greedy.
    assert!(
        optimal_metrics.total_retrace_distance <= greedy_metrics.total_retrace_distance + 1e-6,
        "optimal retrace ({}) should be <= greedy retrace ({})",
        optimal_metrics.total_retrace_distance,
        greedy_metrics.total_retrace_distance,
    );
}
