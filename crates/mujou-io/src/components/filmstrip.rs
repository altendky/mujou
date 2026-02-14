//! Horizontal filmstrip of pipeline stage thumbnails.
//!
//! Displays a scrollable strip of small previews for each pipeline stage.
//! Clicking a thumbnail selects that stage for full-size preview and
//! shows its parameter controls.
//!
//! All raster thumbnails use pre-built Blob URLs from the worker result
//! (PNG encoding happens in the worker thread, not the main thread).
//! When no result is available yet (initial processing), placeholder
//! tiles with a skeleton shimmer are shown instead.

use std::rc::Rc;

use dioxus::prelude::*;

use crate::stage::StageId;
use crate::worker::WorkerResult;

/// Props for the [`Filmstrip`] component.
#[derive(Props, Clone)]
pub struct FilmstripProps {
    /// Pre-rendered pipeline result with Blob URLs for raster stages.
    /// `None` during initial processing — placeholder tiles are shown.
    result: Option<Rc<WorkerResult>>,
    /// Currently selected stage.
    selected: StageId,
    /// Callback fired when a stage tile is clicked.
    on_select: EventHandler<StageId>,
}

impl PartialEq for FilmstripProps {
    fn eq(&self, other: &Self) -> bool {
        let results_eq = match (&self.result, &other.result) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        results_eq && self.selected == other.selected
    }
}

/// Horizontal scrollable strip of pipeline stage thumbnails.
///
/// Each tile shows a small preview of one stage's output. The selected
/// tile is highlighted with a border accent color. When no pipeline
/// result is available yet, placeholder tiles with a skeleton shimmer
/// background are shown for each stage.
#[component]
pub fn Filmstrip(props: FilmstripProps) -> Element {
    let is_dark: Signal<bool> = use_context();

    rsx! {
        div {
            class: "flex flex-nowrap overflow-x-auto gap-2 py-2 scrollbar-thin",

            for stage in StageId::ALL {
                {render_tile(props.result.as_deref(), stage, props.selected == stage, &props.on_select, is_dark())}
            }
        }
    }
}

/// Render a single filmstrip tile.
fn render_tile(
    result: Option<&WorkerResult>,
    stage: StageId,
    is_selected: bool,
    on_select: &EventHandler<StageId>,
    is_dark: bool,
) -> Element {
    let border = if is_selected {
        "border-2 border-[var(--border-accent)]"
    } else {
        "border border-[var(--border)]"
    };

    let onclick = {
        let on_select = *on_select;
        move |_| on_select.call(stage)
    };

    rsx! {
        button {
            class: "flex-shrink-0 flex flex-col items-center gap-1 p-1 rounded cursor-pointer
                    w-[80px] md:w-[100px] lg:w-[120px] bg-[var(--surface)] hover:bg-[var(--surface-active)]
                    transition-colors {border}",
            onclick: onclick,
            title: "{stage.label()}",
            aria_label: "Show {stage.label()} stage",
            "aria-pressed": "{is_selected}",

            // Thumbnail (or placeholder skeleton)
            div { class: "w-full aspect-square overflow-hidden rounded bg-[var(--preview-bg)]",
                {result.map_or_else(render_placeholder, |r| render_thumbnail(r, stage, is_dark))}
            }

            // Label: full name at lg+, short label below lg
            span { class: "text-xs text-[var(--text-secondary)] truncate w-full text-center
                          hidden lg:block",
                "{stage.label()}"
            }
            span { class: "text-xs text-[var(--text-secondary)] truncate w-full text-center
                          lg:hidden",
                "{stage.short_label()}"
            }
        }
    }
}

/// Render a placeholder skeleton for a filmstrip tile when no result is
/// available yet.
fn render_placeholder() -> Element {
    rsx! {
        div {
            class: "w-full h-full animate-pulse bg-[var(--border)]",
            role: "status",
            aria_label: "Loading thumbnail",
        }
    }
}

/// Render the thumbnail content for a stage tile.
fn render_thumbnail(result: &WorkerResult, stage: StageId, is_dark: bool) -> Element {
    match stage {
        StageId::Original => render_img_thumb(result.original_url.url(), "Original thumbnail"),
        StageId::Downsampled => {
            render_img_thumb(result.downsampled_url.url(), "Downsampled thumbnail")
        }
        StageId::Blur => render_img_thumb(result.blur_url.url(), "Blur thumbnail"),

        StageId::Edges => {
            let url = if is_dark {
                result.edges_dark_url.url()
            } else {
                result.edges_light_url.url()
            };
            render_img_thumb(url, "Edges thumbnail")
        }

        StageId::Contours | StageId::Simplified | StageId::Masked => {
            let mask_polylines;
            let polylines: &[mujou_pipeline::Polyline] = match stage {
                StageId::Contours => &result.contours,
                StageId::Masked => {
                    if let Some(mr) = &result.masked {
                        mask_polylines = mr.all_polylines().cloned().collect::<Vec<_>>();
                        &mask_polylines
                    } else {
                        &result.simplified
                    }
                }
                _ => &result.simplified,
            };
            let w = result.dimensions.width;
            let h = result.dimensions.height;
            let view_box = format!("0 0 {w} {h}");

            rsx! {
                svg {
                    xmlns: "http://www.w3.org/2000/svg",
                    view_box: "{view_box}",
                    class: "w-full h-full",
                    "preserveAspectRatio": "xMidYMid meet",

                    for polyline in polylines.iter() {
                        {render_thumbnail_path(polyline)}
                    }
                }
            }
        }

        StageId::Join => {
            let polyline = &result.joined;
            let w = result.dimensions.width;
            let h = result.dimensions.height;
            let view_box = format!("0 0 {w} {h}");
            let d = super::preview::build_path_data(polyline);

            rsx! {
                svg {
                    xmlns: "http://www.w3.org/2000/svg",
                    view_box: "{view_box}",
                    class: "w-full h-full",
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

/// Render a raster thumbnail from a pre-built Blob URL.
fn render_img_thumb(url: &str, alt: &str) -> Element {
    rsx! {
        img {
            src: "{url}",
            class: "w-full h-full object-cover",
            alt: "{alt}",
        }
    }
}

/// Render a single polyline as an SVG path for a thumbnail.
fn render_thumbnail_path(polyline: &mujou_pipeline::Polyline) -> Element {
    let d = super::preview::build_path_data(polyline);
    if d.is_empty() {
        return rsx! {};
    }

    rsx! {
        path {
            d: "{d}",
            fill: "none",
            stroke: "var(--preview-stroke)",
            stroke_width: "1",
        }
    }
}
