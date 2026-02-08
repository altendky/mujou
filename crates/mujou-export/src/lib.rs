//! mujou-export: Pure format serializers (sans-IO)
//!
//! Converts polylines into output formats. Currently supports SVG.
//! Future formats: THR, G-code, DXF, PNG.

pub mod svg;

pub use svg::to_svg;
