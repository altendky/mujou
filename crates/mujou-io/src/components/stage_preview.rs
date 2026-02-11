//! Full-size preview for the currently selected pipeline stage.
//!
//! Dispatches between raster `<img>` display (for grayscale/blur/edges)
//! and inline SVG display (for vector stages).
//!
//! The Edges stage receives special treatment: because it is a synthetic
//! binary image (not a photograph), its colors are themed to match the
//! current light/dark mode.  Both themed Blob URLs are generated eagerly
//! when a new pipeline result arrives so that theme toggles are instant.

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
///
/// The Edges stage is themed: both light and dark Blob URLs are eagerly
/// pre-computed so theme toggles show instantly.
#[component]
pub fn StagePreview(props: StagePreviewProps) -> Element {
    let staged = &props.staged;
    let selected = props.selected;
    let w = staged.dimensions.width;
    let h = staged.dimensions.height;

    // Reactive theme signal provided by the app root.
    let is_dark: Signal<bool> = use_context();

    // Bridge the Rc prop into the reactive system via a Signal.
    // Pointer equality avoids deep comparison of GrayImage data
    // (which does not implement PartialEq).
    let mut staged_signal = use_signal(|| Rc::clone(&props.staged));
    if !Rc::ptr_eq(&props.staged, &*staged_signal.peek()) {
        staged_signal.set(Rc::clone(&props.staged));
    }

    // Eagerly cached themed Blob URLs for the Edges stage.
    // use_memo subscribes to staged_signal and recomputes when a new
    // Rc<StagedResult> arrives.
    let edge_cache = use_memo(move || {
        let staged = staged_signal();
        let ptr = Rc::as_ptr(&staged) as usize;
        raster::generate_themed_edge_urls(&staged.edges, ptr).ok()
    });

    // Eagerly cached Blob URL for the Original (RGBA) stage.
    // RGBA PNG encoding is ~4× more expensive than grayscale; caching
    // avoids re-encoding when the user toggles back to Original.
    let original_cache = use_memo(move || {
        let staged = staged_signal();
        raster::rgba_image_to_blob_url(&staged.original)
            .ok()
            .map(raster::CachedBlobUrl::new)
    });

    // Eagerly cached Blob URL for the Grayscale stage.
    let grayscale_cache = use_memo(move || {
        let staged = staged_signal();
        raster::gray_image_to_blob_url(&staged.grayscale)
            .ok()
            .map(raster::CachedBlobUrl::new)
    });

    // Eagerly cached Blob URL for the Blur stage.
    let blur_cache = use_memo(move || {
        let staged = staged_signal();
        raster::gray_image_to_blob_url(&staged.blurred)
            .ok()
            .map(raster::CachedBlobUrl::new)
    });

    match selected {
        StageId::Original => {
            // Use the eagerly cached Blob URL; the CachedBlobUrl wrapper
            // revokes it automatically when the memo recomputes.
            let cached = original_cache.read();
            cached.as_ref().map_or_else(
                || {
                    rsx! {
                        p { class: "text-[var(--text-error)] text-sm",
                            "Failed to render Original"
                        }
                    }
                },
                |c| {
                    let url = c.url().to_owned();
                    rsx! {
                        img {
                            src: "{url}",
                            class: "w-full h-auto max-h-[70vh] bg-[var(--preview-bg)] rounded object-contain",
                            alt: "Original stage preview",
                        }
                    }
                },
            )
        }

        StageId::Grayscale | StageId::Blur => {
            // Use eagerly cached Blob URLs; CachedBlobUrl revokes
            // automatically when the memo recomputes.
            let cached = match selected {
                StageId::Grayscale => grayscale_cache.read(),
                _ => blur_cache.read(),
            };
            cached.as_ref().map_or_else(
                || {
                    rsx! {
                        p { class: "text-[var(--text-error)] text-sm",
                            "Failed to render {selected}"
                        }
                    }
                },
                |c| {
                    let url = c.url().to_owned();
                    rsx! {
                        img {
                            src: "{url}",
                            class: "w-full h-auto max-h-[70vh] bg-[var(--preview-bg)] rounded object-contain",
                            alt: "{selected} stage preview",
                        }
                    }
                },
            )
        }

        StageId::Edges => {
            // Use eagerly cached themed Blob URLs for the Edges stage.
            // Both light and dark versions are generated via use_memo when
            // the pipeline result changes; toggling the theme simply selects
            // the other URL.
            let cached = edge_cache.read();
            #[allow(clippy::option_if_let_else)]
            match cached.as_ref() {
                Some(c) => {
                    let url = if is_dark() {
                        c.dark_url.clone()
                    } else {
                        c.light_url.clone()
                    };
                    rsx! {
                        img {
                            src: "{url}",
                            class: "w-full h-auto max-h-[70vh] bg-[var(--preview-bg)] rounded object-contain",
                            alt: "Edges stage preview",
                        }
                    }
                }
                None => rsx! {
                    p { class: "text-[var(--text-error)] text-sm",
                        "Edge preview not available"
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
