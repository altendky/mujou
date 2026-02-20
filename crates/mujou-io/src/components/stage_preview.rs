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
use mujou_pipeline::segment_analysis::find_top_segments;
use mujou_pipeline::{MaskShape, MstEdgeInfo};

/// Props for the [`StagePreview`] component.
#[derive(Props, Clone)]
pub struct StagePreviewProps {
    /// Pre-rendered pipeline result with Blob URLs for raster stages.
    /// `None` during initial processing — a placeholder is shown.
    result: Option<Rc<WorkerResult>>,
    /// Which stage to display.
    selected: StageId,
    /// Whether the dark theme is active.  Passed as a prop (rather than
    /// read from context) so the `PartialEq` memoisation check captures
    /// theme changes and triggers a re-render.
    is_dark: bool,
}

impl PartialEq for StagePreviewProps {
    fn eq(&self, other: &Self) -> bool {
        let results_eq = match (&self.result, &other.result) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        results_eq && self.selected == other.selected && self.is_dark == other.is_dark
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
    let is_dark = props.is_dark;

    // Diagnostic overlay toggle provided by the app root.
    let show_diagnostics: Signal<bool> = use_context();

    rsx! {
        // Raster stage <img> elements are always present in the DOM so
        // the browser eagerly decodes their blob URLs. Non-selected
        // raster stages are hidden with `display: none`.
        {render_raster_img(result.original_url.url(), "Original", selected == StageId::Original)}
        {render_raster_img(result.downsampled_url.url(), "Downsampled", selected == StageId::Downsampled)}
        {render_raster_img(result.blur_url.url(), "Blur", selected == StageId::Blur)}
        {render_raster_edges(result, selected == StageId::Edges, is_dark)}

        // Vector stages — conditionally rendered (SVG is instant).
        {render_vector_preview(result, selected, show_diagnostics())}
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
#[allow(clippy::too_many_lines)]
fn render_vector_preview(
    result: &WorkerResult,
    selected: StageId,
    show_diagnostics: bool,
) -> Element {
    match selected {
        StageId::Contours | StageId::Simplified => {
            let polylines = result.polylines_for_stage(selected);
            let view_box = compute_view_box(&polylines);
            let path_data = build_multi_path_data_normalized(&polylines);
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
                        "vector-effect": "non-scaling-stroke",
                    }
                    }
                }
            }
        }

        StageId::Canvas => {
            let polylines = result.polylines_for_stage(selected);
            let view_box = canvas_view_box(&result.canvas.shape);
            let path_data = build_multi_path_data_normalized(&polylines);

            rsx! {
                svg {
                    xmlns: "http://www.w3.org/2000/svg",
                    view_box: "{view_box}",
                    class: "w-full h-full bg-[var(--preview-bg)] rounded",
                    "preserveAspectRatio": "xMidYMid meet",
                    role: "img",
                    "aria-label": "Canvas stage preview",

                    for (i, d) in path_data.iter().enumerate() {
                    path {
                        key: "{i}",
                        d: "{d}",
                        fill: "none",
                        stroke: "var(--preview-stroke)",
                        stroke_width: "1",
                        "vector-effect": "non-scaling-stroke",
                    }
                    }
                }
            }
        }

        StageId::Output => {
            let polyline = &result.output;
            let view_box = canvas_view_box(&result.canvas.shape);
            let d = build_path_data_normalized(polyline);

            rsx! {
                svg {
                    xmlns: "http://www.w3.org/2000/svg",
                    view_box: "{view_box}",
                    class: "w-full h-full bg-[var(--preview-bg)] rounded",
                    "preserveAspectRatio": "xMidYMid meet",
                    role: "img",
                    "aria-label": "Output stage preview",

                    if !d.is_empty() {
                        path {
                            d: "{d}",
                            fill: "none",
                            stroke: "var(--preview-stroke)",
                            stroke_width: "1",
                            "vector-effect": "non-scaling-stroke",
                        }
                    }
                }
            }
        }

        StageId::Join => {
            let polyline = &result.joined;
            let view_box = canvas_view_box(&result.canvas.shape);
            let d = build_path_data_normalized(polyline);

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
                    class: "w-full h-full bg-[var(--preview-bg)] rounded",
                    "preserveAspectRatio": "xMidYMid meet",
                    role: "img",
                    "aria-label": "Join stage preview",

                    if !d.is_empty() {
                        path {
                            d: "{d}",
                            fill: "none",
                            stroke: if show_diagnostics { "var(--diag-stroke)" } else { "var(--preview-stroke)" },
                            stroke_width: "1",
                            "vector-effect": "non-scaling-stroke",
                        }
                    }

                    // Diagnostic: MST connecting edges (red lines).
                    {render_mst_edges(mst_edges)}

                    // Diagnostic: top-N longest segments (color-coded).
                    {render_top_segments(&top_segments)}

                    // Diagnostic: hollow green circle at path start point.
                    {render_start_indicator(polyline, show_diagnostics)}
                }
            }
        }

        // Raster stages are handled by the always-present <img> elements.
        _ => rsx! {},
    }
}

/// Build an SVG path `d` attribute string for a normalized-space polyline,
/// negating Y for SVG (+Y down).
///
/// Normalized space is +Y up; SVG is +Y down, so Y coordinates are
/// negated before emitting.
#[allow(clippy::cast_possible_truncation)]
pub fn build_path_data_normalized(polyline: &mujou_pipeline::Polyline) -> String {
    use std::fmt::Write;

    let points = polyline.points();
    if points.len() < 2 {
        return String::new();
    }

    let mut d = String::new();
    let first = &points[0];
    let _ = write!(d, "M{},{}", first.x as f32, -first.y as f32);
    for p in &points[1..] {
        let _ = write!(d, " L{},{}", p.x as f32, -p.y as f32);
    }
    d
}

/// Build SVG path `d` attributes for multiple normalized-space polylines,
/// negating Y for SVG.
pub fn build_multi_path_data_normalized(polylines: &[mujou_pipeline::Polyline]) -> Vec<String> {
    polylines
        .iter()
        .filter_map(|pl| {
            let d = build_path_data_normalized(pl);
            if d.is_empty() { None } else { Some(d) }
        })
        .collect()
}

/// Compute an SVG `viewBox` that contains all polyline data.
///
/// In normalized space (stages 5–9), polylines are centered at the
/// origin with typical extent ≈ ±1.5.  The viewBox is computed purely
/// from the data bounds.  When no data is present, a default covering
/// the unit circle (±1.5) is returned.
///
/// A small padding is added so strokes aren't clipped at the boundary.
#[must_use]
pub fn compute_view_box(polylines: &[mujou_pipeline::Polyline]) -> String {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for poly in polylines {
        for p in poly.points() {
            let svg_y = -p.y;
            min_x = min_x.min(p.x);
            min_y = min_y.min(svg_y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(svg_y);
        }
    }

    // Default to unit circle extent if no data.
    if min_x > max_x {
        min_x = -1.5;
        min_y = -1.5;
        max_x = 1.5;
        max_y = 1.5;
    }

    // Padding: 5% of the larger dimension (minimum 0.05).
    let dx = max_x - min_x;
    let dy = max_y - min_y;
    let pad = 0.05 * dx.max(dy).max(0.1);
    min_x -= pad;
    min_y -= pad;
    max_x += pad;
    max_y += pad;

    format!("{min_x} {min_y} {} {}", max_x - min_x, max_y - min_y)
}

/// Compute an SVG `viewBox` from the canvas mask geometry.
///
/// Post-canvas stages (Canvas, Join, Output) use a fixed viewBox
/// derived from the mask shape rather than the data bounds.  This
/// ensures consistent framing regardless of zoom level or whether
/// any polylines were clipped.
///
/// A small padding is added so border strokes aren't clipped.
#[must_use]
pub fn canvas_view_box(shape: &MaskShape) -> String {
    let (min_x, min_y, max_x, max_y) = match *shape {
        MaskShape::Circle { center, radius } => (
            center.x - radius,
            -center.y - radius,
            center.x + radius,
            -center.y + radius,
        ),
        MaskShape::Rectangle {
            center,
            half_width,
            half_height,
        } => (
            center.x - half_width,
            -center.y - half_height,
            center.x + half_width,
            -center.y + half_height,
        ),
    };

    // Padding: 5% of the larger dimension (minimum 0.05).
    let dx = max_x - min_x;
    let dy = max_y - min_y;
    let pad = 0.05 * dx.max(dy).max(0.1);

    format!(
        "{} {} {} {}",
        min_x - pad,
        min_y - pad,
        2.0f64.mul_add(pad, dx),
        2.0f64.mul_add(pad, dy),
    )
}

// ───────────────────────── Diagnostic overlay ────────────────────────

/// Number of longest segments to highlight in the diagnostic overlay.
const TOP_N_SEGMENTS: usize = 5;

/// Render MST connecting edges as red SVG `<line>` elements.
fn render_mst_edges(edges: &[MstEdgeInfo]) -> Element {
    if edges.is_empty() {
        return rsx! {};
    }
    rsx! {
        g {
            "data-layer": "mst-edges",
            for (i, edge) in edges.iter().enumerate() {
                {
                    let y1 = -edge.point_a.1;
                    let y2 = -edge.point_b.1;
                    rsx! {
                        line {
                            key: "mst-{i}",
                            x1: "{edge.point_a.0}",
                            y1: "{y1}",
                            x2: "{edge.point_b.0}",
                            y2: "{y2}",
                            stroke: "var(--diag-mst)",
                            stroke_width: "2",
                            stroke_dasharray: "4,2",
                            opacity: "0.8",
                            "vector-effect": "non-scaling-stroke",
                        }
                    }
                }
            }
        }
    }
}

/// Render a hollow green circle at the first point of the joined path.
///
/// In normalized space the typical drawing extent is ≈ 3 units
/// (diameter of the unit circle + padding).  The indicator radius is
/// a fixed fraction of that so it stays visually consistent.
/// `vector-effect: non-scaling-stroke` keeps the stroke visually
/// stable when the browser scales the SVG to fit the viewport.
fn render_start_indicator(polyline: &mujou_pipeline::Polyline, show: bool) -> Element {
    if !show {
        return rsx! {};
    }
    let Some(start) = polyline.first() else {
        return rsx! {};
    };

    // Fixed radius in normalized units — approximately 1.2% of the
    // typical ~3 unit viewBox extent, matching the old pixel-space
    // proportions.
    let r = 0.035;

    let cy = -start.y;
    rsx! {
        circle {
            cx: "{start.x}",
            cy: "{cy}",
            r: "{r}",
            fill: "none",
            stroke: "var(--diag-start)",
            stroke_width: "2",
            "vector-effect": "non-scaling-stroke",
            opacity: "0.9",
            "data-layer": "start-indicator",
        }
    }
}

/// CSS variable names for the top-N segment colors, corresponding to
/// `--diag-seg-1` through `--diag-seg-5` in `theme.css`.
const SEGMENT_CSS_VARS: &[&str] = &[
    "var(--diag-seg-1)",
    "var(--diag-seg-2)",
    "var(--diag-seg-3)",
    "var(--diag-seg-4)",
    "var(--diag-seg-5)",
];
const _: () = assert!(
    TOP_N_SEGMENTS <= SEGMENT_CSS_VARS.len(),
    "Need at least as many CSS vars as TOP_N_SEGMENTS"
);

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
                    let color = SEGMENT_CSS_VARS.get(rank).copied()
                        .unwrap_or("var(--diag-stroke)");
                    let y1 = -seg.from.1;
                    let y2 = -seg.to.1;
                    rsx! {
                        line {
                            key: "seg-{rank}",
                            x1: "{seg.from.0}",
                            y1: "{y1}",
                            x2: "{seg.to.0}",
                            y2: "{y2}",
                            stroke: "{color}",
                            stroke_width: "3",
                            opacity: "0.9",
                            "vector-effect": "non-scaling-stroke",
                        }
                    }
                }
            }
        }
    }
}
