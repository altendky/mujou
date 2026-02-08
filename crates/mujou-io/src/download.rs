//! File download via Blob URLs.
//!
//! Dioxus has no built-in file download API.  This module triggers
//! downloads by creating a `Blob`, generating an object URL, and
//! programmatically clicking a temporary `<a>` element.
//!
//! All functions in this module require a browser environment
//! (`wasm32-unknown-unknown` target).

use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use web_sys::BlobPropertyBag;

/// Errors that can occur when triggering a file download.
#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    /// A browser API call returned an error.
    #[error("browser API error: {0}")]
    JsError(String),
}

impl From<JsValue> for DownloadError {
    fn from(value: JsValue) -> Self {
        Self::JsError(format!("{value:?}"))
    }
}

/// Trigger a file download in the browser.
///
/// Creates a `Blob` from `data`, generates an object URL, and
/// programmatically clicks a temporary `<a download="filename">` element.
/// The object URL is revoked after the click.
///
/// # Errors
///
/// Returns [`DownloadError::JsError`] if any browser API call fails
/// (e.g., `Blob` creation, `URL.createObjectURL`, element creation).
pub fn trigger_download(data: &str, filename: &str, mime_type: &str) -> Result<(), DownloadError> {
    let window =
        web_sys::window().ok_or_else(|| DownloadError::JsError("no global window".into()))?;
    let document = window
        .document()
        .ok_or_else(|| DownloadError::JsError("no document".into()))?;

    // Create a Blob from the data string.
    let parts = js_sys::Array::new();
    parts.push(&JsValue::from_str(data));

    let opts = BlobPropertyBag::new();
    opts.set_type(mime_type);

    let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &opts)?;

    // Generate an object URL for the Blob.
    let url = web_sys::Url::create_object_url_with_blob(&blob)?;

    // Create a temporary <a> element, set href and download, click it.
    let anchor: web_sys::HtmlAnchorElement = document
        .create_element("a")?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|e| DownloadError::JsError(format!("failed to cast element: {e:?}")))?;

    anchor.set_href(&url);
    anchor.set_download(filename);

    // Append to body, click, and remove.
    let body = document
        .body()
        .ok_or_else(|| DownloadError::JsError("no document body".into()))?;
    body.append_child(&anchor)?;
    anchor.click();

    // Best-effort cleanup — the download is already initiated.
    // Failures here should not be reported as "download failed".
    let _ = body.remove_child(&anchor);
    let _ = web_sys::Url::revoke_object_url(&url);

    Ok(())
}
