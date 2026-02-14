//! Export popup component with format checkboxes and batch download.

use std::rc::Rc;

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
        results_eq && self.filename == other.filename && self.show == other.show
    }
}

/// Export popup with format checkboxes and a download button.
///
/// Renders a modal overlay (matching the info modal pattern) with
/// checkboxes for each export format. Currently only SVG is functional;
/// future formats are shown but disabled. The popup dismisses on
/// backdrop click, the Cancel button, or after a successful download.
#[component]
pub fn ExportPanel(props: ExportPanelProps) -> Element {
    let mut show = props.show;

    let has_result = props.result.is_some();
    let mut svg_selected = use_signal(|| true);
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
        move |_| {
            if let Some(ref res) = result {
                if svg_selected() {
                    let polyline = res.final_polyline();
                    let svg = mujou_export::to_svg(std::slice::from_ref(polyline), res.dimensions);
                    let download_name = format!("{filename}.svg");
                    if let Err(e) =
                        download::trigger_download(&svg, &download_name, "image/svg+xml")
                    {
                        export_error.set(Some(format!("Download failed: {e}")));
                        return;
                    }
                }
                export_error.set(None);
            }
        }
    };

    let any_selected = svg_selected();

    let label_enabled = "flex items-center gap-3 cursor-pointer";
    let label_disabled = "flex items-center gap-3 cursor-not-allowed opacity-50";

    rsx! {
        div {
            class: "fixed inset-0 z-[60] flex items-start justify-center pt-[15vh]",
            // Backdrop — click outside the card to dismiss.
            onclick: move |_| show.set(false),
            // Card — stop propagation so clicking inside doesn't dismiss.
            div {
                class: "relative z-10 w-full max-w-sm mx-4 p-6 rounded-lg shadow-lg bg-[var(--surface)] border border-[var(--border)] text-[var(--text)]",
                onclick: move |e| e.stop_propagation(),
                h2 { class: "text-lg font-semibold mb-4 text-[var(--text-heading)]",
                    "Export"
                }

                if let Some(ref err) = export_error() {
                    p { class: "text-[var(--text-error)] text-sm mb-3", "{err}" }
                }

                // Format checkboxes
                div { class: "space-y-3 mb-5",
                    label {
                        class: if has_result { label_enabled } else { label_disabled },
                        input {
                            r#type: "checkbox",
                            checked: svg_selected(),
                            disabled: !has_result,
                            oninput: move |_| svg_selected.toggle(),
                            class: "w-4 h-4 accent-[var(--btn-primary)]",
                        }
                        span { "SVG" }
                    }

                    // Future format checkboxes (disabled until serializers exist)
                    label { class: label_disabled,
                        input { r#type: "checkbox", disabled: true, class: "w-4 h-4" }
                        span { "THR" }
                        span { class: "text-xs text-[var(--text-secondary)]", "(coming soon)" }
                    }
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
                        "Oasis Mini / One: use SVG, upload at "
                        a {
                            href: "https://app.grounded.so",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            class: "underline text-[var(--btn-primary)] hover:opacity-80",
                            "app.grounded.so"
                        }
                    }
                }
            }
        }
    }
}
