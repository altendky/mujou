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
    parts.push(&uint8_array);

    // 3. Create a Blob with image/png MIME type.
    let opts = BlobPropertyBag::new();
    opts.set_type("image/png");
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts)?;

    // 4. Generate an object URL.
    let url = web_sys::Url::create_object_url_with_blob(&blob)?;

    Ok(url)
}

/// Encode a binary `GrayImage` as a themed RGB PNG Blob URL.
///
/// Maps grayscale pixel values to the given foreground/background colors:
/// - Pixel value 0 (background) → `bg`
/// - Pixel value 255 (foreground) → `fg`
///
/// Designed for binary images (Canny edge maps) where every pixel is
/// either 0 or 255.  Intermediate values are mapped proportionally.
///
/// The returned URL must be revoked via [`revoke_blob_url`] when no
/// longer needed to avoid memory leaks.
///
/// # Errors
///
/// Returns [`RasterError::PngEncode`] if PNG encoding fails.
/// Returns [`RasterError::JsError`] if Blob or URL creation fails.
pub fn themed_gray_image_to_blob_url(
    image: &GrayImage,
    bg: [u8; 3],
    fg: [u8; 3],
) -> Result<String, RasterError> {
    // 1. Map grayscale pixels to RGB using bg/fg colors.
    let (w, h) = (image.width(), image.height());
    let mut rgb_buf = Vec::with_capacity((w * h * 3) as usize);
    for p in image.pixels() {
        let v = p.0[0];
        // For binary images (0 or 255) this reduces to selecting bg or fg.
        // For intermediate values, interpolate linearly.
        for c in 0..3 {
            let color = u16::from(bg[c]) * u16::from(255 - v) + u16::from(fg[c]) * u16::from(v);
            #[expect(clippy::cast_possible_truncation)]
            rgb_buf.push((color / 255) as u8);
        }
    }

    // 2. Encode RGB buffer → PNG bytes.
    let mut png_bytes = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
    encoder.write_image(&rgb_buf, w, h, image::ExtendedColorType::Rgb8)?;

    // 3. Create Blob URL (same pattern as gray_image_to_blob_url).
    let uint8_array = js_sys::Uint8Array::from(png_bytes.as_slice());
    let parts = js_sys::Array::new();
    parts.push(&uint8_array);

    let opts = BlobPropertyBag::new();
    opts.set_type("image/png");
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts)?;

    let url = web_sys::Url::create_object_url_with_blob(&blob)?;
    Ok(url)
}

/// RGB color parsed from a CSS hex color value.
type Rgb = [u8; 3];

/// Preview background and foreground (stroke) colors for one theme.
#[derive(Debug, Clone, Copy)]
pub struct PreviewColors {
    /// Background color (`--preview-bg`).
    pub bg: Rgb,
    /// Stroke/foreground color (`--preview-stroke`).
    pub fg: Rgb,
}

/// Preview colors for both light and dark themes.
#[derive(Debug, Clone, Copy)]
pub struct BothPreviewColors {
    /// Colors when `data-theme="light"`.
    pub light: PreviewColors,
    /// Colors when `data-theme="dark"`.
    pub dark: PreviewColors,
}

/// Read `--preview-bg` and `--preview-stroke` from the document's
/// computed style.
///
/// # Errors
///
/// Returns [`RasterError::JsError`] if browser APIs fail or if the
/// CSS values cannot be parsed as hex colors.
pub fn read_preview_colors() -> Result<PreviewColors, RasterError> {
    let window =
        web_sys::window().ok_or_else(|| RasterError::JsError("no global window".into()))?;
    let doc = window
        .document()
        .ok_or_else(|| RasterError::JsError("no document".into()))?;
    let el = doc
        .document_element()
        .ok_or_else(|| RasterError::JsError("no document element".into()))?;
    let style = window
        .get_computed_style(&el)?
        .ok_or_else(|| RasterError::JsError("no computed style".into()))?;

    let bg = parse_css_hex(&style.get_property_value("--preview-bg")?)?;
    let fg = parse_css_hex(&style.get_property_value("--preview-stroke")?)?;
    Ok(PreviewColors { bg, fg })
}

/// Read preview colors for **both** light and dark themes.
///
/// Reads the current theme's colors normally, then temporarily swaps
/// the `data-theme` attribute to read the other theme's values and
/// restores the original.  No visual flash occurs because browsers do
/// not repaint during synchronous JavaScript execution.
///
/// # Errors
///
/// Returns [`RasterError::JsError`] if browser APIs fail or CSS
/// values cannot be parsed.
pub fn read_both_preview_colors() -> Result<BothPreviewColors, RasterError> {
    let window =
        web_sys::window().ok_or_else(|| RasterError::JsError("no global window".into()))?;
    let doc = window
        .document()
        .ok_or_else(|| RasterError::JsError("no document".into()))?;
    let el = doc
        .document_element()
        .ok_or_else(|| RasterError::JsError("no document element".into()))?;

    let current_theme = el.get_attribute("data-theme").unwrap_or_default();
    let other_theme = if current_theme == "dark" {
        "light"
    } else {
        "dark"
    };

    // Read current theme's colors.
    let current_colors = read_preview_colors()?;

    // Temporarily swap to the other theme and read its colors.
    el.set_attribute("data-theme", other_theme)?;
    let other_colors = read_preview_colors();
    // Always restore, even if reading failed.
    let _ = el.set_attribute("data-theme", &current_theme);
    let other_colors = other_colors?;

    if current_theme == "dark" {
        Ok(BothPreviewColors {
            light: other_colors,
            dark: current_colors,
        })
    } else {
        Ok(BothPreviewColors {
            light: current_colors,
            dark: other_colors,
        })
    }
}

/// Parse a CSS hex color string (e.g. `"#1a1a1a"` or `" #fff "`) into
/// an `[u8; 3]` RGB triple.
fn parse_css_hex(s: &str) -> Result<Rgb, RasterError> {
    let s = s.trim();
    let hex = s
        .strip_prefix('#')
        .ok_or_else(|| RasterError::JsError(format!("not a hex color: {s:?}")))?;
    match hex.len() {
        3 => {
            // Short form: #rgb → #rrggbb
            let mut rgb = [0u8; 3];
            for (i, ch) in hex.chars().enumerate() {
                let n = ch
                    .to_digit(16)
                    .ok_or_else(|| RasterError::JsError(format!("invalid hex char: {ch}")))?;
                #[expect(clippy::cast_possible_truncation)]
                {
                    rgb[i] = (n * 17) as u8;
                }
            }
            Ok(rgb)
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16);
            let g = u8::from_str_radix(&hex[2..4], 16);
            let b = u8::from_str_radix(&hex[4..6], 16);
            match (r, g, b) {
                (Ok(r), Ok(g), Ok(b)) => Ok([r, g, b]),
                _ => Err(RasterError::JsError(format!("invalid hex color: {s:?}"))),
            }
        }
        _ => Err(RasterError::JsError(format!(
            "unexpected hex length: {s:?}"
        ))),
    }
}

/// Revoke a Blob URL previously created by [`gray_image_to_blob_url`]
/// or [`themed_gray_image_to_blob_url`].
///
/// Best-effort: failures are silently ignored since the URL may have
/// already been revoked or garbage collected.
pub fn revoke_blob_url(url: &str) {
    let _ = web_sys::Url::revoke_object_url(url);
}
