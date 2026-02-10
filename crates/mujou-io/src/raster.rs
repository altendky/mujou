//! Raster image encoding and Blob URL creation.
//!
//! Converts `GrayImage` data to browser-displayable Blob URLs by
//! encoding to PNG and creating object URLs via the Web API.

use image::ImageEncoder;
use mujou_pipeline::GrayImage;
use wasm_bindgen::JsValue;
use web_sys::BlobPropertyBag;

/// Errors that can occur during raster-to-Blob-URL conversion.
#[derive(Debug, thiserror::Error)]
pub enum RasterError {
    /// PNG encoding failed.
    #[error("PNG encoding failed: {0}")]
    PngEncode(String),

    /// A browser API call returned an error.
    #[error("browser API error: {0}")]
    JsError(String),
}

impl From<JsValue> for RasterError {
    fn from(value: JsValue) -> Self {
        Self::JsError(format!("{value:?}"))
    }
}

impl From<image::ImageError> for RasterError {
    fn from(err: image::ImageError) -> Self {
        Self::PngEncode(err.to_string())
    }
}

/// Encode a `GrayImage` as a PNG Blob URL for use as an `<img src>`.
///
/// The returned URL must be revoked via [`revoke_blob_url`] when no
/// longer needed to avoid memory leaks.
///
/// # Errors
///
/// Returns [`RasterError::PngEncode`] if PNG encoding fails.
/// Returns [`RasterError::JsError`] if Blob or URL creation fails.
pub fn gray_image_to_blob_url(image: &GrayImage) -> Result<String, RasterError> {
    // 1. Encode GrayImage → PNG bytes.
    let mut png_bytes = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
    encoder.write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::L8,
    )?;

    // 2. Create a Uint8Array from the PNG bytes.
    let uint8_array = js_sys::Uint8Array::from(png_bytes.as_slice());
    let parts = js_sys::Array::new();
    parts.push(&uint8_array.buffer());

    // 3. Create a Blob with image/png MIME type.
    let opts = BlobPropertyBag::new();
    opts.set_type("image/png");
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts)?;

    // 4. Generate an object URL.
    let url = web_sys::Url::create_object_url_with_blob(&blob)?;

    Ok(url)
}

/// Revoke a Blob URL previously created by [`gray_image_to_blob_url`].
///
/// Best-effort: failures are silently ignored since the URL may have
/// already been revoked or garbage collected.
pub fn revoke_blob_url(url: &str) {
    let _ = web_sys::Url::revoke_object_url(url);
}
