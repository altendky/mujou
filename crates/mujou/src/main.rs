use std::rc::Rc;

use dioxus::prelude::*;
use mujou_io::{
    ExportPanel, FileUpload, Filmstrip, PipelineWorker, StageControls, StageId, StagePreview,
    WorkerResult,
};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

/// Debounce delay in milliseconds for config changes.
///
/// After the user stops adjusting a slider/control, the pipeline re-runs
/// after this delay. Short enough to feel responsive, long enough to avoid
/// thrashing during continuous slider drags.
const CONFIG_DEBOUNCE_MS: u32 = 200;

/// Interval in milliseconds for updating the elapsed time display
/// during processing.
const ELAPSED_UPDATE_MS: u32 = 100;

/// Cherry blossoms example image bundled at compile time so the app
/// can show example output immediately on first load.
static CHERRY_BLOSSOMS: &[u8] = include_bytes!(env!("CHERRY_BLOSSOMS_PATH"));

/// Worker JS glue — compiled from `crates/mujou-worker/` by `build.rs`
/// via `wasm-pack build --target no-modules`.
static WORKER_JS: &str = include_str!(env!("WORKER_JS_PATH"));

/// Worker WASM binary — compiled from `crates/mujou-worker/` by
/// `build.rs` via `wasm-pack build --target no-modules`.
static WORKER_WASM: &[u8] = include_bytes!(env!("WORKER_WASM_PATH"));

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
    // --- Theme state ---
    // Detect current theme from the DOM and provide a reactive signal
    // so components can respond to theme changes (e.g. re-color raster
    // previews).  The JS callback is registered once via use_hook; the
    // leaked Closure ensures the callback lives for the page lifetime.
    let is_dark = use_context_provider(|| Signal::new(is_dark_from_dom()));
    use_hook(move || {
        let mut is_dark = is_dark;
        let cb = Closure::<dyn FnMut(String)>::new(move |resolved: String| {
            is_dark.set(resolved == "dark");
        });
        if let Some(window) = web_sys::window() {
            let _ = js_sys::Reflect::set(
                &window,
                &wasm_bindgen::JsValue::from_str("__mujou_theme_changed"),
                cb.as_ref().unchecked_ref(),
            );
        }
        cb.forget(); // leak — lives for the page lifetime
    });

    // --- Pipeline worker ---
    // Created once at app startup. The worker runs mujou-pipeline in a
    // dedicated web worker thread so the main thread stays responsive.
    let worker = use_hook(|| Rc::new(PipelineWorker::new(WORKER_JS, WORKER_WASM)));

    // --- Application state ---
    let mut image_bytes = use_signal(|| Some(CHERRY_BLOSSOMS.to_vec()));
    let mut filename = use_signal(|| String::from("cherry-blossoms"));
    let mut result = use_signal(|| Option::<Rc<WorkerResult>>::None);
    let mut processing = use_signal(|| false);
    let mut error = use_signal(|| Option::<String>::None);
    let mut generation = use_signal(|| 0u64);
    let mut debounce_generation = use_signal(|| 0u64);
    let mut selected_stage = use_signal(|| StageId::Masked);

    // Elapsed time tracking for the processing indicator.
    let mut elapsed_ms = use_signal(|| 0.0_f64);

    // Start timestamp for the current processing run. Shared between the
    // spawn (which sets it) and the render-complete effect (which reads
    // it for the final elapsed snapshot).
    let mut processing_start = use_signal(|| Option::<f64>::None);

    // The "committed" config is what the pipeline actually runs with.
    // Updated immediately on image upload, debounced on slider changes.
    let mut committed_config = use_signal(mujou_pipeline::PipelineConfig::default);

    // The "live" config tracks the UI controls in real-time (including
    // mid-drag slider positions). This drives the displayed values in
    // StageControls without triggering pipeline re-runs.
    let mut live_config = use_signal(mujou_pipeline::PipelineConfig::default);

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
    // Re-runs whenever image_bytes or committed_config changes.
    // Sends the work to the web worker so the main thread stays free
    // for UI updates, animations, and cancel button clicks.
    let worker_for_effect = Rc::clone(&worker);
    use_effect(move || {
        let Some(bytes) = image_bytes() else {
            return;
        };
        let cfg = committed_config();

        // Increment generation so any in-flight task from a prior
        // trigger knows it is stale and should discard its result.
        generation += 1;
        let my_generation = *generation.peek();

        processing.set(true);
        error.set(None);
        elapsed_ms.set(0.0);
        let start = js_sys::Date::now();
        processing_start.set(Some(start));

        let worker = Rc::clone(&worker_for_effect);
        spawn(async move {
            // Spawn a timer task that updates elapsed_ms every 100ms.
            // This lives alongside the worker call — both are async
            // tasks on the single-threaded WASM executor, interleaving
            // via the event loop.
            //
            // The timer watches `processing_start` and stops when it is
            // cleared (either by completion, cancellation, or a new run).
            spawn(async move {
                loop {
                    gloo_timers::future::TimeoutFuture::new(ELAPSED_UPDATE_MS).await;
                    match *processing_start.peek() {
                        Some(s) => elapsed_ms.set(js_sys::Date::now() - s),
                        None => break,
                    }
                }
            });

            // Run the pipeline in the web worker — this .await yields
            // to the browser event loop so animations and cancel clicks
            // keep working. PNG encoding also happens in the worker, so
            // the returned WorkerResult has ready-to-use Blob URLs.
            #[allow(clippy::cast_precision_loss)]
            let outcome = worker.run(&bytes, &cfg, my_generation as f64).await;

            // If another run was triggered while we were processing,
            // discard this stale result silently. The new run already
            // reset processing_start, so the timer will track the new
            // run's start time.
            if *generation.peek() != my_generation {
                return;
            }

            // Record final elapsed time and hide the overlay. Since
            // PNG encoding happened in the worker, result.set() only
            // triggers a lightweight re-render (no main-thread encoding).
            elapsed_ms.set(js_sys::Date::now() - start);
            processing.set(false);
            processing_start.set(None);

            match outcome {
                Ok(res) => {
                    result.set(Some(Rc::new(res)));
                    error.set(None);
                }
                Err(e) => {
                    error.set(Some(format!("{e}")));
                }
            }
        });
    });

    // --- Debounced config commit effect ---
    // When live_config changes (e.g. slider drag), wait for the debounce
    // period before committing. This avoids re-running the pipeline on
    // every micro-movement of a slider.
    use_effect(move || {
        let cfg = live_config();

        // Skip debounce if pipeline-relevant fields are unchanged.
        // UI-only fields like canny_max are excluded so adjusting
        // slider range alone never triggers reprocessing.
        if cfg.pipeline_eq(&committed_config.peek()) {
            return;
        }

        debounce_generation += 1;
        let my_generation = *debounce_generation.peek();

        spawn(async move {
            gloo_timers::future::TimeoutFuture::new(CONFIG_DEBOUNCE_MS).await;

            // Only commit if no newer config change has arrived during
            // the debounce period.
            if *debounce_generation.peek() == my_generation {
                committed_config.set(cfg);
            }
        });
    });

    // --- Cancel handler ---
    let worker_for_cancel = Rc::clone(&worker);
    let on_cancel = move |_| {
        worker_for_cancel.cancel();
        processing.set(false);
        processing_start.set(None);
    };

    // --- Config change handler ---
    let on_config_change = move |new_config: mujou_pipeline::PipelineConfig| {
        live_config.set(new_config);
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

        div { class: "min-h-screen bg-(--bg) text-(--text) flex flex-col overflow-x-hidden",
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
            // Extra right padding leaves room for the fixed-position theme
            // toggle (--btn-height wide at right:1rem). The calc gives a 1rem
            // gap between the upload button and the toggle, matching the
            // toggle's own offset from the viewport edge.
            header { class: "pl-6 pr-[calc(var(--btn-height)+2rem)] py-4 border-b border-(--border) flex items-start justify-between gap-4",
                div {
                    h1 { class: "text-2xl title-brand", "mujou" }
                    p { class: "text-(--muted) text-sm",
                        "Image to vector path converter for sand tables and CNC devices"
                    }
                }
                FileUpload {
                    on_upload: on_upload,
                }
            }

            // Main content area
            div { class: "flex-1 flex flex-col lg:flex-row gap-6 p-6 min-w-0",
                // Left column: Preview + Filmstrip + Controls
                div { class: "flex-1 flex flex-col gap-4 min-w-0",

                    // Show results (stale or fresh) when available.
                    // The processing indicator overlays rather than replaces.
                    if let Some(ref worker_result) = result() {
                        // Stage preview with processing overlay
                        div { class: "relative",
                            // Preview content (stays visible during re-processing)
                            div {
                                class: if processing() { "opacity-50 transition-opacity" } else { "transition-opacity" },

                                StagePreview {
                                    result: Rc::clone(worker_result),
                                    selected: selected_stage(),
                                }
                            }

                            // Processing indicator overlay
                            if processing() {
                                div { class: "absolute inset-0 flex items-center justify-center",
                                    div { class: "bg-[var(--surface)] bg-opacity-90 rounded-lg px-4 py-3 shadow flex flex-col items-center gap-2",
                                        p { class: "text-(--text-secondary) text-sm animate-pulse",
                                            "Processing... ({format_elapsed(elapsed_ms())})"
                                        }
                                        button {
                                            class: "text-xs px-3 py-1 rounded bg-(--error-bg) text-(--text-error) border border-(--error-border) hover:opacity-80 cursor-pointer",
                                            onclick: on_cancel,
                                            "Cancel"
                                        }
                                    }
                                }
                            }
                        }

                        // Filmstrip (always visible)
                        Filmstrip {
                            result: Rc::clone(worker_result),
                            selected: selected_stage(),
                            on_select: on_stage_select,
                        }

                        // Per-stage controls (always visible, never destroyed)
                        div { class: "bg-[var(--surface)] rounded p-4",
                            h3 { class: "text-sm font-semibold text-[var(--text-heading)] mb-2",
                                "{selected_stage()} Controls"
                            }
                            StageControls {
                                stage: selected_stage(),
                                config: live_config(),
                                on_config_change: on_config_change,
                            }
                        }
                    } else if processing() {
                        // First-time processing (no previous result to show)
                        div { class: "flex-1 flex items-center justify-center",
                            div { class: "flex flex-col items-center gap-3",
                                p { class: "text-(--text-secondary) text-lg animate-pulse",
                                    "Processing... ({format_elapsed(elapsed_ms())})"
                                }
                                button {
                                    class: "text-sm px-4 py-1.5 rounded bg-(--error-bg) text-(--text-error) border border-(--error-border) hover:opacity-80 cursor-pointer",
                                    onclick: on_cancel,
                                    "Cancel"
                                }
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
                div { class: "hidden lg:block lg:w-72 shrink-0",
                    ExportPanel {
                        result: result(),
                        filename: filename(),
                    }
                }
            }


        }
    }
}

/// Format elapsed milliseconds as a human-readable duration string.
///
/// Examples: "0.0s", "1.2s", "12.3s"
fn format_elapsed(ms: f64) -> String {
    let seconds = ms / 1000.0;
    format!("{seconds:.1}s")
}

/// Read the current theme from the DOM `data-theme` attribute.
fn is_dark_from_dom() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
        .and_then(|el| el.get_attribute("data-theme"))
        .is_some_and(|t| t == "dark")
}
