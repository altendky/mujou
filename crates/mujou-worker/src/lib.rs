//! Web worker entry point for mujou pipeline processing.
//!
//! This crate compiles to a standalone WASM module that runs inside a
//! `Worker`. It receives image bytes and a `PipelineConfig` via
//! `postMessage`, calls `mujou_pipeline::process_staged`, and posts
//! the result back.
//!
//! Raster images (original, grayscale, blurred, edges) are sent as raw
//! `Uint8Array` buffers to avoid the massive overhead of JSON-encoding
//! megabytes of pixel data as number arrays. Vector data (polylines,
//! dimensions) is sent as a small JSON string.
//!
//! Running the pipeline in a worker keeps the browser's main thread
//! free for UI updates, animations, and user interaction.

use mujou_pipeline::{Dimensions, Polyline, StagedResult};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

/// The vector (non-raster) portion of a `StagedResult`, serialized as
/// JSON. Raster images are sent separately as raw `Uint8Array` buffers.
#[derive(Serialize, Deserialize)]
pub struct VectorResult {
    pub contours: Vec<Polyline>,
    pub simplified: Vec<Polyline>,
    pub joined: Polyline,
    pub masked: Option<Polyline>,
    pub dimensions: Dimensions,
}

/// Message protocol: the main thread sends a JS object with:
/// - `imageBytes`: `Uint8Array` containing the raw image file bytes
/// - `configJson`: `String` containing JSON-serialized `PipelineConfig`
/// - `generation`: `f64` generation counter (passed through to response)
///
/// On success the worker responds with a JS object containing:
/// - `generation`: `f64` matching the request generation
/// - `ok`: `true`
/// - `vectorJson`: `String` — JSON-serialized `VectorResult`
/// - `originalWidth`, `originalHeight`: `f64` — RGBA image dimensions
/// - `originalPixels`: `Uint8Array` — raw RGBA pixel data
/// - `grayscaleWidth`, `grayscaleHeight`, `grayscalePixels`
/// - `blurredWidth`, `blurredHeight`, `blurredPixels`
/// - `edgesWidth`, `edgesHeight`, `edgesPixels`
///
/// On error the worker responds with:
/// - `generation`: `f64`
/// - `ok`: `false`
/// - `errorJson`: `String` — JSON-serialized `PipelineError`
///
/// # Worker entry point
///
/// Called automatically when the WASM module is instantiated in the
/// worker context.
#[wasm_bindgen(start)]
pub fn worker_main() {
    // Get the worker global scope.
    let global: web_sys::DedicatedWorkerGlobalScope = js_sys::global()
        .dyn_into()
        .expect_throw("not running in a DedicatedWorkerGlobalScope");

    // Set up the message handler.
    let onmessage =
        Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            handle_message(event);
        });
    global.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget(); // leak — lives for the worker lifetime
}

/// Handle an incoming message from the main thread.
///
/// Extracts the image bytes and config, runs the pipeline, and posts
/// the result back.
#[allow(clippy::expect_used, clippy::needless_pass_by_value)]
fn handle_message(event: web_sys::MessageEvent) {
    let data = event.data();

    // Extract fields from the message object.
    let image_bytes_val = js_sys::Reflect::get(&data, &JsValue::from_str("imageBytes"))
        .expect_throw("missing imageBytes field");
    let config_json_val = js_sys::Reflect::get(&data, &JsValue::from_str("configJson"))
        .expect_throw("missing configJson field");
    let generation_val = js_sys::Reflect::get(&data, &JsValue::from_str("generation"))
        .expect_throw("missing generation field");

    // Convert JS types to Rust types.
    let image_bytes_js: js_sys::Uint8Array = image_bytes_val
        .dyn_into()
        .expect_throw("imageBytes is not a Uint8Array");
    let image_bytes = image_bytes_js.to_vec();

    let config_json = config_json_val
        .as_string()
        .expect_throw("configJson is not a string");
    let generation = generation_val
        .as_f64()
        .expect_throw("generation is not a number");

    // Deserialize the pipeline config.
    let config: mujou_pipeline::PipelineConfig = match serde_json::from_str(&config_json) {
        Ok(c) => c,
        Err(e) => {
            post_error_response(generation, &format!("failed to parse config: {e}"));
            return;
        }
    };

    // Run the pipeline (synchronous — blocks this worker thread only).
    let outcome = mujou_pipeline::process_staged(&image_bytes, &config);

    match outcome {
        Ok(staged) => post_success_response(generation, &staged),
        Err(e) => {
            let error_json = serde_json::to_string(&e)
                .unwrap_or_else(|ser_err| format!("\"serialization error: {ser_err}\""));
            post_error_json(generation, &error_json);
        }
    }
}

/// Post a successful pipeline result back to the main thread.
///
/// Raster images are sent as raw `Uint8Array` buffers (zero JSON
/// overhead). Vector data is sent as a small JSON string.
#[allow(clippy::expect_used)]
fn post_success_response(generation: f64, staged: &StagedResult) {
    let vector = VectorResult {
        contours: staged.contours.clone(),
        simplified: staged.simplified.clone(),
        joined: staged.joined.clone(),
        masked: staged.masked.clone(),
        dimensions: staged.dimensions,
    };

    let vector_json = match serde_json::to_string(&vector) {
        Ok(json) => json,
        Err(e) => {
            post_error_response(generation, &format!("failed to serialize vector data: {e}"));
            return;
        }
    };

    let response = js_sys::Object::new();
    let set = |key: &str, val: &JsValue| {
        js_sys::Reflect::set(&response, &JsValue::from_str(key), val)
            .expect_throw("failed to set response field");
    };

    set("generation", &JsValue::from_f64(generation));
    set("ok", &JsValue::from_bool(true));
    set("vectorJson", &JsValue::from_str(&vector_json));

    // Raster images as raw Uint8Array buffers with dimensions.
    set(
        "originalWidth",
        &JsValue::from_f64(f64::from(staged.original.width())),
    );
    set(
        "originalHeight",
        &JsValue::from_f64(f64::from(staged.original.height())),
    );
    set(
        "originalPixels",
        &js_sys::Uint8Array::from(staged.original.as_raw().as_slice()),
    );

    set(
        "grayscaleWidth",
        &JsValue::from_f64(f64::from(staged.grayscale.width())),
    );
    set(
        "grayscaleHeight",
        &JsValue::from_f64(f64::from(staged.grayscale.height())),
    );
    set(
        "grayscalePixels",
        &js_sys::Uint8Array::from(staged.grayscale.as_raw().as_slice()),
    );

    set(
        "blurredWidth",
        &JsValue::from_f64(f64::from(staged.blurred.width())),
    );
    set(
        "blurredHeight",
        &JsValue::from_f64(f64::from(staged.blurred.height())),
    );
    set(
        "blurredPixels",
        &js_sys::Uint8Array::from(staged.blurred.as_raw().as_slice()),
    );

    set(
        "edgesWidth",
        &JsValue::from_f64(f64::from(staged.edges.width())),
    );
    set(
        "edgesHeight",
        &JsValue::from_f64(f64::from(staged.edges.height())),
    );
    set(
        "edgesPixels",
        &js_sys::Uint8Array::from(staged.edges.as_raw().as_slice()),
    );

    let global: web_sys::DedicatedWorkerGlobalScope = js_sys::global()
        .dyn_into()
        .expect_throw("not in worker scope");
    global
        .post_message(&response)
        .expect_throw("failed to postMessage");
}

/// Post an error response back to the main thread.
fn post_error_response(generation: f64, error_msg: &str) {
    let error = mujou_pipeline::PipelineError::InvalidConfig(error_msg.to_string());
    let error_json = serde_json::to_string(&error).unwrap_or_else(|_| "\"unknown error\"".into());
    post_error_json(generation, &error_json);
}

/// Post a pre-serialized error JSON back to the main thread.
fn post_error_json(generation: f64, error_json: &str) {
    let response = js_sys::Object::new();
    let _ = js_sys::Reflect::set(
        &response,
        &JsValue::from_str("generation"),
        &JsValue::from_f64(generation),
    );
    let _ = js_sys::Reflect::set(
        &response,
        &JsValue::from_str("ok"),
        &JsValue::from_bool(false),
    );
    let _ = js_sys::Reflect::set(
        &response,
        &JsValue::from_str("errorJson"),
        &JsValue::from_str(error_json),
    );

    if let Ok(global) = js_sys::global().dyn_into::<web_sys::DedicatedWorkerGlobalScope>() {
        let _ = global.post_message(&response);
    }
}
