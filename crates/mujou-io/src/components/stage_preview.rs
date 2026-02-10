//! Full-size preview for the currently selected pipeline stage.
//!
//! Dispatches between raster `<img>` display (for grayscale/blur/edges)
//! and inline SVG display (for vector stages).

use std::fmt::Write;
use std::rc::Rc;

use dioxus::prelude::*;
use mujou_pipeline::StagedResult;

use super::preview::build_path_data;
use crate::raster;
use crate::stage::StageId;

/// Props for the [`StagePreview`] component.
#[derive(Props, Clone)]
pub struct StagePreviewProps {
    /// Full pipeline result with all intermediate data.
    staged: Rc<StagedResult>,
    /// Which stage to display.
    selected: StageId,
}

impl PartialEq for StagePreviewProps {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.staged, &other.staged) && self.selected == other.selected
    }
}

/// Full-size preview for one pipeline stage.
///
/// Raster stages (Grayscale, Blur, Edges) are displayed as `<img>`
/// elements using Blob URLs. Vector stages are displayed as inline SVG.
#[component]
pub fn StagePreview(props: StagePreviewProps) -> Element {
    let staged = &props.staged;
    let selected = props.selected;
    let w = staged.dimensions.width;
    let h = staged.dimensions.height;

    match selected {
        StageId::Grayscale | StageId::Blur | StageId::Edges => {
            let image = match selected {
                StageId::Grayscale => &staged.grayscale,
                StageId::Blur => &staged.blurred,
                _ => &staged.edges,
            };

            match raster::gray_image_to_blob_url(image) {
                Ok(url) => rsx! {
                    img {
                        src: "{url}",
                        class: "w-full h-auto max-h-[70vh] bg-[var(--preview-bg)] rounded object-contain",
                        alt: "{selected} stage preview",
                        onload: move |_| raster::revoke_blob_url(&url),
                    }
                },
                Err(e) => rsx! {
                    p { class: "text-[var(--text-error)] text-sm",
                        "Failed to render {selected}: {e}"
                    }
                },
            }
        }

        StageId::Contours | StageId::Simplified => {
            let polylines = match selected {
                StageId::Contours => &staged.contours,
                _ => &staged.simplified,
            };
            let view_box = format!("0 0 {w} {h}");
            let path_data = build_multi_path_data(polylines);

            rsx! {
                svg {
                    xmlns: "http://www.w3.org/2000/svg",
                    view_box: "{view_box}",
                    class: "w-full h-auto max-h-[70vh] bg-[var(--preview-bg)] rounded",
                    "preserveAspectRatio": "xMidYMid meet",

                    for (i, d) in path_data.iter().enumerate() {
                        path {
                            key: "{i}",
                            d: "{d}",
                            fill: "none",
                            stroke: "var(--preview-stroke)",
                            stroke_width: "1",
                        }
                    }
                }
            }
        }

        StageId::Path | StageId::Masked => {
            let polyline = match selected {
                StageId::Masked => staged.final_polyline(),
                _ => &staged.joined,
            };
            let view_box = format!("0 0 {w} {h}");
            let d = build_path_data(polyline);

            rsx! {
                svg {
                    xmlns: "http://www.w3.org/2000/svg",
                    view_box: "{view_box}",
                    class: "w-full h-auto max-h-[70vh] bg-[var(--preview-bg)] rounded",
                    "preserveAspectRatio": "xMidYMid meet",

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
    }
}

/// Build SVG path `d` attributes for multiple polylines.
fn build_multi_path_data(polylines: &[mujou_pipeline::Polyline]) -> Vec<String> {
    polylines
        .iter()
        .filter_map(|pl| {
            let points = pl.points();
            if points.len() < 2 {
                return None;
            }
            let mut d = String::new();
            for (i, p) in points.iter().enumerate() {
                let cmd = if i == 0 { "M" } else { "L" };
                let _ = write!(d, "{cmd} {:.1} {:.1} ", p.x, p.y);
            }
            Some(d)
        })
        .collect()
}
