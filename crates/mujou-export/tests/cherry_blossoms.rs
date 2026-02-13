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
    let svg = mujou_export::to_svg(&[result.polyline], result.dimensions);

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
