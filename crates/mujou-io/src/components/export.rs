//! Export panel component with download buttons.

use std::rc::Rc;

use crate::download;
use crate::worker::WorkerResult;
use dioxus::prelude::*;

/// Props for the [`ExportPanel`] component.
#[derive(Props, Clone)]
pub struct ExportPanelProps {
    /// The pipeline result to export. `None` disables all buttons.
    /// Wrapped in `Rc` to avoid cloning intermediate data on each render.
    result: Option<Rc<WorkerResult>>,
    /// Base filename (without extension) for downloads.
    filename: String,
}

impl PartialEq for ExportPanelProps {
    fn eq(&self, other: &Self) -> bool {
        let results_eq = match (&self.result, &other.result) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        results_eq && self.filename == other.filename
    }
}

/// Export panel with download buttons for each output format.
///
/// Currently only SVG export is functional. Other format buttons
/// are shown but disabled until their serializers are implemented.
#[component]
pub fn ExportPanel(props: ExportPanelProps) -> Element {
    let has_result = props.result.is_some();
    let mut export_error = use_signal(|| Option::<String>::None);

    // Clear stale export errors when the pipeline result changes.
    let result_present = props.result.is_some();
    use_effect(move || {
        // Subscribe to result_present so this fires on each change.
        let _ = result_present;
        export_error.set(None);
    });

    let svg_click = {
        let result = props.result.clone();
        let filename = props.filename;
        move |_| {
            if let Some(ref res) = result {
                let polyline = res.final_polyline();
                let svg = mujou_export::to_svg(std::slice::from_ref(polyline), res.dimensions);
                let download_name = format!("{filename}.svg");
                if let Err(e) = download::trigger_download(&svg, &download_name, "image/svg+xml") {
                    export_error.set(Some(format!("Download failed: {e}")));
                } else {
                    export_error.set(None);
                }
            }
        }
    };

    let enabled_class = "px-4 py-2 bg-[var(--btn-primary)] hover:bg-[var(--btn-primary-hover)] rounded text-white font-medium transition-colors cursor-pointer";
    let disabled_class =
        "px-4 py-2 bg-[var(--btn-disabled)] rounded text-[var(--text-disabled)] cursor-not-allowed";

    rsx! {
        div { class: "space-y-3",
            h3 { class: "text-lg font-semibold text-[var(--text-heading)]", "Export" }

            if let Some(ref err) = export_error() {
                p { class: "text-[var(--text-error)] text-sm", "{err}" }
            }

            div { class: "flex flex-wrap gap-2",
                button {
                    class: if has_result { enabled_class } else { disabled_class },
                    disabled: !has_result,
                    onclick: svg_click,
                    "SVG"
                }

                // Future format buttons (disabled until serializers exist)
                button {
                    class: "{disabled_class}",
                    disabled: true,
                    title: "Coming soon",
                    "THR"
                }
                button {
                    class: "{disabled_class}",
                    disabled: true,
                    title: "Coming soon",
                    "G-code"
                }
                button {
                    class: "{disabled_class}",
                    disabled: true,
                    title: "Coming soon",
                    "DXF"
                }
                button {
                    class: "{disabled_class}",
                    disabled: true,
                    title: "Coming soon",
                    "PNG"
                }
            }
        }
    }
}
