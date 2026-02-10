//! Horizontal filmstrip of pipeline stage thumbnails.
//!
//! Displays a scrollable strip of small previews for each pipeline stage.
//! Clicking a thumbnail selects that stage for full-size preview and
//! shows its parameter controls.

use std::fmt::Write;
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
    rsx! {
        div {
            class: "flex flex-nowrap overflow-x-auto gap-2 py-2 scrollbar-thin",

            for stage in StageId::ALL {
                {render_tile(&props.staged, stage, props.selected == stage, &props.on_select)}
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
                {render_thumbnail(staged, stage)}
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
fn render_thumbnail(staged: &StagedResult, stage: StageId) -> Element {
    match stage {
        StageId::Grayscale | StageId::Blur | StageId::Edges => {
            let image = match stage {
                StageId::Grayscale => &staged.grayscale,
                StageId::Blur => &staged.blurred,
                _ => &staged.edges,
            };

            match raster::gray_image_to_blob_url(image) {
                Ok(url) => rsx! {
                    img {
                        src: "{url}",
                        class: "w-full h-full object-cover",
                        alt: "{stage.label()} thumbnail",
                        onload: move |_| raster::revoke_blob_url(&url),
                    }
                },
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
    let points = polyline.points();
    if points.len() < 2 {
        return rsx! {};
    }
    let mut d = String::new();
    for (i, p) in points.iter().enumerate() {
        let cmd = if i == 0 { "M" } else { "L" };
        let _ = write!(d, "{cmd} {:.1} {:.1} ", p.x, p.y);
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
