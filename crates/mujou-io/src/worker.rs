//! Web worker communication for off-main-thread pipeline processing.
//!
//! [`PipelineWorker`] wraps a `web_sys::Worker` running the
//! `mujou-worker` WASM module. It sends image bytes, config, and theme
//! colors to the worker via `postMessage` and receives pre-encoded PNG
//! bytes + vector data back.
//!
//! PNG encoding happens entirely in the worker. The main thread only
//! creates Blob URLs from the pre-encoded bytes — a near-instant
//! operation that doesn't block the UI.

use std::cell::RefCell;
use std::rc::Rc;

use mujou_pipeline::{Dimensions, PipelineConfig, PipelineError, Polyline};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::raster;

/// The vector (non-raster) portion of a `StagedResult`, matching the
/// worker's `VectorResult` struct.
#[derive(Serialize, Deserialize)]
struct VectorResult {
    contours: Vec<Polyline>,
    simplified: Vec<Polyline>,
    joined: Polyline,
    masked: Option<Polyline>,
    dimensions: Dimensions,
}

/// Pre-rendered pipeline result with Blob URLs ready for `<img src>`.
///
/// All raster stages are pre-encoded as PNG in the worker thread.
/// The main thread creates Blob URLs from the PNG bytes (near-instant).
/// Blob URLs are automatically revoked when this struct is dropped.
pub struct WorkerResult {
    /// Blob URL for the original RGBA image.
    pub original_url: raster::CachedBlobUrl,
    /// Blob URL for the grayscale image.
    pub grayscale_url: raster::CachedBlobUrl,
    /// Blob URL for the blurred image.
    pub blur_url: raster::CachedBlobUrl,
    /// Blob URL for edges in light theme.
    pub edges_light_url: raster::CachedBlobUrl,
    /// Blob URL for edges in dark theme.
    pub edges_dark_url: raster::CachedBlobUrl,
    /// Contour polylines.
    pub contours: Vec<Polyline>,
    /// Simplified polylines.
    pub simplified: Vec<Polyline>,
    /// Joined single polyline.
    pub joined: Polyline,
    /// Masked polyline (if circular mask was applied).
    pub masked: Option<Polyline>,
    /// Image dimensions.
    pub dimensions: Dimensions,
}

impl WorkerResult {
    /// The final output polyline: masked if a circular mask was applied,
    /// otherwise the joined path.
    #[must_use]
    pub fn final_polyline(&self) -> &Polyline {
        self.masked.as_ref().unwrap_or(&self.joined)
    }
}

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
    /// Sends image bytes, config, and theme colors to the worker,
    /// returning a future that resolves when the worker posts the
    /// result back. All PNG encoding happens in the worker; the
    /// returned [`WorkerResult`] contains ready-to-use Blob URLs.
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
    ) -> Result<WorkerResult, PipelineError> {
        let config_json = serde_json::to_string(config).map_err(|e| {
            PipelineError::InvalidConfig(format!("failed to serialize config: {e}"))
        })?;

        // Read theme colors from the DOM before dispatching to the worker.
        // The worker can't access the DOM, so we send the colors as part
        // of the request.
        let colors = raster::read_both_preview_colors().map_err(|e| {
            PipelineError::InvalidConfig(format!("failed to read theme colors: {e}"))
        })?;

        // Create a JS message object.
        let message = js_sys::Object::new();
        let image_array = js_sys::Uint8Array::from(image_bytes);
        js_sys::Reflect::set(&message, &JsValue::from_str("imageBytes"), &image_array)
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

        // Theme colors as hex strings (no # prefix).
        set_rgb(&message, "lightBg", colors.light.bg)?;
        set_rgb(&message, "lightFg", colors.light.fg)?;
        set_rgb(&message, "darkBg", colors.dark.bg)?;
        set_rgb(&message, "darkFg", colors.dark.fg)?;

        // Create a promise that resolves when the worker posts a message back.
        let result = Rc::new(RefCell::new(None::<Result<WorkerResult, PipelineError>>));
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

                let outcome = decode_response(&data);
                *result_clone.borrow_mut() = Some(outcome);
                resolve_clone.call0(&JsValue::NULL).ok();
            },
        );

        // Set up error handler.
        let onerror =
            Closure::<dyn FnMut(web_sys::ErrorEvent)>::new(move |event: web_sys::ErrorEvent| {
                let _ = reject.call1(&JsValue::NULL, &JsValue::from_str(&event.message()));
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

/// Set an RGB hex string field on a JS object.
fn set_rgb(obj: &js_sys::Object, key: &str, rgb: [u8; 3]) -> Result<(), PipelineError> {
    let hex = format!("{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
    js_sys::Reflect::set(obj, &JsValue::from_str(key), &JsValue::from_str(&hex))
        .map_err(|_| PipelineError::InvalidConfig(format!("failed to set {key}")))?;
    Ok(())
}

/// Decode a worker response into a `Result<WorkerResult, PipelineError>`.
///
/// The response contains pre-encoded PNG buffers for raster stages
/// and JSON for vector data. We create Blob URLs from the PNG bytes
/// (near-instant) and return the complete `WorkerResult`.
fn decode_response(data: &JsValue) -> Result<WorkerResult, PipelineError> {
    let ok = js_sys::Reflect::get(data, &JsValue::from_str("ok"))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if !ok {
        // Error response.
        let error_json = js_sys::Reflect::get(data, &JsValue::from_str("errorJson"))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_else(|| "\"unknown worker error\"".into());
        return serde_json::from_str::<PipelineError>(&error_json).map_or_else(
            |_| {
                Err(PipelineError::InvalidConfig(format!(
                    "worker error (raw): {error_json}"
                )))
            },
            Err,
        );
    }

    // Decode vector JSON.
    let vector_json = js_sys::Reflect::get(data, &JsValue::from_str("vectorJson"))
        .ok()
        .and_then(|v| v.as_string())
        .ok_or_else(|| PipelineError::InvalidConfig("missing vectorJson".into()))?;

    let vector: VectorResult = serde_json::from_str(&vector_json).map_err(|e| {
        PipelineError::InvalidConfig(format!("failed to deserialize vector data: {e}"))
    })?;

    // Create Blob URLs from pre-encoded PNG bytes (near-instant).
    let original_url = png_to_blob_url(data, "originalPng")?;
    let grayscale_url = png_to_blob_url(data, "grayscalePng")?;
    let blur_url = png_to_blob_url(data, "blurredPng")?;
    let edges_light_url = png_to_blob_url(data, "edgesLightPng")?;
    let edges_dark_url = png_to_blob_url(data, "edgesDarkPng")?;

    Ok(WorkerResult {
        original_url,
        grayscale_url,
        blur_url,
        edges_light_url,
        edges_dark_url,
        contours: vector.contours,
        simplified: vector.simplified,
        joined: vector.joined,
        masked: vector.masked,
        dimensions: vector.dimensions,
    })
}

/// Create a Blob URL from a pre-encoded PNG `Uint8Array` in the response.
fn png_to_blob_url(data: &JsValue, key: &str) -> Result<raster::CachedBlobUrl, PipelineError> {
    let val = js_sys::Reflect::get(data, &JsValue::from_str(key))
        .map_err(|_| PipelineError::InvalidConfig(format!("missing field: {key}")))?;
    let array: js_sys::Uint8Array = val
        .dyn_into()
        .map_err(|_| PipelineError::InvalidConfig(format!("{key} is not a Uint8Array")))?;

    let parts = js_sys::Array::new();
    parts.push(&array);

    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("image/png");
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts)
        .map_err(|e| PipelineError::InvalidConfig(format!("Blob creation failed: {e:?}")))?;

    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|e| PipelineError::InvalidConfig(format!("URL creation failed: {e:?}")))?;

    Ok(raster::CachedBlobUrl::new(url))
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
    let wasm_blob = web_sys::Blob::new_with_buffer_source_sequence_and_options(
        &wasm_blob_parts,
        &wasm_blob_opts,
    )
    .expect_throw("failed to create WASM Blob");
    let wasm_url = web_sys::Url::create_object_url_with_blob(&wasm_blob)
        .expect_throw("failed to create WASM Blob URL");

    // Create a wrapper script that:
    // 1. Queues any messages that arrive before WASM is ready
    // 2. Defines the wasm_bindgen JS glue
    // 3. Calls wasm_bindgen(wasm_url) to initialize
    // 4. After init, replays queued messages through the real handler
    let wrapper_js = format!(
        r#"// Worker wrapper — loads embedded wasm_bindgen glue and WASM blob.

// Queue messages that arrive before WASM init completes.
var _msgQueue = [];
self.onmessage = function(e) {{ _msgQueue.push(e); }};

{worker_js}

// Initialize the WASM module from the embedded blob URL.
// worker_main() (the #[wasm_bindgen(start)] function) runs during
// instantiation and sets the real onmessage handler on self.
wasm_bindgen("{wasm_url}")
    .then(function() {{
        // Replay any messages that arrived before initialization.
        var q = _msgQueue;
        _msgQueue = null;
        for (var i = 0; i < q.length; i++) {{
            self.onmessage(q[i]);
        }}
    }})
    .catch(function(e) {{ console.error("Worker WASM init failed:", e); }});
"#
    );

    // Create a Blob URL for the wrapper script.
    let js_blob_parts = js_sys::Array::new();
    js_blob_parts.push(&JsValue::from_str(&wrapper_js));
    let js_blob_opts = web_sys::BlobPropertyBag::new();
    js_blob_opts.set_type("application/javascript");
    let js_blob = web_sys::Blob::new_with_str_sequence_and_options(&js_blob_parts, &js_blob_opts)
        .expect_throw("failed to create JS Blob");
    let js_url = web_sys::Url::create_object_url_with_blob(&js_blob)
        .expect_throw("failed to create JS Blob URL");

    // Create the worker.
    let worker = web_sys::Worker::new(&js_url).expect_throw("failed to create Worker");

    // Clean up the Blob URLs (the worker has already fetched them).
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
