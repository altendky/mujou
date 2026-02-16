//! Lightweight Simple Analytics event tracking.
//!
//! Calls the global `sa_event` function injected by the Simple
//! Analytics `<script>` tag.  All functions silently no-op when the
//! script is absent (e.g., blocked by an ad-blocker or during tests).
//!
//! Event names follow Simple Analytics conventions: lowercase
//! alphanumeric with underscores, max 200 characters.

use wasm_bindgen::prelude::*;

/// Fire a Simple Analytics custom event.
///
/// Silently does nothing when the analytics script is absent.
fn track_event(name: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(func) = js_sys::Reflect::get(&window, &JsValue::from_str("sa_event")) else {
        return;
    };
    if !func.is_function() {
        return;
    }
    let func: js_sys::Function = func.unchecked_into();
    let _ = func.call1(&JsValue::NULL, &JsValue::from_str(name));
}

/// Record a successful file export with the given format (e.g., `"svg"`).
///
/// Fires an event named `export_<format>` (e.g., `export_svg`).
pub fn track_export(format: &str) {
    track_event(&format!("export_{format}"));
}
