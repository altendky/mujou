//! Web worker entry point for mujou pipeline processing.
//!
//! This crate compiles to a standalone WASM module that runs inside a
//! `Worker`. It receives image bytes and a `PipelineConfig` via
//! `postMessage`, calls `mujou_pipeline::process_staged`, and posts
//! the serialized `Result<StagedResult, PipelineError>` back.
//!
//! Running the pipeline in a worker keeps the browser's main thread
//! free for UI updates, animations, and user interaction.

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

/// Message protocol: the main thread sends a JS object with:
/// - `imageBytes`: `Uint8Array` containing the raw image file bytes
/// - `configJson`: `String` containing JSON-serialized `PipelineConfig`
/// - `generation`: `f64` generation counter (passed through to response)
///
/// The worker responds with a JS object containing:
/// - `generation`: `f64` matching the request generation
/// - `resultJson`: `String` containing JSON-serialized
///   `Result<StagedResult, PipelineError>`
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

    // Serialize the result.
    let result_json = match serde_json::to_string(&outcome) {
        Ok(json) => json,
        Err(e) => {
            post_error_response(generation, &format!("failed to serialize result: {e}"));
            return;
        }
    };

    // Post the result back to the main thread.
    let response = js_sys::Object::new();
    js_sys::Reflect::set(
        &response,
        &JsValue::from_str("generation"),
        &JsValue::from_f64(generation),
    )
    .expect_throw("failed to set generation");
    js_sys::Reflect::set(
        &response,
        &JsValue::from_str("resultJson"),
        &JsValue::from_str(&result_json),
    )
    .expect_throw("failed to set resultJson");

    let global: web_sys::DedicatedWorkerGlobalScope = js_sys::global()
        .dyn_into()
        .expect_throw("not in worker scope");
    global
        .post_message(&response)
        .expect_throw("failed to postMessage");
}

/// Post an error response back to the main thread when we can't even
/// serialize the pipeline result properly.
fn post_error_response(generation: f64, error_msg: &str) {
    let error_result: Result<mujou_pipeline::StagedResult, mujou_pipeline::PipelineError> = Err(
        mujou_pipeline::PipelineError::InvalidConfig(error_msg.to_string()),
    );

    // Best effort — if this also fails to serialize, we're in trouble,
    // but the main thread's generation counter will handle the timeout.
    if let Ok(json) = serde_json::to_string(&error_result) {
        let response = js_sys::Object::new();
        let _ = js_sys::Reflect::set(
            &response,
            &JsValue::from_str("generation"),
            &JsValue::from_f64(generation),
        );
        let _ = js_sys::Reflect::set(
            &response,
            &JsValue::from_str("resultJson"),
            &JsValue::from_str(&json),
        );

        if let Ok(global) = js_sys::global().dyn_into::<web_sys::DedicatedWorkerGlobalScope>() {
            let _ = global.post_message(&response);
        }
    }
}
