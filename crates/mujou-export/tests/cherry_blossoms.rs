//! Integration test: run the cherry blossoms example image through the full pipeline and export to SVG.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

#[test]
fn cherry_blossoms_pipeline_to_svg() {
    // Locate the example image relative to the workspace root.
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let image_path = workspace_root.join("assets/examples/cherry-blossoms.png");
    assert!(
        image_path.exists(),
        "cherry blossoms image not found at {image_path:?}"
    );

    let image_bytes = std::fs::read(&image_path).unwrap();
    eprintln!("Loaded cherry-blossoms.png: {} bytes", image_bytes.len());

    // Run the pipeline with default config.
    let config = mujou_pipeline::PipelineConfig::default();
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
