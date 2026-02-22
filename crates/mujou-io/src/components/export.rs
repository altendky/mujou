//! Export popup component with format checkboxes and batch download.

use std::rc::Rc;

use crate::analytics;
use crate::download;
use crate::worker::WorkerResult;
use dioxus::prelude::*;

/// Props for the [`ExportPanel`] component.
#[derive(Props, Clone)]
pub struct ExportPanelProps {
    /// The pipeline result to export. `None` disables all format checkboxes.
    /// Wrapped in `Rc` to avoid cloning intermediate data on each render.
    result: Option<Rc<WorkerResult>>,
    /// Base filename (without extension) for downloads.
    filename: String,
    /// Pre-formatted description of pipeline parameters for SVG metadata.
    /// Embedded in the `<desc>` element so exported files are distinguishable.
    config_description: String,
    /// Serialized `PipelineConfig` JSON for structured SVG metadata.
    /// Embedded in a `<metadata>` element for machine-parseable reproducibility.
    /// `None` if serialization failed — the `<metadata>` block is omitted.
    config_json: Option<String>,
    /// Border margin fraction (0.0–0.15) for SVG document layout.
    ///
    /// Passed through to [`mujou_export::document_mapping`] so the SVG
    /// drawing area is inset from the document edges.
    border_margin: f64,
    /// Controls visibility of the export popup.
    show: Signal<bool>,
}

impl PartialEq for ExportPanelProps {
    fn eq(&self, other: &Self) -> bool {
        let results_eq = match (&self.result, &other.result) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        results_eq
            && self.filename == other.filename
            && self.config_description == other.config_description
            && self.config_json == other.config_json
            && self.border_margin == other.border_margin
            && self.show == other.show
    }
}

/// Return the current local time formatted as `YYYY-MM-DD_HH-MM-SS`.
///
/// Uses `js_sys::Date` so this works in the WASM environment.
fn now_timestamp() -> String {
    let d = js_sys::Date::new_0();
    format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
        d.get_full_year(),
        d.get_month() + 1, // JS months are 0-indexed
        d.get_date(),
        d.get_hours(),
        d.get_minutes(),
        d.get_seconds(),
    )
}

/// Export popup with format checkboxes and a download button.
///
/// Renders a modal overlay (matching the info modal pattern) with
/// checkboxes for each export format. SVG and THR are functional;
/// future formats are shown but disabled. The popup dismisses on
/// backdrop click, the Cancel button, or after a successful download.
#[component]
pub fn ExportPanel(props: ExportPanelProps) -> Element {
    let mut show = props.show;

    let has_result = props.result.is_some();
    let mut svg_selected = use_signal(|| false);
    let mut thr_selected = use_signal(|| true);
    let mut export_error = use_signal(|| Option::<String>::None);

    // Clear stale export errors when the popup opens.  `show` is a
    // Signal so Dioxus tracks it as a reactive dependency — unlike a
    // plain bool, this re-fires when the value changes.
    use_effect(move || {
        if show() {
            export_error.set(None);
        }
    });

    if !show() {
        return rsx! {};
    }
    let handle_download = {
        let result = props.result.clone();
        let filename = props.filename;
        let config_description = props.config_description;
        let config_json = props.config_json;
        let border_margin = props.border_margin;
        move |_| {
            if let Some(ref res) = result {
                let timestamp = now_timestamp();

                if svg_selected() {
                    let description = format!("{config_description}\nExported: {timestamp}");
                    let metadata = mujou_export::SvgMetadata {
                        title: Some(&filename),
                        description: Some(&description),
                        config_json: config_json.as_deref(),
                    };
                    // Use the joined (pre-subsampled) path for SVG. Subsampling
                    // interpolates extra points for THR polar conversion that add
                    // no visual benefit to Cartesian SVG and inflate point count
                    // ~3x, causing compatibility issues with grounded.so.
                    let polyline = &res.joined;
                    let mapping = mujou_export::document_mapping(&res.canvas.shape, border_margin);
                    let svg =
                        mujou_export::to_svg(std::slice::from_ref(polyline), &metadata, &mapping);
                    let download_name = format!("{filename}_{timestamp}.svg");
                    if let Err(e) =
                        download::trigger_download(&svg, &download_name, "image/svg+xml")
                    {
                        export_error.set(Some(format!("SVG download failed: {e}")));
                        return;
                    }
                    analytics::track_export("svg");
                }

                if thr_selected() {
                    let thr_metadata = mujou_export::ThrMetadata {
                        title: Some(&filename),
                        description: Some(&config_description),
                        timestamp: Some(&timestamp),
                        config_json: config_json.as_deref(),
                    };
                    let polyline = res.final_polyline();
                    let thr = mujou_export::to_thr(std::slice::from_ref(polyline), &thr_metadata);
                    let download_name = format!("{filename}_{timestamp}.thr");
                    if let Err(e) = download::trigger_download(&thr, &download_name, "text/plain") {
                        export_error.set(Some(format!("THR download failed: {e}")));
                        return;
                    }
                    analytics::track_export("thr");
                }

                export_error.set(None);
            }
        }
    };

    let any_selected = svg_selected() || thr_selected();

    let label_enabled = "flex items-center gap-3 cursor-pointer";
    let label_disabled = "flex items-center gap-3 cursor-not-allowed opacity-50";

    rsx! {
        div {
            class: "fixed inset-0 z-[60] flex items-start justify-center pt-[15vh] bg-[var(--backdrop)]",
            // Escape key dismisses the modal.
            onkeydown: move |e: KeyboardEvent| {
                if e.key() == Key::Escape {
                    show.set(false);
                }
            },
            // Backdrop — click outside the card to dismiss.
            onclick: move |_| show.set(false),
            // Card — stop propagation so clicking inside doesn't dismiss.
            div {
                id: "export-dialog",
                role: "dialog",
                "aria-modal": "true",
                "aria-labelledby": "export-dialog-title",
                class: "relative z-10 w-full max-w-sm mx-4 p-6 rounded-lg shadow-lg bg-[var(--surface)] border border-[var(--border)] text-[var(--text)]",
                onclick: move |e| e.stop_propagation(),
                h2 {
                    id: "export-dialog-title",
                    class: "text-lg font-semibold mb-4 text-[var(--text-heading)]",
                    "Export"
                }

                if let Some(ref err) = export_error() {
                    p { class: "text-[var(--text-error)] text-sm mb-3", role: "alert", "{err}" }
                }

                // Format checkboxes
                div { class: "space-y-3 mb-5",
                    label {
                        class: if has_result { label_enabled } else { label_disabled },
                        r#for: "export-svg-checkbox",
                        input {
                            id: "export-svg-checkbox",
                            r#type: "checkbox",
                            checked: svg_selected(),
                            disabled: !has_result,
                            oninput: move |_| svg_selected.toggle(),
                            class: "w-4 h-4 accent-[var(--btn-primary)]",
                        }
                        span { "SVG" }
                    }

                    label {
                        class: if has_result { label_enabled } else { label_disabled },
                        r#for: "export-thr-checkbox",
                        input {
                            id: "export-thr-checkbox",
                            r#type: "checkbox",
                            checked: thr_selected(),
                            disabled: !has_result,
                            oninput: move |_| thr_selected.toggle(),
                            class: "w-4 h-4 accent-[var(--btn-primary)]",
                        }
                        span { "THR" }
                        span { class: "text-xs text-[var(--text-secondary)]", "(Sisyphus, Oasis, Dune Weaver)" }
                    }

                    // Future format checkboxes (disabled until serializers exist)
                    label { class: label_disabled,
                        input { r#type: "checkbox", disabled: true, class: "w-4 h-4" }
                        span { "G-code" }
                        span { class: "text-xs text-[var(--text-secondary)]", "(coming soon)" }
                    }
                    label { class: label_disabled,
                        input { r#type: "checkbox", disabled: true, class: "w-4 h-4" }
                        span { "DXF" }
                        span { class: "text-xs text-[var(--text-secondary)]", "(coming soon)" }
                    }
                    label { class: label_disabled,
                        input { r#type: "checkbox", disabled: true, class: "w-4 h-4" }
                        span { "PNG" }
                        span { class: "text-xs text-[var(--text-secondary)]", "(coming soon)" }
                    }
                }

                // Action buttons
                div { class: "flex gap-3 mb-5",
                    button {
                        class: "flex-1 text-sm px-4 py-1.5 rounded bg-[var(--btn-primary)] hover:bg-[var(--btn-primary-hover)] text-white cursor-pointer transition-colors disabled:bg-[var(--btn-disabled)] disabled:text-[var(--text-disabled)] disabled:cursor-not-allowed",
                        disabled: !has_result || !any_selected,
                        onclick: handle_download,
                        "Download"
                    }
                    button {
                        class: "text-sm px-4 py-1.5 rounded border border-[var(--border)] text-[var(--text)] hover:opacity-80 cursor-pointer transition-colors",
                        onclick: move |_| show.set(false),
                        "Cancel"
                    }
                }

                // Upload guidance
                div { class: "text-sm text-[var(--text-secondary)]",
                    p { class: "font-medium text-[var(--text)] mb-1",
                        "Next, upload to your table"
                    }
                    p {
                        "Oasis: use THR, upload at "
                        a {
                            href: "https://app.grounded.so",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            class: "underline text-[var(--btn-primary)] hover:opacity-80",
                            aria_label: "app.grounded.so (opens in new tab)",
                            "app.grounded.so"
                        }
                    }
                    p {
                        "Sisyphus: use THR, upload via the "
                        a {
                            href: "https://sisyphus-industries.com/",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            class: "underline text-[var(--btn-primary)] hover:opacity-80",
                            aria_label: "Sisyphus app — Sisyphus Industries website (opens in new tab)",
                            "Sisyphus app"
                        }
                    }
                    p {
                        "Dune Weaver ("
                        a {
                            href: "https://github.com/tuanchris/dune-weaver",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            class: "underline text-[var(--btn-primary)] hover:opacity-80",
                            aria_label: "Dune Weaver on GitHub (opens in new tab)",
                            "GitHub"
                        }
                        "): use THR, upload via your table\u{2019}s web UI"
                    }
                }
            }
        }
    }
}
