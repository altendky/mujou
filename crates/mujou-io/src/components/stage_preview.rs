//! Full-size preview for the currently selected pipeline stage.
//!
//! Dispatches between raster `<img>` display (for grayscale/blur/edges)
//! and inline SVG display (for vector stages).

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

    // Track the current blob URL so we can revoke it on re-render
    // (handles rapid re-renders where onload/onerror never fires on the
    // replaced <img>) and on unmount/stage-switch.
    let mut prev_blob_url: Signal<Option<String>> = use_signal(|| None);

    // Revoke outstanding blob URL when the component is destroyed.
    {
        let prev_blob_url = prev_blob_url;
        use_drop(move || {
            if let Some(ref url) = *prev_blob_url.peek() {
                raster::revoke_blob_url(url);
            }
        });
    }

    // If switching away from a raster stage, clean up any outstanding
    // blob URL that may not have been revoked via onload/onerror.
    if !selected.is_raster()
        && let Some(ref prev) = prev_blob_url.take()
    {
        raster::revoke_blob_url(prev);
    }

    match selected {
        StageId::Grayscale | StageId::Blur | StageId::Edges => {
            let image = match selected {
                StageId::Grayscale => &staged.grayscale,
                StageId::Blur => &staged.blurred,
                _ => &staged.edges,
            };

            // Revoke the previous blob URL before creating a new one.
            if let Some(ref prev) = prev_blob_url.take() {
                raster::revoke_blob_url(prev);
            }

            match raster::gray_image_to_blob_url(image) {
                Ok(url) => {
                    prev_blob_url.set(Some(url.clone()));
                    let url_for_error = url.clone();
                    rsx! {
                        img {
                            src: "{url}",
                            class: "w-full h-auto max-h-[70vh] bg-[var(--preview-bg)] rounded object-contain",
                            alt: "{selected} stage preview",
                            onload: move |_| raster::revoke_blob_url(&url),
                            onerror: move |_| raster::revoke_blob_url(&url_for_error),
                        }
                    }
                }
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
            let d = build_path_data(pl);
            if d.is_empty() { None } else { Some(d) }
        })
        .collect()
}
