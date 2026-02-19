//! Integration test: run the cherry blossoms example image through the full pipeline and export to SVG.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Shared `Clock` implementation for test functions that need
/// `process_staged_with_diagnostics`.
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
        mask_mode: mujou_pipeline::MaskMode::Off,
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

    // Export to SVG (no mask — mask_mode=Off in this test config).
    let svg = mujou_export::to_svg(
        &[result.polyline],
        result.dimensions,
        &mujou_export::SvgMetadata::default(),
        None,
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
/// `mask_mode=Circle`) can process the cherry blossoms image without OOM.
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
        mask_mode: mujou_pipeline::MaskMode::Off,
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
    //
    // When n > DP_THRESHOLD the "Optimal" strategy is a heuristic
    // (better of Euclidean-greedy and graph-distance-greedy), not true
    // optimal.  Different matchings can lead to different Euler paths
    // with different retrace characteristics, so this invariant is not
    // guaranteed in general.  For the cherry-blossoms image it holds
    // today; log a warning instead of hard-failing so future images or
    // parameter changes don't cause a flaky test.
    if optimal_metrics.total_retrace_distance > greedy_metrics.total_retrace_distance + 1e-6 {
        eprintln!(
            "WARNING: optimal retrace ({}) > greedy retrace ({}) — \
             this can happen when n > DP_THRESHOLD and the heuristic \
             fallback produces a slightly worse Euler traversal",
            optimal_metrics.total_retrace_distance, greedy_metrics.total_retrace_distance,
        );
    }
}

/// Diagnostic test: run the cherry blossoms image with **default config**
/// (circular mask, MST joiner, default Canny thresholds) and report
/// per-MST-edge details to help identify the long diagonal connecting
/// line visible in the output.
///
/// Outputs:
/// - A table of all MST edges sorted by weight (longest first)
/// - Border-routing analysis for each edge
/// - A diagnostic SVG to `target/cherry-blossoms-diagnostic.svg` with
///   MST edges highlighted in red
#[test]
#[allow(clippy::too_many_lines)]
fn cherry_blossoms_mst_edge_diagnostics() {
    let workspace_root = workspace_root();
    let image_path = workspace_root.join("assets/examples/cherry-blossoms.png");
    let image_bytes = std::fs::read(&image_path).unwrap();
    eprintln!("Loaded cherry-blossoms.png: {} bytes", image_bytes.len());

    // Match the exact config used to produce the image with the visible
    // long diagonal: mask_scale=0.6, mst_neighbours=200, Optimal parity.
    let config = mujou_pipeline::PipelineConfig {
        mask_scale: 0.6,
        mst_neighbours: 200,
        parity_strategy: mujou_pipeline::ParityStrategy::Optimal,
        ..mujou_pipeline::PipelineConfig::default()
    };
    let clock = StdClock;

    let (result, diag) =
        mujou_pipeline::diagnostics::process_staged_with_diagnostics(&image_bytes, &config, &clock)
            .expect("pipeline should succeed with default config");

    // Print full diagnostics report.
    eprintln!("\n{}", diag.report());

    // Extract join quality metrics.
    let quality = match &diag.join.metrics {
        mujou_pipeline::diagnostics::StageMetrics::Join { quality, .. } => quality
            .clone()
            .expect("MST joiner should produce quality metrics"),
        other => panic!("expected Join metrics, got {other:?}"),
    };

    // Compute mask geometry for border-routing analysis.
    let w = f64::from(result.dimensions.width);
    let h = f64::from(result.dimensions.height);
    let center_x = w / 2.0;
    let center_y = h / 2.0;
    let diagonal = w.hypot(h);
    let radius = diagonal * config.mask_scale / 2.0;

    eprintln!("\n=== MST Edge Diagnostics ===");
    eprintln!(
        "Mask: center=({:.1}, {:.1}) radius={:.1}px mask_scale={:.2}",
        center_x, center_y, radius, config.mask_scale,
    );
    eprintln!("Total MST edges: {}", quality.mst_edge_details.len());
    eprintln!();

    // Sort edges by weight descending for display.
    let mut sorted_edges: Vec<(usize, &mujou_pipeline::MstEdgeInfo)> =
        quality.mst_edge_details.iter().enumerate().collect();
    sorted_edges.sort_by(|a, b| {
        b.1.weight
            .partial_cmp(&a.1.weight)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    eprintln!(
        "{:>4}  {:>8}  {:>6} {:>6}  {:>20}  {:>20}  {:>8} {:>8}  {:>10}  BdrBetter?",
        "Rank", "Weight", "PolyA", "PolyB", "PointA", "PointB", "d_bdr_A", "d_bdr_B", "bdr_sum",
    );
    eprintln!("{}", "-".repeat(120));

    for (rank, (orig_idx, edge)) in sorted_edges.iter().enumerate() {
        // Distance from each connection point to the border circle.
        let dist_to_center_a = (edge.point_a.0 - center_x).hypot(edge.point_a.1 - center_y);
        let dist_to_border_a = (dist_to_center_a - radius).abs();

        let dist_to_center_b = (edge.point_b.0 - center_x).hypot(edge.point_b.1 - center_y);
        let dist_to_border_b = (dist_to_center_b - radius).abs();

        let border_sum = dist_to_border_a + dist_to_border_b;
        let border_better = border_sum < edge.weight;

        eprintln!(
            "{:>4}  {:>8.2}  {:>6} {:>6}  ({:>8.1}, {:>8.1})  ({:>8.1}, {:>8.1})  {:>8.1} {:>8.1}  {:>10.1}  {}",
            rank + 1,
            edge.weight,
            edge.poly_a,
            edge.poly_b,
            edge.point_a.0,
            edge.point_a.1,
            edge.point_b.0,
            edge.point_b.1,
            dist_to_border_a,
            dist_to_border_b,
            border_sum,
            if border_better { "YES" } else { "no" },
        );

        // For the top 5 longest edges, print extra detail.
        if rank < 5 {
            eprintln!(
                "       orig_idx={} seg_a={} seg_b={} dist_center_a={:.1} dist_center_b={:.1}",
                orig_idx, edge.seg_a, edge.seg_b, dist_to_center_a, dist_to_center_b,
            );
        }
    }

    eprintln!();
    eprintln!(
        "Summary: {} edges where border-routing would be shorter",
        sorted_edges
            .iter()
            .filter(|(_, e)| {
                let da = ((e.point_a.0 - center_x).hypot(e.point_a.1 - center_y) - radius).abs();
                let db = ((e.point_b.0 - center_x).hypot(e.point_b.1 - center_y) - radius).abs();
                da + db < e.weight
            })
            .count(),
    );

    // === Longest segments in the JOIN INPUT polylines ===
    // Scan the polylines that were fed into the joiner for long segments.
    // This identifies contour segments that survived simplification + clipping.
    {
        let join_input: Vec<&mujou_pipeline::Polyline> = result
            .canvas
            .as_ref()
            .expect("mask should be enabled")
            .all_polylines()
            .collect();

        eprintln!("\n=== Longest Segments in Join Input Polylines ===");
        eprintln!("Join input: {} polylines", join_input.len());

        let mut input_segments: Vec<(
            usize,
            usize,
            f64,
            &mujou_pipeline::Point,
            &mujou_pipeline::Point,
        )> = Vec::new();
        for (pi, poly) in join_input.iter().enumerate() {
            let pts = poly.points();
            for si in 0..pts.len().saturating_sub(1) {
                let d = pts[si].distance(pts[si + 1]);
                if d > 20.0 {
                    input_segments.push((pi, si, d, &pts[si], &pts[si + 1]));
                }
            }
        }
        input_segments.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        eprintln!(
            "{:>4}  {:>5}  {:>3}  {:>8}  {:>20}  {:>20}",
            "Rank", "Poly", "Seg", "Length", "From", "To",
        );
        eprintln!("{}", "-".repeat(80));
        for (rank, &(pi, si, len, from, to)) in input_segments.iter().take(20).enumerate() {
            eprintln!(
                "{:>4}  {:>5}  {:>3}  {:>8.2}  ({:>8.1}, {:>8.1})  ({:>8.1}, {:>8.1})",
                rank + 1,
                pi,
                si,
                len,
                from.x,
                from.y,
                to.x,
                to.y,
            );
        }
        eprintln!();
    }

    // === Longest segments in the joined polyline ===
    // This identifies ANY long straight-line segments in the output,
    // regardless of origin (MST edge, contour, retrace, border).
    eprintln!("\n=== Longest Segments in Joined Polyline ===");
    eprintln!("Total points in joined path: {}", result.joined.len());

    let points = result.joined.points();
    let mut segments: Vec<(usize, f64, &mujou_pipeline::Point, &mujou_pipeline::Point)> = points
        .windows(2)
        .enumerate()
        .map(|(i, w)| (i, w[0].distance(w[1]), &w[0], &w[1]))
        .collect();
    segments.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    eprintln!(
        "{:>4}  {:>8}  {:>20}  {:>20}  {:>6}  {:>6}",
        "Rank", "Length", "From", "To", "dx", "dy",
    );
    eprintln!("{}", "-".repeat(80));

    // Check each long segment against MST edges to classify it.
    // Quantize coordinates to 0.1px grid for set-based lookup.
    #[allow(clippy::cast_possible_truncation)]
    let mst_set: std::collections::HashSet<((i64, i64), (i64, i64))> = quality
        .mst_edge_details
        .iter()
        .flat_map(|e| {
            let a = (
                (e.point_a.0 * 10.0).round() as i64,
                (e.point_a.1 * 10.0).round() as i64,
            );
            let b = (
                (e.point_b.0 * 10.0).round() as i64,
                (e.point_b.1 * 10.0).round() as i64,
            );
            // Both directions
            vec![(a, b), (b, a)]
        })
        .collect();

    #[allow(clippy::cast_possible_truncation)]
    for (rank, &(idx, len, from, to)) in segments.iter().take(30).enumerate() {
        let dx = to.x - from.x;
        let dy = to.y - from.y;

        // Classify: is this an MST edge?
        let from_key = (
            (from.x * 10.0).round() as i64,
            (from.y * 10.0).round() as i64,
        );
        let to_key = ((to.x * 10.0).round() as i64, (to.y * 10.0).round() as i64);
        let is_mst = mst_set.contains(&(from_key, to_key));

        // Is either endpoint on the border circle?
        let from_r = ((from.x - center_x).hypot(from.y - center_y) - radius).abs();
        let to_r = ((to.x - center_x).hypot(to.y - center_y) - radius).abs();
        let on_border = from_r < 2.0 || to_r < 2.0;

        let classification = if is_mst {
            "MST"
        } else if on_border {
            "BORDER"
        } else {
            "contour/retrace"
        };

        eprintln!(
            "{:>4}  {:>8.2}  ({:>8.1}, {:>8.1})  ({:>8.1}, {:>8.1})  {:>6.1}  {:>6.1}  idx={} {}",
            rank + 1,
            len,
            from.x,
            from.y,
            to.x,
            to.y,
            dx,
            dy,
            idx,
            classification,
        );
    }

    // Write diagnostic SVG with highlighted MST edges.
    let diagnostic_svg = mujou_export::to_diagnostic_svg(
        std::slice::from_ref(&result.joined),
        result.dimensions,
        &mujou_export::SvgMetadata {
            title: Some("cherry-blossoms (MST edge diagnostics)"),
            description: Some("Red lines = MST connecting edges"),
            config_json: None,
        },
        &quality.mst_edge_details,
    );

    let output_path = workspace_root.join("target/cherry-blossoms-diagnostic.svg");
    std::fs::write(&output_path, &diagnostic_svg).unwrap();
    eprintln!(
        "\nDiagnostic SVG written to {output_path:?} ({} bytes)",
        diagnostic_svg.len(),
    );
}

/// Run the cherry blossoms image through the pipeline and export to THR.
///
/// Validates header, rho range [0, 1], continuous theta, and 5-decimal
/// precision — the same properties checked by the unit tests, but
/// exercised on a real photograph.
#[test]
fn cherry_blossoms_pipeline_to_thr() {
    let workspace_root = workspace_root();
    let image_path = workspace_root.join("assets/examples/cherry-blossoms.png");
    assert!(
        image_path.exists(),
        "cherry blossoms image not found at {image_path:?}"
    );

    let image_bytes = std::fs::read(&image_path).unwrap();
    eprintln!("Loaded cherry-blossoms.png: {} bytes", image_bytes.len());

    // Use same fast config as the SVG integration test.
    let config = mujou_pipeline::PipelineConfig {
        canny_low: 50.0,
        canny_high: 150.0,
        path_joiner: mujou_pipeline::PathJoinerKind::StraightLine,
        mask_mode: mujou_pipeline::MaskMode::Off,
        ..mujou_pipeline::PipelineConfig::default()
    };
    let result =
        mujou_pipeline::process_staged(&image_bytes, &config).expect("pipeline should succeed");

    let mask_shape = result.canvas.as_ref().map(|mr| &mr.shape);
    let thr = mujou_export::to_thr(
        std::slice::from_ref(result.final_polyline()),
        result.dimensions,
        &mujou_export::ThrMetadata {
            title: Some("cherry-blossoms.png"),
            description: Some("integration test"),
            ..mujou_export::ThrMetadata::default()
        },
        mask_shape,
    );

    // Header assertions.
    assert!(thr.starts_with("# mujou\n"));
    assert!(thr.contains("# Source: cherry-blossoms.png\n"));

    // Parse data lines.
    let pairs: Vec<(f64, f64)> = thr
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let mut parts = line.split_whitespace();
            let theta: f64 = parts.next().unwrap().parse().unwrap();
            let rho: f64 = parts.next().unwrap().parse().unwrap();
            (theta, rho)
        })
        .collect();

    eprintln!(
        "THR output: {} theta-rho pairs, {} bytes",
        pairs.len(),
        thr.len(),
    );
    assert!(
        pairs.len() >= 10,
        "expected at least 10 theta-rho pairs from cherry blossoms, got {}",
        pairs.len(),
    );

    // All rho values in [0, 1].
    for (i, &(_, rho)) in pairs.iter().enumerate() {
        assert!(
            (0.0..=1.0001).contains(&rho),
            "rho[{i}] = {rho} is outside [0, 1]",
        );
    }

    // Precision: every data line should have 5 decimal places.
    for line in thr
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        for part in line.split_whitespace() {
            let dot_pos = part.find('.').expect("should have decimal point");
            let decimals = &part[dot_pos + 1..];
            assert_eq!(decimals.len(), 5, "expected 5 decimal places in {part}",);
        }
    }

    // Write THR to target for manual inspection.
    let output_path = workspace_root.join("target/cherry-blossoms-output.thr");
    std::fs::write(&output_path, &thr).unwrap();
    eprintln!("THR written to {output_path:?} ({} bytes)", thr.len());
}

/// Run the cherry blossoms image through the pipeline with a circular
/// mask and export to THR, validating that rho stays within [0, 1]
/// even when the mask clips contours at the edge.
#[test]
fn cherry_blossoms_pipeline_to_thr_with_mask() {
    let workspace_root = workspace_root();
    let image_path = workspace_root.join("assets/examples/cherry-blossoms.png");
    let image_bytes = std::fs::read(&image_path).unwrap();

    let config = mujou_pipeline::PipelineConfig::default();
    let result =
        mujou_pipeline::process_staged(&image_bytes, &config).expect("pipeline should succeed");

    let mask_shape = result.canvas.as_ref().map(|mr| &mr.shape);
    let thr = mujou_export::to_thr(
        std::slice::from_ref(result.final_polyline()),
        result.dimensions,
        &mujou_export::ThrMetadata::default(),
        mask_shape,
    );

    let pairs: Vec<(f64, f64)> = thr
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let mut parts = line.split_whitespace();
            let theta: f64 = parts.next().unwrap().parse().unwrap();
            let rho: f64 = parts.next().unwrap().parse().unwrap();
            (theta, rho)
        })
        .collect();

    eprintln!(
        "THR with mask: {} pairs, mask_shape={:?}",
        pairs.len(),
        mask_shape,
    );
    assert!(!pairs.is_empty(), "expected non-empty THR output with mask");

    // All rho values should be in [0, 1] (mask guarantees points
    // are within the circle).
    for (i, &(_, rho)) in pairs.iter().enumerate() {
        assert!(
            (0.0..=1.0001).contains(&rho),
            "rho[{i}] = {rho} is outside [0, 1] with circular mask",
        );
    }
}

/// Generate a diagnostic SVG highlighting the top 5 longest segments
/// in the joined output using the **default** pipeline config.
///
/// This uses `to_segment_diagnostic_svg()` to produce a visual with
/// color-coded long segments and a legend, making it easy to identify
/// unexpectedly long lines (MST edges, retrace artifacts, or bugs).
///
/// Output: `target/cherry-blossoms-segments.svg`
#[test]
fn cherry_blossoms_segment_diagnostics() {
    let workspace_root = workspace_root();
    let image_path = workspace_root.join("assets/examples/cherry-blossoms.png");
    let image_bytes = std::fs::read(&image_path).unwrap();
    eprintln!("Loaded cherry-blossoms.png: {} bytes", image_bytes.len());

    // Use the default config — this is the config where the user
    // noticed odd lines in the output.
    let config = mujou_pipeline::PipelineConfig::default();
    let clock = StdClock;

    let (result, diag) =
        mujou_pipeline::diagnostics::process_staged_with_diagnostics(&image_bytes, &config, &clock)
            .expect("pipeline should succeed");

    // Print full diagnostics report (includes odd vertex counts).
    eprintln!("\n{}", diag.report());

    let joined = &result.joined;
    let points = joined.points();
    eprintln!(
        "Joined path: {} points, image {}x{}",
        points.len(),
        result.dimensions.width,
        result.dimensions.height,
    );

    // Report the top 10 longest segments to stderr.
    let mut segments: Vec<(usize, f64, &mujou_pipeline::Point, &mujou_pipeline::Point)> = points
        .windows(2)
        .enumerate()
        .map(|(i, w)| (i, w[0].distance(w[1]), &w[0], &w[1]))
        .collect();
    segments.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    eprintln!("\n=== Top 10 Longest Segments (default config) ===");
    eprintln!(
        "{:>4}  {:>8}  {:>20}  {:>20}  {:>6}",
        "Rank", "Length", "From", "To", "Index",
    );
    eprintln!("{}", "-".repeat(70));
    for (rank, &(idx, len, from, to)) in segments.iter().take(10).enumerate() {
        eprintln!(
            "{:>4}  {:>8.2}  ({:>8.1}, {:>8.1})  ({:>8.1}, {:>8.1})  {:>6}",
            rank + 1,
            len,
            from.x,
            from.y,
            to.x,
            to.y,
            idx,
        );
    }

    // Generate the segment diagnostic SVG.
    let svg = mujou_export::to_segment_diagnostic_svg(
        std::slice::from_ref(joined),
        result.dimensions,
        &mujou_export::SvgMetadata {
            title: Some("cherry-blossoms (segment diagnostics)"),
            description: Some("Top 5 longest segments highlighted in distinct colors"),
            config_json: None,
        },
        5,
    );

    let output_path = workspace_root.join("target/cherry-blossoms-segments.svg");
    std::fs::write(&output_path, &svg).unwrap();
    eprintln!(
        "\nSegment diagnostic SVG written to {output_path:?} ({} bytes)",
        svg.len(),
    );
}
