//! SVG preview component for rendering traced paths.

use std::fmt::Write;

use dioxus::prelude::*;
use mujou_pipeline::{Dimensions, Polyline};

/// Props for the [`Preview`] component.
#[derive(Props, Clone, PartialEq)]
pub struct PreviewProps {
    /// The traced polyline to render.
    polyline: Polyline,
    /// Source image dimensions (used for the SVG `viewBox`).
    dimensions: Dimensions,
}

/// Renders a `Polyline` as an inline SVG element.
///
/// The SVG `viewBox` matches the source image dimensions so coordinates
/// map 1:1. The SVG is responsive -- it fills the container width and
/// maintains aspect ratio.
#[component]
pub fn Preview(props: PreviewProps) -> Element {
    let d = build_path_data(&props.polyline);
    let w = props.dimensions.width;
    let h = props.dimensions.height;
    let view_box = format!("0 0 {w} {h}");

    rsx! {
        svg {
            xmlns: "http://www.w3.org/2000/svg",
            view_box: "{view_box}",
            class: "w-full h-auto max-h-[70vh] bg-white rounded",
            "preserveAspectRatio": "xMidYMid meet",

            if !d.is_empty() {
                path {
                    d: "{d}",
                    fill: "none",
                    stroke: "black",
                    stroke_width: "1",
                }
            }
        }
    }
}

/// Build an SVG path `d` attribute from a polyline.
///
/// Uses `M` for the first point and `L` for subsequent points.
/// Coordinates are formatted to 1 decimal place (matching the export
/// serializer).
fn build_path_data(polyline: &Polyline) -> String {
    let points = polyline.points();
    if points.len() < 2 {
        return String::new();
    }

    let mut d = String::new();
    for (i, p) in points.iter().enumerate() {
        let cmd = if i == 0 { "M" } else { "L" };
        let _ = write!(d, "{cmd}{:.1},{:.1} ", p.x, p.y);
    }
    d
}
