//! SVG preview component for rendering traced paths.

use std::rc::Rc;

use dioxus::prelude::*;
use mujou_export::build_path_data;
use mujou_pipeline::ProcessResult;

/// Props for the [`Preview`] component.
#[derive(Props, Clone, PartialEq)]
pub struct PreviewProps {
    /// The pipeline result to render (shared via `Rc` to avoid
    /// cloning the full `Vec<Point>` on every render).
    result: Rc<ProcessResult>,
}

/// Renders a `Polyline` as an inline SVG element.
///
/// The SVG `viewBox` matches the source image dimensions so coordinates
/// map 1:1. The SVG is responsive -- it fills the container width and
/// maintains aspect ratio.
#[component]
pub fn Preview(props: PreviewProps) -> Element {
    let d = build_path_data(&props.result.polyline);
    let w = props.result.dimensions.width;
    let h = props.result.dimensions.height;
    let view_box = format!("0 0 {w} {h}");

    rsx! {
        svg {
            xmlns: "http://www.w3.org/2000/svg",
            view_box: "{view_box}",
            class: "w-full h-full bg-[var(--preview-bg)] rounded",
            "preserveAspectRatio": "xMidYMid meet",
            role: "img",
            "aria-label": "Traced path preview",

            if !d.is_empty() {
                path {
                    d: "{d}",
                    fill: "none",
                    stroke: "var(--preview-stroke)",
                    stroke_width: "1",
                }
            }
        }
    }
}
