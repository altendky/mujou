use std::rc::Rc;

use dioxus::prelude::*;
use mujou_io::stage::StageId;
use mujou_io::{ExportPanel, FileUpload, Filmstrip, StageControls, StagePreview};

fn main() {
    dioxus::launch(app);
}

/// Root application component.
///
/// Manages the core application state via Dioxus signals and wires
/// together the upload, filmstrip, stage preview, stage controls, and
/// export components.
#[allow(clippy::too_many_lines)]
fn app() -> Element {
    // --- Application state ---
    let mut image_bytes = use_signal(|| Option::<Vec<u8>>::None);
    let mut filename = use_signal(|| String::from("output"));
    let mut result = use_signal(|| Option::<Rc<mujou_pipeline::StagedResult>>::None);
    let mut processing = use_signal(|| false);
    let mut error = use_signal(|| Option::<String>::None);
    let mut generation = use_signal(|| 0u64);
    let mut config = use_signal(mujou_pipeline::PipelineConfig::default);
    let mut selected_stage = use_signal(|| StageId::Masked);

    // --- File upload handler ---
    let on_upload = move |(bytes, name): (Vec<u8>, String)| {
        // Strip extension for the export filename.
        let base_name = name
            .rsplit_once('.')
            .map_or(name.as_str(), |(base, _)| base)
            .to_owned();
        filename.set(base_name);
        result.set(None);
        error.set(None);
        image_bytes.set(Some(bytes));
    };

    // --- Pipeline processing effect ---
    // Re-runs whenever image_bytes or config changes.
    // Spawns an async task so the "Processing..." indicator renders
    // before the heavy synchronous pipeline work blocks the thread.
    use_effect(move || {
        let Some(bytes) = image_bytes() else {
            return;
        };
        let cfg = config();

        // Increment generation so any in-flight task from a prior
        // trigger knows it is stale and should discard its result.
        generation += 1;
        let my_generation = *generation.peek();

        processing.set(true);
        error.set(None);

        spawn(async move {
            // Yield to the browser event loop so it can paint the
            // "Processing..." state before we block on the pipeline.
            gloo_timers::future::TimeoutFuture::new(0).await;

            let outcome = mujou_pipeline::process_staged(&bytes, &cfg);

            // If another run was triggered while we were processing,
            // discard this stale result silently.
            if *generation.peek() != my_generation {
                return;
            }

            match outcome {
                Ok(res) => {
                    result.set(Some(Rc::new(res)));
                    error.set(None);
                }
                Err(e) => {
                    error.set(Some(format!("{e}")));
                    // Keep the previous result visible if one exists.
                }
            }

            processing.set(false);
        });
    });

    // --- Config change handler ---
    let on_config_change = move |new_config: mujou_pipeline::PipelineConfig| {
        config.set(new_config);
    };

    // --- Stage select handler ---
    let on_stage_select = move |stage: StageId| {
        selected_stage.set(stage);
    };

    // --- Layout ---
    rsx! {
        // Tailwind CSS utilities — compiled by build.rs via npx @tailwindcss/cli.
        // See: https://github.com/altendky/mujou/issues/12
        style { dangerous_inner_html: include_str!(env!("TAILWIND_CSS_PATH")) }

        // Shared theme (CSS variables + toggle button styles) — copied from
        // site/theme.css by build.rs to avoid fragile ../../../ paths.
        style { dangerous_inner_html: include_str!(env!("THEME_CSS_PATH")) }

        // Google Fonts — Noto Sans (Latin) and Noto Sans JP for the title
        // wordmark.  See follow-up issue to self-host for offline/privacy.
        link { rel: "preconnect", href: "https://fonts.googleapis.com" }
        link { rel: "preconnect", href: "https://fonts.gstatic.com", crossorigin: "anonymous" }
        link {
            rel: "stylesheet",
            href: "https://fonts.googleapis.com/css2?family=Noto+Sans:wght@400&family=Noto+Sans+JP:wght@400&display=swap",
        }

        div { class: "min-h-screen bg-(--bg) text-(--text) flex flex-col",
            // Theme toggle (fixed-positioned via shared theme.css;
            // content injected by shared theme-toggle.js)
            button {
                class: "theme-toggle",
                aria_label: "Toggle theme",
            }
            // Theme toggle logic — must come after the button so the
            // button is in the DOM when the script's init() runs.
            // Copied from site/theme-toggle.js by build.rs.
            script { dangerous_inner_html: include_str!(env!("THEME_TOGGLE_JS_PATH")) }

            // Header
            header { class: "px-6 py-4 border-b border-(--border)",
                h1 { class: "text-2xl title-brand", "mujou" }
                p { class: "text-(--muted) text-sm",
                    "Image to vector path converter for sand tables and CNC devices"
                }
            }

            // Main content area
            div { class: "flex-1 flex flex-col lg:flex-row gap-6 p-6",
                // Left column: Preview + Filmstrip + Controls
                div { class: "flex-1 flex flex-col gap-4",
                    // Stage preview
                    if processing() {
                        div { class: "flex-1 flex items-center justify-center",
                            p { class: "text-(--text-secondary) text-lg animate-pulse",
                                "Processing..."
                            }
                        }
                    } else if let Some(ref staged) = result() {
                        // Full-size stage preview
                        StagePreview {
                            staged: Rc::clone(staged),
                            selected: selected_stage(),
                        }

                        // Filmstrip
                        Filmstrip {
                            staged: Rc::clone(staged),
                            selected: selected_stage(),
                            on_select: on_stage_select,
                        }

                        // Per-stage controls
                        div { class: "bg-[var(--surface)] rounded p-4",
                            h3 { class: "text-sm font-semibold text-[var(--text-heading)] mb-2",
                                "{selected_stage()} Controls"
                            }
                            StageControls {
                                stage: selected_stage(),
                                config: config(),
                                on_config_change: on_config_change,
                            }
                        }
                    } else if image_bytes().is_some() {
                        div { class: "flex-1 flex items-center justify-center",
                            p { class: "text-(--muted) text-lg",
                                "Processing failed"
                            }
                        }
                    } else {
                        div { class: "flex-1 flex items-center justify-center",
                            p { class: "text-(--text-placeholder) text-lg",
                                "Upload an image to get started"
                            }
                        }
                    }

                    // Error display
                    if let Some(ref err) = error() {
                        div { class: "bg-(--error-bg) border border-(--error-border) rounded p-3",
                            p { class: "text-(--text-error) text-sm", "{err}" }
                        }
                    }

                    // Export panel (inline on mobile/tablet)
                    div { class: "lg:hidden",
                        ExportPanel {
                            result: result(),
                            filename: filename(),
                        }
                    }
                }

                // Right sidebar: Export (desktop only)
                div { class: "hidden lg:block lg:w-72 flex-shrink-0",
                    ExportPanel {
                        result: result(),
                        filename: filename(),
                    }
                }
            }

            // Footer: Upload zone
            div { class: "px-6 pb-6",
                FileUpload {
                    on_upload: on_upload,
                }
            }
        }
    }
}
