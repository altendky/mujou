//! mujou-export: Pure format serializers (sans-IO)
//!
//! Converts polylines into output formats. Currently supports SVG and THR.
//! Future formats: G-code, DXF, PNG.

pub mod svg;
pub mod thr;

pub use svg::{
    DocumentMapping, SvgMetadata, build_path_data, document_mapping, to_diagnostic_svg,
    to_segment_diagnostic_svg, to_svg,
};
pub use thr::{ThrMetadata, to_thr};
