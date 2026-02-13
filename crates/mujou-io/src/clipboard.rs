//! Clipboard read/write via the browser Clipboard API.
//!
//! Provides async helpers for copying text to and reading text from
//! the system clipboard.  All functions require a browser environment
//! (`wasm32-unknown-unknown` target) and a user-gesture context
//! (i.e., called from a click handler).

use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

/// Errors that can occur when accessing the clipboard.
#[derive(Debug, thiserror::Error)]
pub enum ClipboardError {
    /// A browser API call returned an error or a required object was missing.
    #[error("clipboard API error: {0}")]
    JsError(String),
}

impl From<JsValue> for ClipboardError {
    fn from(value: JsValue) -> Self {
        Self::JsError(format!("{value:?}"))
    }
}

/// Copy `text` to the system clipboard.
///
/// Wraps [`navigator.clipboard.writeText()`][mdn].
///
/// # Errors
///
/// Returns [`ClipboardError::JsError`] if the browser window, navigator,
/// or clipboard object is unavailable, or if the write operation fails
/// (e.g., the page does not have clipboard-write permission).
///
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/Clipboard/writeText
#[allow(clippy::future_not_send)] // WASM is single-threaded; Clipboard is !Send
pub async fn write_text(text: &str) -> Result<(), ClipboardError> {
    let clipboard = get_clipboard()?;
    let promise = clipboard.write_text(text);
    JsFuture::from(promise).await?;
    Ok(())
}

/// Read text from the system clipboard.
///
/// Wraps [`navigator.clipboard.readText()`][mdn].
///
/// # Errors
///
/// Returns [`ClipboardError::JsError`] if the browser window, navigator,
/// or clipboard object is unavailable, or if the read operation fails
/// (e.g., the page does not have clipboard-read permission).
///
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/Clipboard/readText
#[allow(clippy::future_not_send)] // WASM is single-threaded; Clipboard is !Send
pub async fn read_text() -> Result<String, ClipboardError> {
    let clipboard = get_clipboard()?;
    let promise = clipboard.read_text();
    let value = JsFuture::from(promise).await?;
    value
        .as_string()
        .ok_or_else(|| ClipboardError::JsError("readText() did not return a string".into()))
}

/// Obtain the `Clipboard` object from `window.navigator.clipboard`.
fn get_clipboard() -> Result<web_sys::Clipboard, ClipboardError> {
    let window =
        web_sys::window().ok_or_else(|| ClipboardError::JsError("no global window".into()))?;
    let navigator = window.navigator();
    let clipboard = navigator.clipboard();
    Ok(clipboard)
}
