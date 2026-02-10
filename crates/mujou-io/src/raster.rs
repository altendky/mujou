//! Raster image encoding and Blob URL creation.
//!
//! Converts `GrayImage` data to browser-displayable Blob URLs by
//! encoding to PNG and creating object URLs via the Web API.

use image::ImageEncoder;
use mujou_pipeline::{GrayImage, RgbaImage};
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
    let mut png_bytes = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
    encoder.write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::L8,
    )?;
    png_bytes_to_blob_url(&png_bytes)
}

/// Encode an `RgbaImage` as a PNG Blob URL for use as an `<img src>`.
///
/// The returned URL must be revoked via [`revoke_blob_url`] when no
/// longer needed to avoid memory leaks.
///
/// # Errors
///
/// Returns [`RasterError::PngEncode`] if PNG encoding fails.
/// Returns [`RasterError::JsError`] if Blob or URL creation fails.
pub fn rgba_image_to_blob_url(image: &RgbaImage) -> Result<String, RasterError> {
    let mut png_bytes = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
    encoder.write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::Rgba8,
    )?;
    png_bytes_to_blob_url(&png_bytes)
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
    // 0. Soft-dilate edge pixels so thin lines survive smooth downscaling.
    let dilated = dilate_soft(image);

    // 1. Map grayscale pixels to RGB using bg/fg colors.
    let (w, h) = (dilated.width(), dilated.height());
    let mut rgb_buf = Vec::with_capacity((w * h * 3) as usize);
    for p in dilated.pixels() {
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

    png_bytes_to_blob_url(&png_bytes)
}

/// Soft-dilate a binary image to approximate 1.25 px stroke width.
///
/// Edge pixels (255) are kept at full intensity.  Their four cardinal
/// neighbors are set to quarter intensity (64) if they are not already
/// foreground.  The color mapper's linear interpolation then renders
/// those fringe pixels as a 25 % blend of bg/fg, giving each 1 px
/// edge line a visual weight of roughly 1.25 px after smooth browser
/// downscaling.
///
/// Only intended for binary images (values 0 and 255).
fn dilate_soft(image: &GrayImage) -> GrayImage {
    let (w, h) = (image.width(), image.height());
    GrayImage::from_fn(w, h, |x, y| {
        // Already a foreground pixel — keep at full intensity.
        if image.get_pixel(x, y).0[0] == 255 {
            return image::Luma([255]);
        }
        // Check cardinal neighbors for a foreground pixel.
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

    let current_theme = el.get_attribute("data-theme");
    let other_theme = if current_theme.as_deref() == Some("dark") {
        "light"
    } else {
        "dark"
    };

    // Read current theme's colors.
    let current_colors = read_preview_colors()?;

    // Temporarily swap to the other theme and read its colors.
    el.set_attribute("data-theme", other_theme)?;
    // Force synchronous style recalculation — reading a layout property
    // makes the browser flush pending style changes before we query
    // computed CSS custom properties.
    let html: &web_sys::HtmlElement = wasm_bindgen::JsCast::unchecked_ref(&el);
    let _ = html.offset_height();
    let other_colors = read_preview_colors();
    // Always restore, even if reading failed.
    match &current_theme {
        Some(t) => {
            let _ = el.set_attribute("data-theme", t);
        }
        None => {
            let _ = el.remove_attribute("data-theme");
        }
    }
    let other_colors = other_colors?;

    if current_theme.as_deref() == Some("dark") {
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
///
/// Accepts 3-digit (`#rgb`), 4-digit (`#rgba`), 6-digit (`#rrggbb`),
/// and 8-digit (`#rrggbbaa`) forms.  Alpha channels are silently ignored.
fn parse_css_hex(s: &str) -> Result<Rgb, RasterError> {
    let s = s.trim();
    let hex = s
        .strip_prefix('#')
        .ok_or_else(|| RasterError::JsError(format!("not a hex color: {s:?}")))?;
    match hex.len() {
        3 | 4 => {
            // Short form: #rgb or #rgba (alpha ignored) → #rrggbb
            let mut rgb = [0u8; 3];
            for (i, ch) in hex[..3].chars().enumerate() {
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
        6 | 8 => {
            // Long form: #rrggbb or #rrggbbaa (alpha ignored)
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

/// Create a Blob URL from raw PNG bytes.
///
/// Shared helper used by both [`gray_image_to_blob_url`] and
/// [`themed_gray_image_to_blob_url`] to avoid duplicating the
/// `Uint8Array` → `Blob` → object URL pipeline.
fn png_bytes_to_blob_url(png_bytes: &[u8]) -> Result<String, RasterError> {
    let uint8_array = js_sys::Uint8Array::from(png_bytes);
    let parts = js_sys::Array::new();
    parts.push(&uint8_array);

    let opts = BlobPropertyBag::new();
    opts.set_type("image/png");
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts)?;

    let url = web_sys::Url::create_object_url_with_blob(&blob)?;
    Ok(url)
}

/// Eagerly cached themed Blob URLs for a binary edge image.
///
/// Both light and dark URLs are generated when a new pipeline result
/// arrives.  On theme toggle, the component simply selects the other URL
/// — no re-encoding needed.
#[derive(PartialEq, Eq)]
pub struct ThemedEdgeUrls {
    /// Pointer identity of the `StagedResult` these URLs were generated from.
    pub staged_ptr: usize,
    /// Blob URL using light-mode preview colors.
    pub light_url: String,
    /// Blob URL using dark-mode preview colors.
    pub dark_url: String,
}

impl Drop for ThemedEdgeUrls {
    fn drop(&mut self) {
        revoke_blob_url(&self.light_url);
        revoke_blob_url(&self.dark_url);
    }
}

/// Generate both themed Blob URLs for a binary edge image.
///
/// `staged_ptr` is the pointer identity of the `StagedResult` these
/// URLs are generated from, used for cache invalidation.
///
/// # Errors
///
/// Returns [`RasterError`] if CSS color reading or PNG encoding fails.
pub fn generate_themed_edge_urls(
    edges: &GrayImage,
    staged_ptr: usize,
) -> Result<ThemedEdgeUrls, RasterError> {
    let colors = read_both_preview_colors()?;
    let light_url = themed_gray_image_to_blob_url(edges, colors.light.bg, colors.light.fg)?;
    let dark_url = themed_gray_image_to_blob_url(edges, colors.dark.bg, colors.dark.fg)?;
    Ok(ThemedEdgeUrls {
        staged_ptr,
        light_url,
        dark_url,
    })
}

/// A single Blob URL that auto-revokes on drop.
///
/// Used by `use_memo` caches so the URL is revoked when the memo
/// recomputes or the component unmounts.
#[derive(PartialEq, Eq)]
pub struct CachedBlobUrl {
    /// The Blob URL string.
    pub url: String,
}

impl Drop for CachedBlobUrl {
    fn drop(&mut self) {
        revoke_blob_url(&self.url);
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
