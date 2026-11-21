//! Dioxus UI components for mujou.
//!
//! Provides the file upload button and drag overlay, SVG preview canvas,
//! export panel, filmstrip stage navigation, per-stage controls, and
//! stage preview.

// Dioxus Props generates sibling empty enums for missing required fields.
// These module expectations cover those helpers and fail when no longer needed.
#[expect(clippy::empty_enums, reason = "Dioxus Props builder diagnostics")]
mod export;
#[expect(clippy::empty_enums, reason = "Dioxus Props builder diagnostics")]
mod filmstrip;
#[expect(clippy::empty_enums, reason = "Dioxus Props builder diagnostics")]
mod preview;
#[expect(clippy::empty_enums, reason = "Dioxus Props builder diagnostics")]
mod stage_controls;
#[expect(clippy::empty_enums, reason = "Dioxus Props builder diagnostics")]
mod stage_preview;
#[expect(clippy::empty_enums, reason = "Dioxus Props builder diagnostics")]
mod upload;

pub use export::ExportPanel;
pub use filmstrip::Filmstrip;
pub use preview::Preview;
pub use stage_controls::StageControls;
pub use stage_preview::StagePreview;
pub use stage_preview::canvas_view_box;
pub use stage_preview::compute_view_box;
pub use upload::FileUpload;
