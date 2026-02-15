//! mujou-export: Pure format serializers (sans-IO)
//!
//! Converts polylines into output formats. Currently supports SVG.
//! Future formats: THR, G-code, DXF, PNG.

pub mod svg;

pub use svg::{SvgMetadata, to_diagnostic_svg, to_segment_diagnostic_svg, to_svg};
