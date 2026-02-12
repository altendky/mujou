//! Full-size preview for the currently selected pipeline stage.
//!
//! Dispatches between raster `<img>` display (for Original, Grayscale,
//! Blur, Edges) and inline SVG display (for vector stages).
//!
//! All raster Blob URLs are pre-built by the worker — no PNG encoding
//! happens on the main thread. Raster `<img>` elements are always
//! present in the DOM (hidden when not selected) so the browser eagerly
//! decodes them and stage switching is instant.

use std::rc::Rc;

use dioxus::prelude::*;

use super::preview::build_path_data;
use crate::stage::StageId;
use crate::worker::WorkerResult;

/// Props for the [`StagePreview`] component.
#[derive(Props, Clone)]
pub struct StagePreviewProps {
    /// Pre-rendered pipeline result with Blob URLs for raster stages.
    result: Rc<WorkerResult>,
    /// Which stage to display.
    selected: StageId,
}

impl PartialEq for StagePreviewProps {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.result, &other.result) && self.selected == other.selected
    }
}

/// Full-size preview for one pipeline stage.
///
/// Raster stages are displayed as `<img>` elements using pre-built
/// Blob URLs. All four raster images are always in the DOM (hidden
/// when not selected) so the browser eagerly decodes them — switching
/// stages is instant with no flicker.
///
/// Vector stages (SVG) are conditionally rendered since inline SVG
/// paints synchronously without an async decode step.
#[component]
pub fn StagePreview(props: StagePreviewProps) -> Element {
    let result = &props.result;
    let selected = props.selected;
    let w = result.dimensions.width;
    let h = result.dimensions.height;

    // Reactive theme signal provided by the app root.
    let is_dark: Signal<bool> = use_context();

    rsx! {
        // Raster stage <img> elements are always present in the DOM so
        // the browser eagerly decodes their blob URLs. Non-selected
        // raster stages are hidden with `display: none`.
        {render_raster_img(result.original_url.url(), "Original", selected == StageId::Original)}
        {render_raster_img(result.grayscale_url.url(), "Grayscale", selected == StageId::Grayscale)}
        {render_raster_img(result.blur_url.url(), "Blur", selected == StageId::Blur)}
        {render_raster_edges(result, selected == StageId::Edges, is_dark())}

        // Vector stages — conditionally rendered (SVG is instant).
        {render_vector_preview(result, selected, w, h)}
    }
}

/// Render a raster preview image, hidden when not visible.
fn render_raster_img(url: &str, label: &str, visible: bool) -> Element {
    let hidden = if visible { "" } else { " hidden" };
    rsx! {
        img {
            src: "{url}",
            class: "w-full h-auto max-h-[70vh] bg-[var(--preview-bg)] rounded object-contain{hidden}",
            alt: "{label} stage preview",
        }
    }
}

/// Render the Edges preview with theme-dependent URL.
fn render_raster_edges(result: &WorkerResult, visible: bool, is_dark: bool) -> Element {
    let url = if is_dark {
        result.edges_dark_url.url()
    } else {
        result.edges_light_url.url()
    };
    let hidden = if visible { "" } else { " hidden" };
    rsx! {
        img {
            src: "{url}",
            class: "w-full h-auto max-h-[70vh] bg-[var(--preview-bg)] rounded object-contain{hidden}",
            alt: "Edges stage preview",
        }
    }
}

/// Render the vector (SVG) preview for Contours, Simplified, Path, or
/// Masked stages. Returns empty for raster stages.
fn render_vector_preview(result: &WorkerResult, selected: StageId, w: u32, h: u32) -> Element {
    match selected {
        StageId::Contours | StageId::Simplified | StageId::Masked => {
            let polylines = match selected {
                StageId::Contours => &result.contours,
                StageId::Masked => result.masked.as_deref().unwrap_or(&result.simplified),
                _ => &result.simplified,
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

        StageId::Path => {
            let polyline = &result.joined;
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

        // Raster stages are handled by the always-present <img> elements.
        _ => rsx! {},
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
