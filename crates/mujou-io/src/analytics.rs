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
///
/// # Panics (debug only)
///
/// Debug-asserts that `format` is lowercase alphanumeric/underscore and
/// that the resulting event name fits within the 200-character limit.
pub fn track_export(format: &str) {
    debug_assert!(
        format
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
        "event format must be lowercase alphanumeric or underscore, got: {format:?}"
    );
    let name = format!("export_{format}");
    debug_assert!(
        name.len() <= 200,
        "event name exceeds 200-character limit: {name:?}"
    );
    track_event(&name);
}
