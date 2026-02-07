//! mujou-pipeline: Pure image processing pipeline (sans-IO).
//!
//! Converts raster images into vector polylines through:
//! grayscale -> blur -> edge detection -> contour tracing ->
//! simplification -> optimization -> joining -> optional mask.
//!
//! This crate has **no I/O dependencies** -- it operates on in-memory
//! byte slices and returns structured data. All browser/filesystem
//! interaction lives in `mujou-io`.

pub mod contour;
pub mod join;
pub mod types;

pub use contour::{ContourTracer, ContourTracerKind};
pub use join::{PathJoiner, PathJoinerKind};
pub use types::{Dimensions, PipelineConfig, PipelineError, Point, Polyline};
