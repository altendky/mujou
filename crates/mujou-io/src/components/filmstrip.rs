//! Horizontal filmstrip of pipeline stage thumbnails.
//!
//! Displays a scrollable strip of small previews for each pipeline stage.
//! Clicking a thumbnail selects that stage for full-size preview and
//! shows its parameter controls.
//!
//! All raster thumbnails use pre-built Blob URLs from the worker result
//! (PNG encoding happens in the worker thread, not the main thread).

use std::rc::Rc;

use dioxus::prelude::*;

use crate::stage::StageId;
use crate::worker::WorkerResult;

/// Props for the [`Filmstrip`] component.
#[derive(Props, Clone)]
pub struct FilmstripProps {
    /// Pre-rendered pipeline result with Blob URLs for raster stages.
    result: Rc<WorkerResult>,
    /// Currently selected stage.
    selected: StageId,
    /// Callback fired when a stage tile is clicked.
    on_select: EventHandler<StageId>,
}

impl PartialEq for FilmstripProps {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.result, &other.result) && self.selected == other.selected
    }
}

/// Horizontal scrollable strip of pipeline stage thumbnails.
///
/// Each tile shows a small preview of one stage's output. The selected
/// tile is highlighted with a border accent color.
#[component]
pub fn Filmstrip(props: FilmstripProps) -> Element {
    let is_dark: Signal<bool> = use_context();

    rsx! {
        div {
            class: "flex flex-nowrap overflow-x-auto gap-2 py-2 scrollbar-thin",

            for stage in StageId::ALL {
                {render_tile(&props.result, stage, props.selected == stage, &props.on_select, is_dark())}
            }
        }
    }
}

/// Render a single filmstrip tile.
fn render_tile(
    result: &WorkerResult,
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

            // Thumbnail
            div { class: "w-full aspect-square overflow-hidden rounded bg-[var(--preview-bg)]",
                {render_thumbnail(result, stage, is_dark)}
            }

            // Label
            span { class: "text-xs text-[var(--text-secondary)] truncate w-full text-center
                          hidden md:block",
                "{stage.label()}"
            }
            span { class: "text-xs text-[var(--text-secondary)] md:hidden",
                "{stage.abbreviation()}"
            }
        }
    }
}

/// Render the thumbnail content for a stage tile.
fn render_thumbnail(result: &WorkerResult, stage: StageId, is_dark: bool) -> Element {
    match stage {
        StageId::Original => render_img_thumb(result.original_url.url(), "Original thumbnail"),
        StageId::Grayscale => render_img_thumb(result.grayscale_url.url(), "Grayscale thumbnail"),
        StageId::Blur => render_img_thumb(result.blur_url.url(), "Blur thumbnail"),

        StageId::Edges => {
            let url = if is_dark {
                result.edges_dark_url.url()
            } else {
                result.edges_light_url.url()
            };
            render_img_thumb(url, "Edges thumbnail")
        }

        StageId::Contours | StageId::Simplified => {
            let polylines = match stage {
                StageId::Contours => &result.contours,
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

        StageId::Path | StageId::Masked => {
            let polyline = match stage {
                StageId::Masked => result.final_polyline(),
                _ => &result.joined,
            };
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
