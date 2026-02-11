//! Shared types for the mujou image processing pipeline.

use serde::{Deserialize, Serialize};

use crate::contour::ContourTracerKind;
use crate::join::PathJoinerKind;

/// Re-export `GrayImage` so downstream crates can reference
/// intermediate raster data without depending on `image` directly.
pub use image::GrayImage;

/// Re-export `RgbaImage` so downstream crates can reference the
/// original decoded image without depending on `image` directly.
pub use image::RgbaImage;

/// A 2D point in image coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// Horizontal position (pixels from left edge).
    pub x: f64,
    /// Vertical position (pixels from top edge).
    pub y: f64,
}

impl Point {
    /// Create a new point.
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Squared Euclidean distance to another point.
    ///
    /// Avoids the square root for comparison purposes.
    #[must_use]
    pub fn distance_squared(self, other: Self) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        dx.mul_add(dx, dy * dy)
    }

    /// Euclidean distance to another point.
    #[must_use]
    pub fn distance(self, other: Self) -> f64 {
        self.distance_squared(other).sqrt()
    }
}

/// A sequence of connected points forming a path segment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Polyline(Vec<Point>);

impl Polyline {
    /// Create a new polyline from a vector of points.
    #[must_use]
    pub const fn new(points: Vec<Point>) -> Self {
        Self(points)
    }

    /// Returns `true` if the polyline has no points.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the number of points in the polyline.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns the first point, if any.
    #[must_use]
    pub fn first(&self) -> Option<&Point> {
        self.0.first()
    }

    /// Returns the last point, if any.
    #[must_use]
    pub fn last(&self) -> Option<&Point> {
        self.0.last()
    }

    /// Returns a slice of all points.
    #[must_use]
    pub fn points(&self) -> &[Point] {
        &self.0
    }

    /// Consumes the polyline and returns the underlying vector of points.
    #[must_use]
    pub fn into_points(self) -> Vec<Point> {
        self.0
    }
}

/// Image dimensions in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dimensions {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Configuration for the image processing pipeline.
///
/// All parameters have sensible defaults matching the
/// pipeline specification.
///
/// # Canny threshold invariants
///
/// Both `canny_low` and `canny_high` must be at least
/// [`edge::MIN_THRESHOLD`] (1.0), and `canny_low` must not exceed
/// `canny_high`. These invariants are enforced at the UI level (slider
/// ranges and cross-clamping) and as defense-in-depth inside
/// [`edge::canny`]. See <https://github.com/altendky/mujou/issues/44>.
///
/// # Future work
///
/// Fields are currently public with no construction-time validation.
/// A validated constructor (`try_new`) or builder should be added to
/// enforce invariants such as `blur_sigma > 0`, `canny_low <= canny_high`,
/// `canny_low >= 1.0`, `0.0 <= mask_diameter <= 1.0`, and
/// `simplify_tolerance >= 0.0`.
/// Invalid values would return [`PipelineError::InvalidConfig`].
/// See [open-questions: PipelineConfig validation](https://github.com/altendky/mujou/pull/2#discussion_r2778003093).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineConfig {
    /// Gaussian blur kernel sigma. Higher values produce more smoothing
    /// before edge detection.
    pub blur_sigma: f32,

    /// Canny edge detector low threshold. Pixels with gradient magnitude
    /// between `canny_low` and `canny_high` are edges only if connected
    /// to a strong edge.
    ///
    /// Must be at least [`edge::MIN_THRESHOLD`] and at most `canny_high`.
    pub canny_low: f32,

    /// Canny edge detector high threshold. Pixels with gradient magnitude
    /// above this value are definite edges.
    ///
    /// Must be at least [`edge::MIN_THRESHOLD`] and at least `canny_low`.
    pub canny_high: f32,

    /// Which contour tracing algorithm to use.
    pub contour_tracer: ContourTracerKind,

    /// Ramer-Douglas-Peucker simplification tolerance in pixels.
    /// Higher values remove more points, producing simpler paths.
    pub simplify_tolerance: f64,

    /// Which path joining strategy to use for connecting disconnected
    /// contours into a single continuous path.
    pub path_joiner: PathJoinerKind,

    /// Whether to clip output paths to a circular mask.
    /// Useful for round sand tables.
    pub circular_mask: bool,

    /// Mask diameter as a fraction of image width (0.0 to 1.0).
    /// Only used when `circular_mask` is `true`.
    pub mask_diameter: f64,

    /// Whether to invert the binary edge map before contour tracing.
    pub invert: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            blur_sigma: 1.4,
            canny_low: 50.0,
            canny_high: 150.0,
            contour_tracer: ContourTracerKind::default(),
            simplify_tolerance: 2.0,
            path_joiner: PathJoinerKind::default(),
            circular_mask: false,
            mask_diameter: 1.0,
            invert: false,
        }
    }
}

/// Result of running the full image processing pipeline.
///
/// Contains the traced polyline and metadata about the source image
/// needed by downstream consumers (e.g., export serializers).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessResult {
    /// The single continuous path produced by the pipeline.
    pub polyline: Polyline,

    /// Dimensions of the source image in pixels.
    ///
    /// Export serializers use this to set coordinate spaces
    /// (e.g., SVG `viewBox`, G-code bed scaling).
    pub dimensions: Dimensions,
}

/// Result of running the pipeline with all intermediate stage outputs preserved.
///
/// Each field captures the output of one logical pipeline stage,
/// enabling the UI to display thumbnails and full-size previews for
/// every step of the processing chain.
///
/// Note: does not derive `PartialEq` because `GrayImage` does not
/// implement it. When wrapped in `Rc`, Dioxus will use pointer
/// equality for diffing, which is more efficient than walking pixel data.
///
/// Uses custom `Serialize`/`Deserialize` implementations because
/// `GrayImage` and `RgbaImage` (from the `image` crate) do not
/// implement serde traits. Raster images are serialized as
/// `(width, height, raw_pixels)` tuples.
#[derive(Debug, Clone)]
pub struct StagedResult {
    /// Stage 0: original decoded RGBA image (pre-processing).
    pub original: RgbaImage,
    /// Stage 1: decoded + grayscale image.
    pub grayscale: GrayImage,
    /// Stage 2: Gaussian-blurred image.
    pub blurred: GrayImage,
    /// Stages 3+4: Canny edge map (post-inversion when `invert=true`).
    pub edges: GrayImage,
    /// Stage 5: traced contour polylines.
    pub contours: Vec<Polyline>,
    /// Stage 6: RDP-simplified polylines.
    pub simplified: Vec<Polyline>,
    /// Stage 8: joined single continuous path.
    pub joined: Polyline,
    /// Stage 9: circular-masked path (`Some` only when `circular_mask=true`).
    pub masked: Option<Polyline>,
    /// Source image dimensions in pixels.
    pub dimensions: Dimensions,
}

impl StagedResult {
    /// Returns the final output polyline — masked if masking is enabled,
    /// otherwise the joined path.
    #[must_use]
    pub fn final_polyline(&self) -> &Polyline {
        self.masked.as_ref().unwrap_or(&self.joined)
    }
}

/// Serde-compatible proxy for `StagedResult`.
///
/// Raster images are represented as `(width, height, raw_pixel_bytes)`
/// tuples since `image::ImageBuffer` does not implement serde traits.
#[derive(Serialize, Deserialize)]
struct StagedResultProxy {
    original: (u32, u32, Vec<u8>),
    grayscale: (u32, u32, Vec<u8>),
    blurred: (u32, u32, Vec<u8>),
    edges: (u32, u32, Vec<u8>),
    contours: Vec<Polyline>,
    simplified: Vec<Polyline>,
    joined: Polyline,
    masked: Option<Polyline>,
    dimensions: Dimensions,
}

impl Serialize for StagedResult {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let proxy = StagedResultProxy {
            original: (
                self.original.width(),
                self.original.height(),
                self.original.as_raw().clone(),
            ),
            grayscale: (
                self.grayscale.width(),
                self.grayscale.height(),
                self.grayscale.as_raw().clone(),
            ),
            blurred: (
                self.blurred.width(),
                self.blurred.height(),
                self.blurred.as_raw().clone(),
            ),
            edges: (
                self.edges.width(),
                self.edges.height(),
                self.edges.as_raw().clone(),
            ),
            contours: self.contours.clone(),
            simplified: self.simplified.clone(),
            joined: self.joined.clone(),
            masked: self.masked.clone(),
            dimensions: self.dimensions,
        };
        proxy.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for StagedResult {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let proxy = StagedResultProxy::deserialize(deserializer)?;

        let original = RgbaImage::from_raw(proxy.original.0, proxy.original.1, proxy.original.2)
            .ok_or_else(|| serde::de::Error::custom("invalid RGBA image dimensions"))?;
        let grayscale =
            GrayImage::from_raw(proxy.grayscale.0, proxy.grayscale.1, proxy.grayscale.2)
                .ok_or_else(|| serde::de::Error::custom("invalid grayscale image dimensions"))?;
        let blurred = GrayImage::from_raw(proxy.blurred.0, proxy.blurred.1, proxy.blurred.2)
            .ok_or_else(|| serde::de::Error::custom("invalid blurred image dimensions"))?;
        let edges = GrayImage::from_raw(proxy.edges.0, proxy.edges.1, proxy.edges.2)
            .ok_or_else(|| serde::de::Error::custom("invalid edges image dimensions"))?;

        Ok(Self {
            original,
            grayscale,
            blurred,
            edges,
            contours: proxy.contours,
            simplified: proxy.simplified,
            joined: proxy.joined,
            masked: proxy.masked,
            dimensions: proxy.dimensions,
        })
    }
}

/// Errors that can occur during pipeline processing.
///
/// Uses custom `Serialize`/`Deserialize` because `image::ImageError`
/// does not implement serde traits. The `ImageDecode` variant is
/// serialized as its `Display` string.
#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    /// Failed to decode the input image.
    #[error("failed to decode image: {0}")]
    ImageDecode(#[from] image::ImageError),

    /// The input image bytes were empty.
    #[error("input image data is empty")]
    EmptyInput,

    /// Pipeline configuration is invalid.
    #[error("invalid pipeline configuration: {0}")]
    InvalidConfig(String),

    /// Edge detection produced no contours.
    #[error("no contours found in the image")]
    NoContours,
}

/// Serde-compatible proxy for `PipelineError`.
///
/// `image::ImageError` does not implement serde, so the `ImageDecode`
/// variant stores its `Display` string instead. A deserialized
/// `ImageDecode` will have a generic error message (the original typed
/// error cannot be reconstructed), but the message is preserved.
#[derive(Serialize, Deserialize)]
enum PipelineErrorProxy {
    ImageDecode(String),
    EmptyInput,
    InvalidConfig(String),
    NoContours,
}

impl Serialize for PipelineError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let proxy = match self {
            Self::ImageDecode(e) => PipelineErrorProxy::ImageDecode(e.to_string()),
            Self::EmptyInput => PipelineErrorProxy::EmptyInput,
            Self::InvalidConfig(s) => PipelineErrorProxy::InvalidConfig(s.clone()),
            Self::NoContours => PipelineErrorProxy::NoContours,
        };
        proxy.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PipelineError {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let proxy = PipelineErrorProxy::deserialize(deserializer)?;
        Ok(match proxy {
            PipelineErrorProxy::ImageDecode(msg) => {
                // Reconstruct as an InvalidConfig wrapping the message since
                // we cannot reconstruct the original image::ImageError.
                // Use a custom error that preserves the original message.
                Self::InvalidConfig(format!("image decode error: {msg}"))
            }
            PipelineErrorProxy::EmptyInput => Self::EmptyInput,
            PipelineErrorProxy::InvalidConfig(s) => Self::InvalidConfig(s),
            PipelineErrorProxy::NoContours => Self::NoContours,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Point tests ---

    #[test]
    fn point_new() {
        let p = Point::new(3.0, 4.0);
        assert!((p.x - 3.0).abs() < f64::EPSILON);
        assert!((p.y - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn point_equality() {
        assert_eq!(Point::new(1.0, 2.0), Point::new(1.0, 2.0));
        assert_ne!(Point::new(1.0, 2.0), Point::new(1.0, 3.0));
    }

    #[test]
    fn point_distance_squared() {
        let a = Point::new(0.0, 0.0);
        let b = Point::new(3.0, 4.0);
        assert!((a.distance_squared(b) - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn point_distance() {
        let a = Point::new(0.0, 0.0);
        let b = Point::new(3.0, 4.0);
        assert!((a.distance(b) - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn point_distance_to_self_is_zero() {
        let p = Point::new(7.0, 11.0);
        assert!((p.distance(p)).abs() < f64::EPSILON);
    }

    #[test]
    fn point_copy() {
        let p = Point::new(1.0, 2.0);
        let p2 = p; // Copy
        assert_eq!(p, p2);
    }

    // --- Polyline tests ---

    #[test]
    fn polyline_new_and_len() {
        let pl = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)]);
        assert_eq!(pl.len(), 2);
        assert!(!pl.is_empty());
    }

    #[test]
    fn polyline_empty() {
        let pl = Polyline::new(vec![]);
        assert!(pl.is_empty());
        assert_eq!(pl.len(), 0);
        assert!(pl.first().is_none());
        assert!(pl.last().is_none());
    }

    #[test]
    fn polyline_first_and_last() {
        let pl = Polyline::new(vec![
            Point::new(1.0, 2.0),
            Point::new(3.0, 4.0),
            Point::new(5.0, 6.0),
        ]);
        assert_eq!(pl.first(), Some(&Point::new(1.0, 2.0)));
        assert_eq!(pl.last(), Some(&Point::new(5.0, 6.0)));
    }

    #[test]
    fn polyline_points_returns_all() {
        let points = vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)];
        let pl = Polyline::new(points.clone());
        assert_eq!(pl.points(), &points);
    }

    #[test]
    fn polyline_into_points_returns_owned_vec() {
        let points = vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)];
        let pl = Polyline::new(points.clone());
        assert_eq!(pl.into_points(), points);
    }

    // --- Dimensions tests ---

    #[test]
    fn dimensions_equality() {
        assert_eq!(
            Dimensions {
                width: 100,
                height: 200
            },
            Dimensions {
                width: 100,
                height: 200
            },
        );
        assert_ne!(
            Dimensions {
                width: 100,
                height: 200
            },
            Dimensions {
                width: 100,
                height: 201
            },
        );
    }

    // --- PipelineConfig tests ---

    #[test]
    fn pipeline_config_defaults_match_spec() {
        let config = PipelineConfig::default();
        assert!((config.blur_sigma - 1.4).abs() < f32::EPSILON);
        assert!((config.canny_low - 50.0).abs() < f32::EPSILON);
        assert!((config.canny_high - 150.0).abs() < f32::EPSILON);
        assert_eq!(config.contour_tracer, ContourTracerKind::BorderFollowing);
        assert!((config.simplify_tolerance - 2.0).abs() < f64::EPSILON);
        assert_eq!(config.path_joiner, PathJoinerKind::StraightLine);
        assert!(!config.circular_mask);
        assert!((config.mask_diameter - 1.0).abs() < f64::EPSILON);
        assert!(!config.invert);
    }

    // --- PipelineError tests ---

    #[test]
    fn error_empty_input_display() {
        let err = PipelineError::EmptyInput;
        assert_eq!(err.to_string(), "input image data is empty");
    }

    #[test]
    fn error_invalid_config_display() {
        let err = PipelineError::InvalidConfig("canny_low > canny_high".to_string());
        assert_eq!(
            err.to_string(),
            "invalid pipeline configuration: canny_low > canny_high",
        );
    }

    #[test]
    fn error_no_contours_display() {
        let err = PipelineError::NoContours;
        assert_eq!(err.to_string(), "no contours found in the image");
    }

    // --- Serde round-trip tests ---

    #[test]
    fn point_serde_round_trip() {
        let p = Point::new(3.14, -2.71);
        let json = serde_json::to_string(&p).unwrap();
        let deserialized: Point = serde_json::from_str(&json).unwrap();
        assert_eq!(p, deserialized);
    }

    #[test]
    fn polyline_serde_round_trip() {
        let pl = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(1.5, 2.5),
            Point::new(3.0, 0.0),
        ]);
        let json = serde_json::to_string(&pl).unwrap();
        let deserialized: Polyline = serde_json::from_str(&json).unwrap();
        assert_eq!(pl, deserialized);
    }

    #[test]
    fn dimensions_serde_round_trip() {
        let d = Dimensions {
            width: 640,
            height: 480,
        };
        let json = serde_json::to_string(&d).unwrap();
        let deserialized: Dimensions = serde_json::from_str(&json).unwrap();
        assert_eq!(d, deserialized);
    }

    #[test]
    fn pipeline_config_serde_round_trip() {
        let config = PipelineConfig {
            blur_sigma: 2.0,
            canny_low: 30.0,
            canny_high: 120.0,
            contour_tracer: ContourTracerKind::BorderFollowing,
            simplify_tolerance: 1.5,
            path_joiner: PathJoinerKind::Retrace,
            circular_mask: true,
            mask_diameter: 0.85,
            invert: true,
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: PipelineConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn process_result_serde_round_trip() {
        let pr = ProcessResult {
            polyline: Polyline::new(vec![Point::new(1.0, 2.0), Point::new(3.0, 4.0)]),
            dimensions: Dimensions {
                width: 100,
                height: 200,
            },
        };
        let json = serde_json::to_string(&pr).unwrap();
        let deserialized: ProcessResult = serde_json::from_str(&json).unwrap();
        assert_eq!(pr, deserialized);
    }

    #[test]
    fn staged_result_serde_round_trip() {
        // Create a minimal StagedResult with small images.
        let staged = StagedResult {
            original: RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255])),
            grayscale: GrayImage::from_pixel(2, 2, image::Luma([50])),
            blurred: GrayImage::from_pixel(2, 2, image::Luma([45])),
            edges: GrayImage::from_pixel(2, 2, image::Luma([255])),
            contours: vec![Polyline::new(vec![
                Point::new(0.0, 0.0),
                Point::new(1.0, 1.0),
            ])],
            simplified: vec![Polyline::new(vec![
                Point::new(0.0, 0.0),
                Point::new(1.0, 1.0),
            ])],
            joined: Polyline::new(vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)]),
            masked: None,
            dimensions: Dimensions {
                width: 2,
                height: 2,
            },
        };

        let json = serde_json::to_string(&staged).unwrap();
        let deserialized: StagedResult = serde_json::from_str(&json).unwrap();

        // Verify raster images survived the round trip.
        assert_eq!(
            staged.original.dimensions(),
            deserialized.original.dimensions()
        );
        assert_eq!(staged.original.as_raw(), deserialized.original.as_raw());
        assert_eq!(staged.grayscale.as_raw(), deserialized.grayscale.as_raw());
        assert_eq!(staged.blurred.as_raw(), deserialized.blurred.as_raw());
        assert_eq!(staged.edges.as_raw(), deserialized.edges.as_raw());

        // Verify vector data survived.
        assert_eq!(staged.contours, deserialized.contours);
        assert_eq!(staged.simplified, deserialized.simplified);
        assert_eq!(staged.joined, deserialized.joined);
        assert_eq!(staged.masked, deserialized.masked);
        assert_eq!(staged.dimensions, deserialized.dimensions);
    }

    #[test]
    fn pipeline_error_serde_round_trip_empty_input() {
        let err = PipelineError::EmptyInput;
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: PipelineError = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, PipelineError::EmptyInput));
    }

    #[test]
    fn pipeline_error_serde_round_trip_no_contours() {
        let err = PipelineError::NoContours;
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: PipelineError = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, PipelineError::NoContours));
    }

    #[test]
    fn pipeline_error_serde_round_trip_invalid_config() {
        let err = PipelineError::InvalidConfig("bad value".to_string());
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: PipelineError = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, PipelineError::InvalidConfig(ref s) if s == "bad value"));
    }

    #[test]
    fn pipeline_result_ok_serde_round_trip() {
        // Test that Result<StagedResult, PipelineError> can be serialized
        // and deserialized — this is the type that crosses the worker boundary.
        let staged = StagedResult {
            original: RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 255])),
            grayscale: GrayImage::from_pixel(1, 1, image::Luma([0])),
            blurred: GrayImage::from_pixel(1, 1, image::Luma([0])),
            edges: GrayImage::from_pixel(1, 1, image::Luma([0])),
            contours: vec![],
            simplified: vec![],
            joined: Polyline::new(vec![]),
            masked: None,
            dimensions: Dimensions {
                width: 1,
                height: 1,
            },
        };
        let result: Result<StagedResult, PipelineError> = Ok(staged);
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: Result<StagedResult, PipelineError> =
            serde_json::from_str(&json).unwrap();
        assert!(deserialized.is_ok());
    }

    #[test]
    fn pipeline_result_err_serde_round_trip() {
        let result: Result<StagedResult, PipelineError> = Err(PipelineError::NoContours);
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: Result<StagedResult, PipelineError> =
            serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, Err(PipelineError::NoContours)));
    }
}
