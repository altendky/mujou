//! mujou-io: Browser I/O and Dioxus component library.
//!
//! Handles file uploads, Blob downloads, raster image encoding,
//! web worker communication, and provides reusable UI components
//! for the mujou web application.

pub mod analytics;
pub mod clipboard;
pub mod components;
pub mod download;
pub mod raster;
pub mod stage;
pub mod worker;

pub use components::{ExportPanel, FileUpload, Filmstrip, Preview, StageControls, StagePreview};
pub use stage::StageId;
pub use worker::{PipelineWorker, WorkerResult};
