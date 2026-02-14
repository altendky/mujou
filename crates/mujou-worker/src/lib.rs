//! Web worker entry point for mujou pipeline processing.
//!
//! This crate compiles to a standalone WASM module that runs inside a
//! `Worker`. It receives image bytes, a `PipelineConfig`, and theme
//! colors via `postMessage`, runs `mujou_pipeline::process_staged`,
//! encodes all raster stages to PNG, and posts the results back.
//!
//! PNG encoding happens here (off the main thread) so the browser's
//! main thread only needs to create Blob URLs from the pre-encoded
//! bytes — a near-instant operation.
//!
//! Vector data (polylines, dimensions) is sent as a small JSON string.
//! Raster data is sent as pre-encoded PNG `Uint8Array` buffers.

use image::ImageEncoder;
use mujou_pipeline::{Dimensions, GrayImage, Polyline, StagedResult};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

/// The vector (non-raster) portion of a `StagedResult`, serialized as
/// JSON. Raster images are sent separately as pre-encoded PNG buffers.
#[derive(Serialize, Deserialize)]
pub struct VectorResult {
    pub contours: Vec<Polyline>,
    pub simplified: Vec<Polyline>,
    pub masked: Option<Vec<Polyline>>,
    pub joined: Polyline,
    pub dimensions: Dimensions,
}

/// Message protocol: the main thread sends a JS object with:
/// - `imageBytes`: `Uint8Array` containing the raw image file bytes
/// - `configJson`: `String` containing JSON-serialized `PipelineConfig`
/// - `generation`: `f64` generation counter (passed through to response)
/// - `lightBg`, `lightFg`: `String` — hex RGB for light theme (e.g. "f5f5f5,1a1a1a")
/// - `darkBg`, `darkFg`: `String` — hex RGB for dark theme
///
/// The worker responds with three types of messages, distinguished by a
/// `type` field:
///
/// **Progress** (sent after each pipeline stage completes):
/// - `type`: `"progress"`
/// - `generation`: `f64` matching the request generation
/// - `stageIndex`: `f64` — the 0-based index of the stage just reached
/// - `stageCount`: `f64` — total number of pipeline stages
///
/// **Success** (sent once when the pipeline completes):
/// - `generation`: `f64` matching the request generation
/// - `ok`: `true`
/// - `vectorJson`: `String` — JSON-serialized `VectorResult`
/// - `originalPng`: `Uint8Array` — pre-encoded RGBA PNG
/// - `downsampledPng`: `Uint8Array` — pre-encoded RGBA PNG (working resolution)
/// - `blurredPng`: `Uint8Array` — pre-encoded RGBA PNG (blurred)
/// - `edgesLightPng`: `Uint8Array` — themed edge PNG (light mode)
/// - `edgesDarkPng`: `Uint8Array` — themed edge PNG (dark mode)
///
/// **Error** (sent once on pipeline failure):
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
    console_error_panic_hook::set_once();

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
/// Extracts the image bytes, config, and theme colors, runs the
/// pipeline, encodes PNGs, and posts the result back.
#[allow(
    clippy::expect_used,
    clippy::needless_pass_by_value,
    clippy::similar_names
)]
fn handle_message(event: web_sys::MessageEvent) {
    log("worker: received message");

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

    log(&format!(
        "worker: gen={generation} image={} bytes config={config_json}",
        image_bytes.len(),
    ));

    // Extract theme colors for edge image rendering.
    let (light_bg, light_fg) = (get_rgb(&data, "lightBg"), get_rgb(&data, "lightFg"));
    let (dark_bg, dark_fg) = (get_rgb(&data, "darkBg"), get_rgb(&data, "darkFg"));

    // Deserialize the pipeline config.
    let config: mujou_pipeline::PipelineConfig = match serde_json::from_str(&config_json) {
        Ok(c) => c,
        Err(e) => {
            log(&format!("worker: config parse failed: {e}"));
            post_error_response(generation, &format!("failed to parse config: {e}"));
            return;
        }
    };
    log("worker: config parsed, running pipeline");

    // Run the pipeline (synchronous — blocks this worker thread only).
    // After each stage transition, post a progress message so the main
    // thread can update the per-stage UI.
    let outcome = (|| {
        use mujou_pipeline::pipeline::{Advance, STAGE_COUNT, Stage};

        let mut stage: Stage = mujou_pipeline::Pipeline::new(image_bytes, config).into();
        post_progress(generation, stage.index(), STAGE_COUNT);
        loop {
            match stage.advance()? {
                Advance::Next(next) => {
                    post_progress(generation, next.index(), STAGE_COUNT);
                    stage = next;
                }
                Advance::Complete(done) => break done.complete(),
            }
        }
    })();

    match outcome {
        Ok(staged) => {
            log(&format!(
                "worker: pipeline ok, {}x{}, encoding PNGs",
                staged.dimensions.width, staged.dimensions.height,
            ));
            post_success_response(generation, &staged, light_bg, light_fg, dark_bg, dark_fg);
            log("worker: response posted");
        }
        Err(e) => {
            log(&format!("worker: pipeline error: {e}"));
            let error_json = serde_json::to_string(&e).unwrap_or_else(|ser_err| {
                serde_json::to_string(&format!("serialization error: {ser_err}"))
                    .unwrap_or_else(|_| "\"unknown error\"".into())
            });
            post_error_json(generation, &error_json);
        }
    }
}

/// Log a message to the browser console.
fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(msg));
}

/// Extract an RGB color triple from a JS object field.
///
/// The field contains a hex string like `"f5f5f5"` (no `#` prefix).
/// Returns `[0, 0, 0]` if the field is missing or malformed.
#[allow(clippy::cast_possible_truncation)]
fn get_rgb(data: &JsValue, key: &str) -> [u8; 3] {
    let hex = js_sys::Reflect::get(data, &JsValue::from_str(key))
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default();
    if hex.len() != 6 {
        return [0, 0, 0];
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
    [r, g, b]
}

/// Post a successful pipeline result back to the main thread.
///
/// All raster images are pre-encoded as PNG bytes so the main thread
/// only needs to create Blob URLs (near-instant).
#[allow(clippy::expect_used)]
#[allow(clippy::similar_names)]
fn post_success_response(
    generation: f64,
    staged: &StagedResult,
    light_bg: [u8; 3],
    light_fg: [u8; 3],
    dark_bg: [u8; 3],
    dark_fg: [u8; 3],
) {
    let vector = VectorResult {
        contours: staged.contours.clone(),
        simplified: staged.simplified.clone(),
        masked: staged.masked.clone(),
        joined: staged.joined.clone(),
        dimensions: staged.dimensions,
    };

    let vector_json = match serde_json::to_string(&vector) {
        Ok(json) => json,
        Err(e) => {
            post_error_response(generation, &format!("failed to serialize vector data: {e}"));
            return;
        }
    };

    // Encode all raster stages to PNG.
    macro_rules! encode_or_error {
        ($expr:expr) => {
            match $expr {
                Ok(bytes) => bytes,
                Err(msg) => {
                    post_error_response(generation, &msg);
                    return;
                }
            }
        };
    }
    let original_png = encode_or_error!(encode_rgba_png(&staged.original));
    let downsampled_png = encode_or_error!(encode_rgba_png(&staged.downsampled));
    let blurred_png = encode_or_error!(encode_rgba_png(&staged.blurred));
    // Dilate once — both themes use the same dilated edge image.
    let dilated_edges = dilate_soft(&staged.edges);
    let edges_light_png = encode_themed_edge_png(&dilated_edges, light_bg, light_fg);
    let edges_light_png = encode_or_error!(edges_light_png);
    let edges_dark_png = encode_themed_edge_png(&dilated_edges, dark_bg, dark_fg);
    let edges_dark_png = encode_or_error!(edges_dark_png);

    let response = js_sys::Object::new();
    let set = |key: &str, val: &JsValue| {
        js_sys::Reflect::set(&response, &JsValue::from_str(key), val)
            .expect_throw("failed to set response field");
    };

    set("generation", &JsValue::from_f64(generation));
    set("ok", &JsValue::from_bool(true));
    set("vectorJson", &JsValue::from_str(&vector_json));

    // Pre-encoded PNG buffers.
    set(
        "originalPng",
        &js_sys::Uint8Array::from(original_png.as_slice()),
    );
    set(
        "downsampledPng",
        &js_sys::Uint8Array::from(downsampled_png.as_slice()),
    );
    set(
        "blurredPng",
        &js_sys::Uint8Array::from(blurred_png.as_slice()),
    );
    set(
        "edgesLightPng",
        &js_sys::Uint8Array::from(edges_light_png.as_slice()),
    );
    set(
        "edgesDarkPng",
        &js_sys::Uint8Array::from(edges_dark_png.as_slice()),
    );

    let global: web_sys::DedicatedWorkerGlobalScope = js_sys::global()
        .dyn_into()
        .expect_throw("not in worker scope");
    global
        .post_message(&response)
        .expect_throw("failed to postMessage");
}

/// Encode an RGBA image as PNG bytes.
fn encode_rgba_png(image: &mujou_pipeline::RgbaImage) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut buf);
    encoder
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| format!("RGBA PNG encode failed: {e}"))?;
    Ok(buf)
}

/// Encode a pre-dilated edge image as a themed RGB PNG.
///
/// Maps pixel values to the given foreground/background colors and
/// encodes the result as a PNG.  The caller is responsible for
/// running [`dilate_soft`] first — this avoids redundant dilation
/// when the same edge image is themed for both light and dark modes.
fn encode_themed_edge_png(
    dilated: &GrayImage,
    bg: [u8; 3],
    fg: [u8; 3],
) -> Result<Vec<u8>, String> {
    // Map grayscale pixels to RGB using bg/fg colors.
    let (w, h) = (dilated.width(), dilated.height());
    let mut rgb_buf = Vec::with_capacity((w * h * 3) as usize);
    for p in dilated.pixels() {
        let v = p.0[0];
        for c in 0..3 {
            let color = u16::from(bg[c]) * u16::from(255 - v) + u16::from(fg[c]) * u16::from(v);
            #[expect(clippy::cast_possible_truncation)]
            rgb_buf.push((color / 255) as u8);
        }
    }

    // Encode RGB buffer → PNG bytes.
    let mut buf = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut buf);
    encoder
        .write_image(&rgb_buf, w, h, image::ExtendedColorType::Rgb8)
        .map_err(|e| format!("themed edge PNG encode failed: {e}"))?;
    Ok(buf)
}

/// Soft-dilate a binary image to approximate 1.25 px stroke width.
///
/// Edge pixels (255) are kept at full intensity. Their four cardinal
/// neighbors are set to quarter intensity (64) if they are not already
/// foreground. The color mapper's linear interpolation then renders
/// those fringe pixels as a 25% blend of bg/fg, giving each 1 px
/// edge line a visual weight of roughly 1.25 px after smooth browser
/// downscaling.
fn dilate_soft(image: &GrayImage) -> GrayImage {
    let (w, h) = (image.width(), image.height());
    GrayImage::from_fn(w, h, |x, y| {
        if image.get_pixel(x, y).0[0] == 255 {
            return image::Luma([255]);
        }
        let neighbors: [(u32, u32); 4] = [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ];
        for (nx, ny) in neighbors {
            if nx < w && ny < h && image.get_pixel(nx, ny).0[0] == 255 {
                return image::Luma([64]);
            }
        }
        image::Luma([0])
    })
}

/// Post a stage-progress message back to the main thread.
///
/// Sent after each `stage.advance()` call so the main thread can update
/// the per-stage timing display. The message carries only numeric
/// indices — the main thread maps these to UI stage labels.
#[allow(clippy::cast_precision_loss)]
fn post_progress(generation: f64, stage_index: usize, stage_count: usize) {
    let msg = js_sys::Object::new();
    let _ = js_sys::Reflect::set(
        &msg,
        &JsValue::from_str("type"),
        &JsValue::from_str("progress"),
    );
    let _ = js_sys::Reflect::set(
        &msg,
        &JsValue::from_str("generation"),
        &JsValue::from_f64(generation),
    );
    let _ = js_sys::Reflect::set(
        &msg,
        &JsValue::from_str("stageIndex"),
        &JsValue::from_f64(stage_index as f64),
    );
    let _ = js_sys::Reflect::set(
        &msg,
        &JsValue::from_str("stageCount"),
        &JsValue::from_f64(stage_count as f64),
    );

    if let Ok(global) = js_sys::global().dyn_into::<web_sys::DedicatedWorkerGlobalScope>() {
        let _ = global.post_message(&msg);
    }
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
