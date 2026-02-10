//! Shared types for the mujou image processing pipeline.

use crate::contour::ContourTracerKind;
use crate::join::PathJoinerKind;

/// Re-export `GrayImage` so downstream crates can reference
/// intermediate raster data without depending on `image` directly.
pub use image::GrayImage;

/// A 2D point in image coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
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
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
/// # Future work
///
/// Fields are currently public with no construction-time validation.
/// A validated constructor (`try_new`) or builder should be added to
/// enforce invariants such as `blur_sigma > 0`, `canny_low <= canny_high`,
/// `0.0 <= mask_diameter <= 1.0`, and `simplify_tolerance >= 0.0`.
/// Invalid values would return [`PipelineError::InvalidConfig`].
/// See [open-questions: PipelineConfig validation](https://github.com/altendky/mujou/pull/2#discussion_r2778003093).
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineConfig {
    /// Gaussian blur kernel sigma. Higher values produce more smoothing
    /// before edge detection.
    pub blur_sigma: f32,

    /// Canny edge detector low threshold. Pixels with gradient magnitude
    /// between `canny_low` and `canny_high` are edges only if connected
    /// to a strong edge.
    pub canny_low: f32,

    /// Canny edge detector high threshold. Pixels with gradient magnitude
    /// above this value are definite edges.
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
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone)]
pub struct StagedResult {
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

/// Errors that can occur during pipeline processing.
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
}
