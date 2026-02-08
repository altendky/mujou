//! Export panel component with download buttons.

use dioxus::prelude::*;
use mujou_pipeline::ProcessResult;

use crate::download;

/// Props for the [`ExportPanel`] component.
#[derive(Props, Clone, PartialEq)]
pub struct ExportPanelProps {
    /// The pipeline result to export. `None` disables all buttons.
    result: Option<ProcessResult>,
    /// Base filename (without extension) for downloads.
    filename: String,
}

/// Export panel with download buttons for each output format.
///
/// Currently only SVG export is functional. Other format buttons
/// are shown but disabled until their serializers are implemented.
#[component]
pub fn ExportPanel(props: ExportPanelProps) -> Element {
    let has_result = props.result.is_some();
    let mut export_error = use_signal(|| Option::<String>::None);

    let svg_click = {
        let result = props.result.clone();
        let filename = props.filename;
        move |_| {
            if let Some(ref res) = result {
                let svg = mujou_export::to_svg(std::slice::from_ref(&res.polyline), res.dimensions);
                let download_name = format!("{filename}.svg");
                if let Err(e) = download::trigger_download(&svg, &download_name, "image/svg+xml") {
                    export_error.set(Some(format!("Download failed: {e}")));
                } else {
                    export_error.set(None);
                }
            }
        }
    };

    let enabled_class = "px-4 py-2 bg-blue-600 hover:bg-blue-500 rounded text-white font-medium transition-colors cursor-pointer";
    let disabled_class = "px-4 py-2 bg-gray-700 rounded text-gray-500 cursor-not-allowed";

    rsx! {
        div { class: "space-y-3",
            h3 { class: "text-lg font-semibold text-gray-300", "Export" }

            if let Some(ref err) = export_error() {
                p { class: "text-red-400 text-sm", "{err}" }
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
