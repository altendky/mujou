//! Web worker communication for off-main-thread pipeline processing.
//!
//! [`PipelineWorker`] wraps a `web_sys::Worker` running the
//! `mujou-worker` WASM module. It sends image bytes + config to the
//! worker via `postMessage` and receives the serialized
//! `Result<StagedResult, PipelineError>` back.
//!
//! The worker is created from embedded JS + WASM blobs, so no extra
//! static files need to be served by the dev server.

use std::cell::RefCell;
use std::rc::Rc;

use mujou_pipeline::{PipelineConfig, PipelineError, StagedResult};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// A pipeline worker that runs `process_staged` in a dedicated web worker.
///
/// Create one at app startup and reuse it for all pipeline runs.
/// Call [`cancel`](Self::cancel) to terminate an in-progress run —
/// this kills the worker and spawns a fresh one.
pub struct PipelineWorker {
    /// The embedded JS glue for the worker (from `include_str!` in the
    /// app crate's build.rs).
    worker_js: &'static str,
    /// The embedded WASM binary for the worker (from `include_bytes!`
    /// in the app crate's build.rs).
    worker_wasm: &'static [u8],
    /// The current worker instance. Replaced on cancel.
    inner: RefCell<web_sys::Worker>,
}

impl PipelineWorker {
    /// Create a new pipeline worker from embedded JS and WASM blobs.
    ///
    /// # Panics
    ///
    /// Panics if the worker cannot be created (e.g. in a non-browser
    /// environment).
    #[must_use]
    pub fn new(worker_js: &'static str, worker_wasm: &'static [u8]) -> Self {
        let worker = create_worker(worker_js, worker_wasm);
        Self {
            worker_js,
            worker_wasm,
            inner: RefCell::new(worker),
        }
    }

    /// Run the pipeline in the worker.
    ///
    /// Sends image bytes and config to the worker, returning a future
    /// that resolves when the worker posts the result back.
    ///
    /// The `generation` parameter is passed through to the response so
    /// the caller can detect stale results.
    ///
    /// # Errors
    ///
    /// Returns a `PipelineError` if:
    /// - The config cannot be serialized
    /// - The worker fails to respond (e.g. was terminated)
    /// - The result cannot be deserialized
    #[allow(clippy::future_not_send)] // WASM is single-threaded; Send is not needed
    pub async fn run(
        &self,
        image_bytes: &[u8],
        config: &PipelineConfig,
        generation: f64,
    ) -> Result<StagedResult, PipelineError> {
        let config_json = serde_json::to_string(config).map_err(|e| {
            PipelineError::InvalidConfig(format!("failed to serialize config: {e}"))
        })?;

        // Create a JS message object: { imageBytes: Uint8Array, configJson: string, generation: f64 }
        let message = js_sys::Object::new();
        let image_array = js_sys::Uint8Array::from(image_bytes);
        js_sys::Reflect::set(
            &message,
            &JsValue::from_str("imageBytes"),
            &image_array,
        )
        .map_err(|_| PipelineError::InvalidConfig("failed to set imageBytes".into()))?;
        js_sys::Reflect::set(
            &message,
            &JsValue::from_str("configJson"),
            &JsValue::from_str(&config_json),
        )
        .map_err(|_| PipelineError::InvalidConfig("failed to set configJson".into()))?;
        js_sys::Reflect::set(
            &message,
            &JsValue::from_str("generation"),
            &JsValue::from_f64(generation),
        )
        .map_err(|_| PipelineError::InvalidConfig("failed to set generation".into()))?;

        // Create a promise that resolves when the worker posts a message back.
        let result = Rc::new(RefCell::new(None::<Result<StagedResult, PipelineError>>));
        let result_clone = Rc::clone(&result);

        let (promise, resolve, reject) = new_promise();

        // Set up the onmessage handler for this specific request.
        let resolve_clone = resolve.clone();
        let onmessage = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(
            move |event: web_sys::MessageEvent| {
                let data = event.data();

                // Extract the generation to verify this response matches our request.
                let resp_generation = js_sys::Reflect::get(&data, &JsValue::from_str("generation"))
                    .ok()
                    .and_then(|v| v.as_f64())
                    .unwrap_or(-1.0);

                if (resp_generation - generation).abs() > f64::EPSILON {
                    // Stale response — ignore it.
                    return;
                }

                let result_json = js_sys::Reflect::get(&data, &JsValue::from_str("resultJson"))
                    .ok()
                    .and_then(|v| v.as_string());

                let outcome = result_json.map_or_else(
                    || {
                        Err(PipelineError::InvalidConfig(
                            "worker response missing resultJson".into(),
                        ))
                    },
                    |json| {
                        serde_json::from_str::<Result<StagedResult, PipelineError>>(&json)
                            .unwrap_or_else(|e| {
                                Err(PipelineError::InvalidConfig(format!(
                                    "failed to deserialize worker result: {e}"
                                )))
                            })
                    },
                );

                *result_clone.borrow_mut() = Some(outcome);
                resolve_clone.call0(&JsValue::NULL).ok();
            },
        );

        // Set up error handler.
        let onerror =
            Closure::<dyn FnMut(web_sys::ErrorEvent)>::new(move |event: web_sys::ErrorEvent| {
                let _ = reject.call1(
                    &JsValue::NULL,
                    &JsValue::from_str(&event.message()),
                );
            });

        {
            let worker = self.inner.borrow();
            worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
            worker.set_onerror(Some(onerror.as_ref().unchecked_ref()));

            // Post the message to the worker.
            worker
                .post_message(&message)
                .map_err(|_| PipelineError::InvalidConfig("failed to postMessage".into()))?;
        }

        // Prevent closures from being dropped while we await.
        // They will be cleaned up when the future completes.
        let _onmessage_guard = onmessage;
        let _onerror_guard = onerror;

        // Await the promise — this yields to the browser event loop.
        let await_result = wasm_bindgen_futures::JsFuture::from(promise).await;

        // Clean up the handlers.
        {
            let worker = self.inner.borrow();
            worker.set_onmessage(None);
            worker.set_onerror(None);
        }

        match await_result {
            Ok(_) => result
                .borrow_mut()
                .take()
                .unwrap_or(Err(PipelineError::InvalidConfig(
                    "worker completed but no result captured".into(),
                ))),
            Err(e) => {
                let msg = e
                    .as_string()
                    .unwrap_or_else(|| "unknown worker error".into());
                Err(PipelineError::InvalidConfig(format!("worker error: {msg}")))
            }
        }
    }

    /// Cancel any in-progress pipeline run by terminating the worker
    /// and creating a fresh one.
    ///
    /// This is instant — the worker is killed immediately regardless
    /// of what stage the pipeline is in.
    pub fn cancel(&self) {
        self.inner.borrow().terminate();
        let new_worker = create_worker(self.worker_js, self.worker_wasm);
        *self.inner.borrow_mut() = new_worker;
    }
}

/// Create a web worker from embedded JS glue and WASM binary.
///
/// 1. Creates a Blob URL for the WASM binary
/// 2. Wraps the JS glue in a self-initializing script that loads the
///    WASM from the Blob URL
/// 3. Creates a Blob URL for the wrapper script
/// 4. Creates a Worker from the wrapper Blob URL
fn create_worker(worker_js: &str, worker_wasm: &[u8]) -> web_sys::Worker {
    // Create a Blob URL for the WASM binary.
    let wasm_array = js_sys::Uint8Array::from(worker_wasm);
    let wasm_blob_parts = js_sys::Array::new();
    wasm_blob_parts.push(&wasm_array.buffer());
    let wasm_blob_opts = web_sys::BlobPropertyBag::new();
    wasm_blob_opts.set_type("application/wasm");
    let wasm_blob =
        web_sys::Blob::new_with_buffer_source_sequence_and_options(&wasm_blob_parts, &wasm_blob_opts)
            .expect_throw("failed to create WASM Blob");
    let wasm_url =
        web_sys::Url::create_object_url_with_blob(&wasm_blob).expect_throw("failed to create WASM Blob URL");

    // Create a wrapper script that:
    // 1. Defines the wasm_bindgen JS glue
    // 2. Calls wasm_bindgen(wasm_url) to initialize
    let wrapper_js = format!(
        r#"// Worker wrapper — loads embedded wasm_bindgen glue and WASM blob.
{worker_js}

// Initialize the WASM module from the embedded blob URL.
wasm_bindgen("{wasm_url}")
    .catch(function(e) {{ console.error("Worker WASM init failed:", e); }});
"#
    );

    // Create a Blob URL for the wrapper script.
    let js_blob_parts = js_sys::Array::new();
    js_blob_parts.push(&JsValue::from_str(&wrapper_js));
    let js_blob_opts = web_sys::BlobPropertyBag::new();
    js_blob_opts.set_type("application/javascript");
    let js_blob =
        web_sys::Blob::new_with_str_sequence_and_options(&js_blob_parts, &js_blob_opts)
            .expect_throw("failed to create JS Blob");
    let js_url =
        web_sys::Url::create_object_url_with_blob(&js_blob).expect_throw("failed to create JS Blob URL");

    // Create the worker.
    let worker = web_sys::Worker::new(&js_url).expect_throw("failed to create Worker");

    // Clean up the Blob URLs (the worker has already fetched them).
    // Note: we revoke the JS URL but keep the WASM URL alive since
    // the worker's async init may still be fetching it. The WASM URL
    // will be leaked but is small (just a blob: reference).
    web_sys::Url::revoke_object_url(&js_url).ok();

    worker
}

/// Create a JS Promise along with its resolve and reject functions.
fn new_promise() -> (js_sys::Promise, js_sys::Function, js_sys::Function) {
    let resolve = Rc::new(RefCell::new(None::<js_sys::Function>));
    let reject = Rc::new(RefCell::new(None::<js_sys::Function>));
    let resolve_clone = Rc::clone(&resolve);
    let reject_clone = Rc::clone(&reject);

    let promise = js_sys::Promise::new(&mut move |res, rej| {
        *resolve_clone.borrow_mut() = Some(res);
        *reject_clone.borrow_mut() = Some(rej);
    });

    let resolve_fn = resolve
        .borrow_mut()
        .take()
        .expect_throw("resolve not captured");
    let reject_fn = reject
        .borrow_mut()
        .take()
        .expect_throw("reject not captured");

    (promise, resolve_fn, reject_fn)
}
