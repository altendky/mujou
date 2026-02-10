//! Horizontal filmstrip of pipeline stage thumbnails.
//!
//! Displays a scrollable strip of small previews for each pipeline stage.
//! Clicking a thumbnail selects that stage for full-size preview and
//! shows its parameter controls.
//!
//! The Edges thumbnail uses themed colors (matching the active light/dark
//! mode) with both variants pre-computed for instant theme toggles.

use std::rc::Rc;

use dioxus::prelude::*;
use mujou_pipeline::StagedResult;

use crate::raster;
use crate::stage::StageId;

/// Props for the [`Filmstrip`] component.
#[derive(Props, Clone)]
pub struct FilmstripProps {
    /// Full pipeline result with all intermediate data.
    staged: Rc<StagedResult>,
    /// Currently selected stage.
    selected: StageId,
    /// Callback fired when a stage tile is clicked.
    on_select: EventHandler<StageId>,
}

impl PartialEq for FilmstripProps {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.staged, &other.staged) && self.selected == other.selected
    }
}

/// Horizontal scrollable strip of pipeline stage thumbnails.
///
/// Each tile shows a small preview of one stage's output. The selected
/// tile is highlighted with a border accent color.
#[component]
pub fn Filmstrip(props: FilmstripProps) -> Element {
    let is_dark: Signal<bool> = use_context();

    // Bridge the Rc prop into the reactive system via a Signal.
    let mut staged_signal = use_signal(|| Rc::clone(&props.staged));
    if !Rc::ptr_eq(&props.staged, &*staged_signal.peek()) {
        staged_signal.set(Rc::clone(&props.staged));
    }

    // Eagerly cached themed Blob URLs for the Edges thumbnail.
    // use_memo subscribes to staged_signal and recomputes when a new
    // Rc<StagedResult> arrives.
    let edge_thumb_cache = use_memo(move || {
        let staged = staged_signal();
        let ptr = Rc::as_ptr(&staged) as usize;
        raster::generate_themed_edge_urls(&staged.edges, ptr).ok()
    });

    rsx! {
        div {
            class: "flex flex-nowrap overflow-x-auto gap-2 py-2 scrollbar-thin",

            for stage in StageId::ALL {
                {render_tile(&props.staged, stage, props.selected == stage, &props.on_select, &edge_thumb_cache, is_dark())}
            }
        }
    }
}

/// Render a single filmstrip tile.
fn render_tile(
    staged: &Rc<StagedResult>,
    stage: StageId,
    is_selected: bool,
    on_select: &EventHandler<StageId>,
    edge_thumb_cache: &Memo<Option<raster::ThemedEdgeUrls>>,
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
                {render_thumbnail(staged, stage, edge_thumb_cache, is_dark)}
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
#[allow(clippy::option_if_let_else)]
fn render_thumbnail(
    staged: &StagedResult,
    stage: StageId,
    edge_thumb_cache: &Memo<Option<raster::ThemedEdgeUrls>>,
    is_dark: bool,
) -> Element {
    match stage {
        StageId::Edges => {
            // Use the pre-computed themed Blob URL for the Edges thumbnail.
            // The URL is resolved here (not in the component body) so the
            // clone cost is only paid when this tile actually renders.
            let cached = edge_thumb_cache.read();
            let edge_thumb_url = cached
                .as_ref()
                .map(|c| if is_dark { &c.dark_url } else { &c.light_url });
            match edge_thumb_url {
                Some(url) => {
                    rsx! {
                        img {
                            src: "{url}",
                            class: "w-full h-full object-cover",
                            alt: "Edges thumbnail",
                        }
                    }
                }
                None => rsx! {
                    div { class: "w-full h-full flex items-center justify-center text-[var(--text-disabled)] text-xs",
                        "err"
                    }
                },
            }
        }

        StageId::Grayscale | StageId::Blur => {
            let image = match stage {
                StageId::Grayscale => &staged.grayscale,
                _ => &staged.blurred,
            };

            match raster::gray_image_to_blob_url(image) {
                Ok(url) => {
                    let url_for_error = url.clone();
                    rsx! {
                        img {
                            src: "{url}",
                            class: "w-full h-full object-cover",
                            alt: "{stage.label()} thumbnail",
                            onload: move |_| raster::revoke_blob_url(&url),
                            onerror: move |_| raster::revoke_blob_url(&url_for_error),
                        }
                    }
                }
                Err(_) => rsx! {
                    div { class: "w-full h-full flex items-center justify-center text-[var(--text-disabled)] text-xs",
                        "err"
                    }
                },
            }
        }

        StageId::Contours | StageId::Simplified => {
            let polylines = match stage {
                StageId::Contours => &staged.contours,
                _ => &staged.simplified,
            };
            let w = staged.dimensions.width;
            let h = staged.dimensions.height;
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
                StageId::Masked => staged.final_polyline(),
                _ => &staged.joined,
            };
            let w = staged.dimensions.width;
            let h = staged.dimensions.height;
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
