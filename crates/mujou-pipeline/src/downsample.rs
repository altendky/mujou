//! Image downsampling to a target working resolution.
//!
//! Reduces the input image so the longest axis matches the configured
//! `working_resolution`. This is the first processing step after decode,
//! ensuring all expensive downstream stages (blur, Canny, contour tracing)
//! operate on a much smaller pixel grid.
//!
//! If the image is already at or below the target resolution, it is
//! returned unchanged.

use std::fmt;

use image::DynamicImage;
use serde::{Deserialize, Serialize};

/// Resampling filter used when downsampling.
///
/// Ordered from fastest/lowest-quality to slowest/highest-quality,
/// with a `None` variant to skip downsampling entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownsampleFilter {
    /// Disabled: skip downsampling regardless of image size.
    None,
    /// Nearest-neighbor: fastest, blocky artifacts.
    Nearest,
    /// Bilinear interpolation: fast, decent quality.
    Triangle,
    /// Bicubic (Catmull-Rom): moderate speed, good quality.
    CatmullRom,
    /// Gaussian: moderate speed, smooth output.
    Gaussian,
    /// Lanczos with 3 lobes: slowest, sharpest/best for photos.
    Lanczos3,
}

impl Default for DownsampleFilter {
    fn default() -> Self {
        Self::Triangle
    }
}

impl DownsampleFilter {
    /// Convert to the `image` crate's `FilterType`.
    ///
    /// Returns `Option::None` for [`DownsampleFilter::None`] since
    /// there is no corresponding resampling filter.
    const fn to_image_filter(self) -> Option<image::imageops::FilterType> {
        match self {
            Self::None => Option::None,
            Self::Nearest => Some(image::imageops::FilterType::Nearest),
            Self::Triangle => Some(image::imageops::FilterType::Triangle),
            Self::CatmullRom => Some(image::imageops::FilterType::CatmullRom),
            Self::Gaussian => Some(image::imageops::FilterType::Gaussian),
            Self::Lanczos3 => Some(image::imageops::FilterType::Lanczos3),
        }
    }
}

impl fmt::Display for DownsampleFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Nearest => f.write_str("Nearest"),
            Self::Triangle => f.write_str("Triangle"),
            Self::CatmullRom => f.write_str("CatmullRom"),
            Self::Gaussian => f.write_str("Gaussian"),
            Self::Lanczos3 => f.write_str("Lanczos3"),
        }
    }
}

/// Downsample a decoded image so the longest axis is at most
/// `max_dimension` pixels, using the specified resampling filter.
///
/// Returns the (possibly unchanged) image and whether downsampling
/// was actually applied.
#[must_use]
pub fn downsample(
    image: &DynamicImage,
    max_dimension: u32,
    filter: DownsampleFilter,
) -> (DynamicImage, bool) {
    let Some(image_filter) = filter.to_image_filter() else {
        return (image.clone(), false);
    };

    let (w, h) = (image.width(), image.height());
    let long_axis = w.max(h);

    if long_axis <= max_dimension {
        return (image.clone(), false);
    }

    let resized = image.resize(max_dimension, max_dimension, image_filter);
    (resized, true)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn test_image(w: u32, h: u32) -> DynamicImage {
        DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            w,
            h,
            image::Rgba([128, 128, 128, 255]),
        ))
    }

    #[test]
    fn default_filter_is_triangle() {
        assert_eq!(DownsampleFilter::default(), DownsampleFilter::Triangle);
    }

    #[test]
    fn no_downsample_when_already_small() {
        let img = test_image(100, 80);
        let (result, applied) = downsample(&img, 256, DownsampleFilter::Triangle);
        assert!(!applied);
        assert_eq!(result.width(), 100);
        assert_eq!(result.height(), 80);
    }

    #[test]
    fn no_downsample_when_exact_match() {
        let img = test_image(256, 200);
        let (result, applied) = downsample(&img, 256, DownsampleFilter::Triangle);
        assert!(!applied);
        assert_eq!(result.width(), 256);
        assert_eq!(result.height(), 200);
    }

    #[test]
    fn downsample_landscape() {
        let img = test_image(1024, 768);
        let (result, applied) = downsample(&img, 256, DownsampleFilter::Triangle);
        assert!(applied);
        assert_eq!(result.width(), 256);
        // Aspect ratio preserved: 768 * 256 / 1024 = 192
        assert_eq!(result.height(), 192);
    }

    #[test]
    fn downsample_portrait() {
        let img = test_image(600, 1200);
        let (result, applied) = downsample(&img, 256, DownsampleFilter::Triangle);
        assert!(applied);
        // Long axis is height (1200), so height becomes 256
        assert_eq!(result.height(), 256);
        // 600 * 256 / 1200 = 128
        assert_eq!(result.width(), 128);
    }

    #[test]
    fn downsample_square() {
        let img = test_image(1024, 1024);
        let (result, applied) = downsample(&img, 256, DownsampleFilter::Triangle);
        assert!(applied);
        assert_eq!(result.width(), 256);
        assert_eq!(result.height(), 256);
    }

    #[test]
    fn none_filter_skips_even_large_image() {
        let img = test_image(1024, 768);
        let (result, applied) = downsample(&img, 256, DownsampleFilter::None);
        assert!(!applied);
        assert_eq!(result.width(), 1024);
        assert_eq!(result.height(), 768);
    }
}
