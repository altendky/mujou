//! Full-size preview for the currently selected pipeline stage.
//!
//! Dispatches between raster `<img>` display (for Original, Downsampled,
//! Blur, Edges) and inline SVG display (for vector stages).
//!
//! All raster Blob URLs are pre-built by the worker — no PNG encoding
//! happens on the main thread. Raster `<img>` elements are always
//! present in the DOM (hidden when not selected) so the browser eagerly
//! decodes them and stage switching is instant.
//!
//! When no result is available yet (initial processing), a placeholder
//! container is shown matching the preview area styling.
//!
//! When the diagnostic overlay is active (toggled via `show_diagnostics`
//! context signal), the Join stage preview renders additional layers:
//! - Top-N longest segments highlighted in distinct colors
//! - MST connecting edges shown as red lines

use std::rc::Rc;

use dioxus::prelude::*;

use crate::stage::StageId;
use crate::worker::WorkerResult;
use mujou_export::build_path_data;
use mujou_pipeline::MstEdgeInfo;
use mujou_pipeline::segment_analysis::{SEGMENT_COLORS, find_top_segments};

/// Props for the [`StagePreview`] component.
#[derive(Props, Clone)]
pub struct StagePreviewProps {
    /// Pre-rendered pipeline result with Blob URLs for raster stages.
    /// `None` during initial processing — a placeholder is shown.
    result: Option<Rc<WorkerResult>>,
    /// Which stage to display.
    selected: StageId,
}

impl PartialEq for StagePreviewProps {
    fn eq(&self, other: &Self) -> bool {
        let results_eq = match (&self.result, &other.result) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        results_eq && self.selected == other.selected
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
///
/// When no result is available yet, a placeholder container matching
/// the preview area styling is rendered.
#[component]
pub fn StagePreview(props: StagePreviewProps) -> Element {
    let Some(ref result) = props.result else {
        // No result yet — show placeholder matching the preview area
        return rsx! {
            div {
                class: "w-full h-full bg-[var(--preview-bg)] rounded",
                role: "status",
                aria_label: "Loading preview",
            }
        };
    };

    let selected = props.selected;
    let w = result.dimensions.width;
    let h = result.dimensions.height;

    // Reactive theme signal provided by the app root.
    let is_dark: Signal<bool> = use_context();

    // Diagnostic overlay toggle provided by the app root.
    let show_diagnostics: Signal<bool> = use_context();

    rsx! {
        // Raster stage <img> elements are always present in the DOM so
        // the browser eagerly decodes their blob URLs. Non-selected
        // raster stages are hidden with `display: none`.
        {render_raster_img(result.original_url.url(), "Original", selected == StageId::Original)}
        {render_raster_img(result.downsampled_url.url(), "Downsampled", selected == StageId::Downsampled)}
        {render_raster_img(result.blur_url.url(), "Blur", selected == StageId::Blur)}
        {render_raster_edges(result, selected == StageId::Edges, is_dark())}

        // Vector stages — conditionally rendered (SVG is instant).
        {render_vector_preview(result, selected, w, h, show_diagnostics())}
    }
}

/// Render a raster preview image, hidden when not visible.
fn render_raster_img(url: &str, label: &str, visible: bool) -> Element {
    let hidden = if visible { "" } else { " hidden" };
    rsx! {
        img {
            src: "{url}",
            class: "w-full h-full bg-[var(--preview-bg)] rounded object-contain{hidden}",
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
            class: "w-full h-full bg-[var(--preview-bg)] rounded object-contain{hidden}",
            alt: "Edges stage preview",
        }
    }
}

/// Render the vector (SVG) preview for Contours, Simplified, Join, or
/// Masked stages. Returns empty for raster stages.
fn render_vector_preview(
    result: &WorkerResult,
    selected: StageId,
    w: u32,
    h: u32,
    show_diagnostics: bool,
) -> Element {
    match selected {
        StageId::Contours | StageId::Simplified | StageId::Masked => {
            let polylines = result.polylines_for_stage(selected);
            let view_box = compute_view_box(&polylines, w, h);
            let path_data = build_multi_path_data(&polylines);
            let stage_label = selected.label();

            rsx! {
                svg {
                    xmlns: "http://www.w3.org/2000/svg",
                    view_box: "{view_box}",
                    class: "w-full h-full bg-[var(--preview-bg)] rounded",
                    "preserveAspectRatio": "xMidYMid meet",
                    role: "img",
                    "aria-label": "{stage_label} stage preview",

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

        StageId::Join => {
            let polyline = &result.joined;
            let view_box = compute_view_box(std::slice::from_ref(polyline), w, h);
            let d = build_path_data(polyline);

            // Pre-compute diagnostic overlays when active.
            let top_segments = if show_diagnostics {
                find_top_segments(std::slice::from_ref(polyline), TOP_N_SEGMENTS)
            } else {
                Vec::new()
            };
            let mst_edges = if show_diagnostics {
                &result.mst_edge_details
            } else {
                &[][..]
            };

            rsx! {
                svg {
                    xmlns: "http://www.w3.org/2000/svg",
                    view_box: "{view_box}",
                    class: if show_diagnostics {
                        "w-full h-full bg-[#1a1a1a] rounded"
                    } else {
                        "w-full h-full bg-[var(--preview-bg)] rounded"
                    },
                    "preserveAspectRatio": "xMidYMid meet",
                    role: "img",
                    "aria-label": "Join stage preview",

                    if !d.is_empty() {
                        path {
                            d: "{d}",
                            fill: "none",
                            stroke: if show_diagnostics { "white" } else { "var(--preview-stroke)" },
                            stroke_width: "1",
                        }
                    }

                    // Diagnostic: MST connecting edges (red lines).
                    {render_mst_edges(mst_edges)}

                    // Diagnostic: top-N longest segments (color-coded).
                    {render_top_segments(&top_segments)}

                    // Diagnostic: hollow green circle at path start point.
                    {render_start_indicator(polyline, show_diagnostics, w, h)}
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

/// Compute an SVG `viewBox` that contains all polyline data.
///
/// Encompasses both the image area (`0,0` to `w,h`) and any polyline
/// data that extends beyond it (e.g. a border circle).  Adds a small
/// padding so strokes aren't clipped at the boundary.
#[must_use]
pub fn compute_view_box(polylines: &[mujou_pipeline::Polyline], w: u32, h: u32) -> String {
    let mut min_x: f64 = 0.0;
    let mut min_y: f64 = 0.0;
    let mut max_x = f64::from(w);
    let mut max_y = f64::from(h);

    for poly in polylines {
        for p in poly.points() {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
    }

    // Small padding so strokes at the boundary aren't clipped.
    let pad = 1.0;
    min_x -= pad;
    min_y -= pad;
    max_x += pad;
    max_y += pad;

    format!("{min_x} {min_y} {} {}", max_x - min_x, max_y - min_y)
}

// ───────────────────────── Diagnostic overlay ────────────────────────

/// Number of longest segments to highlight in the diagnostic overlay.
const TOP_N_SEGMENTS: usize = 5;
const _: () = assert!(
    TOP_N_SEGMENTS <= SEGMENT_COLORS.len(),
    "Need at least as many colors as TOP_N_SEGMENTS"
);

/// Render MST connecting edges as red SVG `<line>` elements.
fn render_mst_edges(edges: &[MstEdgeInfo]) -> Element {
    if edges.is_empty() {
        return rsx! {};
    }
    rsx! {
        g {
            "data-layer": "mst-edges",
            for (i, edge) in edges.iter().enumerate() {
                line {
                    key: "mst-{i}",
                    x1: "{edge.point_a.0}",
                    y1: "{edge.point_a.1}",
                    x2: "{edge.point_b.0}",
                    y2: "{edge.point_b.1}",
                    stroke: "#ff0000",
                    stroke_width: "2",
                    stroke_dasharray: "4,2",
                    opacity: "0.8",
                }
            }
        }
    }
}

/// Render a hollow green circle at the first point of the joined path.
///
/// The radius and stroke width are proportional to the image diagonal
/// so the indicator looks consistent across different image sizes.
/// `vector-effect: non-scaling-stroke` keeps the stroke visually
/// stable when the browser scales the SVG to fit the viewport.
fn render_start_indicator(
    polyline: &mujou_pipeline::Polyline,
    show: bool,
    w: u32,
    h: u32,
) -> Element {
    if !show {
        return rsx! {};
    }
    let Some(start) = polyline.first() else {
        return rsx! {};
    };

    // Radius is proportional to the image diagonal so the circle is
    // a consistent fraction of the drawing regardless of resolution.
    // Stroke width uses `vector-effect: non-scaling-stroke` so it
    // stays a constant 2 screen-pixels regardless of how the browser
    // scales the SVG to fit the viewport.
    let diag = f64::from(w).hypot(f64::from(h));
    let r = diag * 0.012;

    rsx! {
        circle {
            cx: "{start.x}",
            cy: "{start.y}",
            r: "{r}",
            fill: "none",
            stroke: "#00cc44",
            stroke_width: "2",
            "vector-effect": "non-scaling-stroke",
            opacity: "0.9",
            "data-layer": "start-indicator",
        }
    }
}

/// Render the top-N longest segments as color-coded SVG `<line>` elements.
fn render_top_segments(segments: &[mujou_pipeline::RankedSegment]) -> Element {
    if segments.is_empty() {
        return rsx! {};
    }
    rsx! {
        g {
            "data-layer": "top-segments",
            for (rank, seg) in segments.iter().enumerate() {
                {
                    let color = SEGMENT_COLORS.get(rank).copied().unwrap_or("#ffffff");
                    rsx! {
                        line {
                            key: "seg-{rank}",
                            x1: "{seg.from.0}",
                            y1: "{seg.from.1}",
                            x2: "{seg.to.0}",
                            y2: "{seg.to.1}",
                            stroke: "{color}",
                            stroke_width: "3",
                            opacity: "0.9",
                        }
                    }
                }
            }
        }
    }
}
