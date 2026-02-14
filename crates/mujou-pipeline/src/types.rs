//! Shared types for the mujou image processing pipeline.

use serde::{Deserialize, Serialize};

use crate::contour::ContourTracerKind;
use crate::downsample::DownsampleFilter;
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

/// Compute the axis-aligned bounding box of all points across polylines.
///
/// Returns `(min_x, min_y, max_x, max_y)`.  When all polylines are empty
/// the returned rectangle has inverted infinities (min > max).
#[must_use]
pub(crate) fn polyline_bounding_box(polylines: &[&Polyline]) -> (f64, f64, f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for poly in polylines {
        for p in poly.points() {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
    }

    (min_x, min_y, max_x, max_y)
}

/// Image dimensions in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dimensions {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl Dimensions {
    /// Compute the mask circle radius in pixels for a given `mask_diameter`.
    ///
    /// The diameter is expressed as a fraction of the image diagonal
    /// (`sqrt(width² + height²)`), so at 1.0 the circle circumscribes the
    /// entire image.
    #[must_use]
    pub fn mask_radius(self, mask_diameter: f64) -> f64 {
        let w = f64::from(self.width);
        let h = f64::from(self.height);
        w.hypot(h) * mask_diameter / 2.0
    }
}

/// Channels to use for edge detection.
///
/// Canny edge detection is run independently on each enabled channel.
/// The resulting edge maps are combined via pixel-wise maximum, so edges
/// detected in *any* enabled channel appear in the final edge map.
///
/// By default only [`luminance`](Self::luminance) is enabled, reproducing
/// the standard single-channel grayscale pipeline. Enabling additional
/// channels captures edges that luminance alone misses — for example,
/// boundaries where color changes but brightness stays similar (hue edges).
///
/// At least one channel must be enabled. See
/// [`PipelineConfig::validate`] for the enforcement rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct EdgeChannels {
    /// sRGB/Rec.709 weighted grayscale (`0.2126R + 0.7152G + 0.0722B`).
    ///
    /// This is the standard luminance conversion matching the `image`
    /// crate's `to_luma8()`. It captures edges where brightness changes
    /// and works well for most photographic images.
    pub luminance: bool,

    /// Red channel from RGB.
    ///
    /// Skin appears bright in the red channel, making it particularly
    /// useful for detecting skin/lip boundaries in portrait photography.
    pub red: bool,

    /// Green channel from RGB.
    ///
    /// Most similar to luminance (green has the highest weight in
    /// Rec.709). Captures overall detail.
    pub green: bool,

    /// Blue channel from RGB.
    ///
    /// Skin and hair appear dark in the blue channel. Tends to be
    /// noisier than red or green in skin regions.
    pub blue: bool,

    /// Saturation channel from HSV.
    ///
    /// Highlights boundaries where colorfulness changes — e.g. lips
    /// against skin, colored clothing against a neutral background.
    /// Computed as `(max(R,G,B) - min(R,G,B)) / max(R,G,B)`, scaled
    /// to 0–255.
    pub saturation: bool,
}

impl EdgeChannels {
    /// Returns `true` if at least one channel is enabled.
    #[must_use]
    pub const fn any_enabled(&self) -> bool {
        self.luminance || self.red || self.green || self.blue || self.saturation
    }

    /// Returns the number of enabled channels.
    #[must_use]
    pub const fn count(&self) -> usize {
        self.luminance as usize
            + self.red as usize
            + self.green as usize
            + self.blue as usize
            + self.saturation as usize
    }
}

impl Default for EdgeChannels {
    fn default() -> Self {
        Self {
            luminance: true,
            red: false,
            green: false,
            blue: false,
            saturation: false,
        }
    }
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
/// `canny_low >= 1.0`, `0.0 <= mask_diameter <= 1.5`, and
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

    /// Upper bound for the Canny threshold sliders in the UI.
    ///
    /// Does **not** affect pipeline computation — it controls slider
    /// range so the user can "zoom in" on a useful threshold region.
    /// Must be at least `canny_high` and at most
    /// [`edge::max_gradient_magnitude()`].
    pub canny_max: f32,

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

    /// Mask diameter as a fraction of image diagonal (0.0 to 1.5).
    /// At 1.0 the circle circumscribes the full image (all four corners
    /// lie on or inside the circle). Only used when `circular_mask` is
    /// `true`.
    pub mask_diameter: f64,

    /// Whether to invert the binary edge map before contour tracing.
    pub invert: bool,

    /// Maximum pixel dimension (longest axis) for the working image.
    ///
    /// After decoding, the image is downsampled so the longest axis
    /// matches this value. All subsequent pipeline stages operate at
    /// this reduced resolution. Based on reference target device
    /// analysis: a 34" sand table with ~5mm track width has ~170
    /// resolvable lines, so 256px provides ~1.5x oversampling.
    pub working_resolution: u32,

    /// Resampling filter used when downsampling. Triangle (bilinear) is
    /// a good default -- fast and sufficient quality given the Gaussian
    /// blur stage that follows. Lanczos3 is sharper but significantly
    /// slower.
    pub downsample_filter: DownsampleFilter,

    /// Number of cross-polyline nearest-neighbour candidates examined per
    /// sample point during MST construction.
    ///
    /// Higher values improve MST quality for images with many small
    /// isolated contours (e.g. scattered petals) at the cost of more
    /// candidate edge generation. Only affects the MST path joiner.
    pub mst_neighbours: usize,

    /// Which image channels to use for edge detection.
    ///
    /// Canny is run independently on each enabled channel and the
    /// results are combined via pixel-wise maximum. See [`EdgeChannels`]
    /// for per-channel documentation.
    #[serde(default)]
    pub edge_channels: EdgeChannels,
}

impl PipelineConfig {
    /// Default Gaussian blur sigma.
    pub const DEFAULT_BLUR_SIGMA: f32 = 1.4;
    /// Default Canny low threshold.
    pub const DEFAULT_CANNY_LOW: f32 = 15.0;
    /// Default Canny high threshold.
    pub const DEFAULT_CANNY_HIGH: f32 = 40.0;
    /// Default Canny slider maximum.
    pub const DEFAULT_CANNY_MAX: f32 = 60.0;
    /// Default RDP simplification tolerance in pixels.
    pub const DEFAULT_SIMPLIFY_TOLERANCE: f64 = 2.0;
    /// Default circular mask enabled state.
    pub const DEFAULT_CIRCULAR_MASK: bool = true;
    /// Default mask diameter as a fraction of image diagonal.
    pub const DEFAULT_MASK_DIAMETER: f64 = 0.75;
    /// Default edge map inversion state.
    pub const DEFAULT_INVERT: bool = false;
    /// Default working resolution (max dimension after downsampling).
    pub const DEFAULT_WORKING_RESOLUTION: u32 = 1000;
    /// Default downsample filter.
    pub const DEFAULT_DOWNSAMPLE_FILTER: DownsampleFilter = DownsampleFilter::Triangle;
    /// Default MST nearest-neighbour candidate count per sample point.
    pub const DEFAULT_MST_NEIGHBOURS: usize = 100;
    /// Default edge channels (luminance only).
    pub const DEFAULT_EDGE_CHANNELS: EdgeChannels = EdgeChannels {
        luminance: true,
        red: false,
        green: false,
        blue: false,
        saturation: false,
    };

    /// Validate that all fields satisfy the documented invariants.
    ///
    /// Returns `Ok(())` if the config is valid, or
    /// [`PipelineError::InvalidConfig`] describing the first violated
    /// constraint.
    ///
    /// # Checked invariants
    ///
    /// - `blur_sigma > 0`
    /// - `canny_low >= edge::MIN_THRESHOLD` (1.0)
    /// - `canny_low <= canny_high`
    /// - `canny_high <= canny_max`
    /// - `canny_max <= edge::max_gradient_magnitude()`
    /// - `simplify_tolerance >= 0`
    /// - `mask_diameter` in `[0.0, 1.5]`
    /// - `working_resolution > 0`
    /// - `mst_neighbours > 0`
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::InvalidConfig`] with a human-readable
    /// message if any invariant is violated.
    pub fn validate(&self) -> Result<(), PipelineError> {
        if self.blur_sigma <= 0.0 {
            return Err(PipelineError::InvalidConfig(format!(
                "blur_sigma must be positive, got {}",
                self.blur_sigma,
            )));
        }
        if self.canny_low < crate::edge::MIN_THRESHOLD {
            return Err(PipelineError::InvalidConfig(format!(
                "canny_low must be at least {}, got {}",
                crate::edge::MIN_THRESHOLD,
                self.canny_low,
            )));
        }
        if self.canny_low > self.canny_high {
            return Err(PipelineError::InvalidConfig(format!(
                "canny_low ({}) must not exceed canny_high ({})",
                self.canny_low, self.canny_high,
            )));
        }
        if self.canny_high > self.canny_max {
            return Err(PipelineError::InvalidConfig(format!(
                "canny_high ({}) must not exceed canny_max ({})",
                self.canny_high, self.canny_max,
            )));
        }
        let max_mag = crate::edge::max_gradient_magnitude();
        if self.canny_max > max_mag {
            return Err(PipelineError::InvalidConfig(format!(
                "canny_max ({}) must not exceed max gradient magnitude ({max_mag})",
                self.canny_max,
            )));
        }
        if self.simplify_tolerance < 0.0 {
            return Err(PipelineError::InvalidConfig(format!(
                "simplify_tolerance must be non-negative, got {}",
                self.simplify_tolerance,
            )));
        }
        if !(0.0..=1.5).contains(&self.mask_diameter) {
            return Err(PipelineError::InvalidConfig(format!(
                "mask_diameter must be in [0.0, 1.5], got {}",
                self.mask_diameter,
            )));
        }
        if self.working_resolution == 0 {
            return Err(PipelineError::InvalidConfig(
                "working_resolution must be positive".to_owned(),
            ));
        }
        if self.mst_neighbours == 0 {
            return Err(PipelineError::InvalidConfig(
                "mst_neighbours must be positive".to_owned(),
            ));
        }
        if !self.edge_channels.any_enabled() {
            return Err(PipelineError::InvalidConfig(
                "at least one edge channel must be enabled".to_owned(),
            ));
        }
        Ok(())
    }
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            blur_sigma: Self::DEFAULT_BLUR_SIGMA,
            canny_low: Self::DEFAULT_CANNY_LOW,
            canny_high: Self::DEFAULT_CANNY_HIGH,
            canny_max: Self::DEFAULT_CANNY_MAX,
            contour_tracer: ContourTracerKind::default(),
            simplify_tolerance: Self::DEFAULT_SIMPLIFY_TOLERANCE,
            path_joiner: PathJoinerKind::default(),
            circular_mask: Self::DEFAULT_CIRCULAR_MASK,
            mask_diameter: Self::DEFAULT_MASK_DIAMETER,
            invert: Self::DEFAULT_INVERT,
            working_resolution: Self::DEFAULT_WORKING_RESOLUTION,
            downsample_filter: Self::DEFAULT_DOWNSAMPLE_FILTER,
            mst_neighbours: Self::DEFAULT_MST_NEIGHBOURS,
            edge_channels: Self::DEFAULT_EDGE_CHANNELS,
        }
    }
}

impl PipelineConfig {
    /// Compare two configs considering only fields that affect pipeline
    /// output.  UI-only fields like [`canny_max`](Self::canny_max) are
    /// ignored so that adjusting slider range alone does not trigger a
    /// costly reprocess.
    #[must_use]
    pub fn pipeline_eq(&self, other: &Self) -> bool {
        // Destructure so adding a field to PipelineConfig without updating
        // this match causes a compile error.
        let Self {
            blur_sigma,
            canny_low,
            canny_high,
            canny_max: _,
            contour_tracer,
            simplify_tolerance,
            path_joiner,
            circular_mask,
            mask_diameter,
            invert,
            working_resolution,
            downsample_filter,
            mst_neighbours,
            edge_channels,
        } = self;

        *blur_sigma == other.blur_sigma
            && *canny_low == other.canny_low
            && *canny_high == other.canny_high
            && *contour_tracer == other.contour_tracer
            && *simplify_tolerance == other.simplify_tolerance
            && *path_joiner == other.path_joiner
            && *circular_mask == other.circular_mask
            && *mask_diameter == other.mask_diameter
            && *invert == other.invert
            && *working_resolution == other.working_resolution
            && *downsample_filter == other.downsample_filter
            && *mst_neighbours == other.mst_neighbours
            && *edge_channels == other.edge_channels
    }

    /// Return the zero-based index of the earliest pipeline stage whose
    /// output would differ between `self` and `other`, given the same
    /// input image.
    ///
    /// Stages 0 (pending) and 1 (decode) have no config dependencies —
    /// they only depend on the source image bytes.  The earliest stage
    /// that can be invalidated by a config change is stage 2
    /// (downsample).
    ///
    /// Returns [`pipeline::STAGE_COUNT`](crate::pipeline::STAGE_COUNT)
    /// when the configs are pipeline-equivalent (identical modulo
    /// UI-only fields like `canny_max`).
    ///
    /// Uses exhaustive destructuring so that adding a field to
    /// [`PipelineConfig`] without updating this method causes a compile
    /// error.
    #[must_use]
    #[allow(clippy::float_cmp)]
    pub fn earliest_changed_stage(&self, other: &Self) -> usize {
        // Destructure to enforce compile-time coverage of all fields.
        let Self {
            blur_sigma,
            canny_low,
            canny_high,
            canny_max: _,
            contour_tracer,
            simplify_tolerance,
            path_joiner,
            circular_mask,
            mask_diameter,
            invert,
            working_resolution,
            downsample_filter,
            mst_neighbours,
            edge_channels,
        } = self;

        // Stage 2 — downsample: working_resolution, downsample_filter
        if *working_resolution != other.working_resolution
            || *downsample_filter != other.downsample_filter
        {
            return 2;
        }

        // Stage 3 — blur: blur_sigma
        if *blur_sigma != other.blur_sigma {
            return 3;
        }

        // Stage 4 — edge detection: edge_channels, canny_low, canny_high, invert
        if *edge_channels != other.edge_channels
            || *canny_low != other.canny_low
            || *canny_high != other.canny_high
            || *invert != other.invert
        {
            return 4;
        }

        // Stage 5 — contour tracing: contour_tracer
        if *contour_tracer != other.contour_tracer {
            return 5;
        }

        // Stage 6 — simplification: simplify_tolerance
        if *simplify_tolerance != other.simplify_tolerance {
            return 6;
        }

        // Stage 7 — masking: circular_mask, mask_diameter
        if *circular_mask != other.circular_mask || *mask_diameter != other.mask_diameter {
            return 7;
        }

        // Stage 8 — joining: path_joiner, mst_neighbours
        if *path_joiner != other.path_joiner || *mst_neighbours != other.mst_neighbours {
            return 8;
        }

        // All pipeline-relevant fields match.
        crate::pipeline::STAGE_COUNT
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
    /// Stage 1: downsampled RGBA image (working resolution).
    pub downsampled: RgbaImage,
    /// Stage 2: Gaussian-blurred RGBA image.
    pub blurred: RgbaImage,
    /// Stages 3+4: Canny edge map (post-inversion when `invert=true`).
    pub edges: GrayImage,
    /// Stage 5: traced contour polylines.
    pub contours: Vec<Polyline>,
    /// Stage 6: RDP-simplified polylines.
    pub simplified: Vec<Polyline>,
    /// Stage 7: circular-masked polylines (`Some` only when `circular_mask=true`).
    ///
    /// Contains the simplified polylines after clipping to the circular mask
    /// boundary. Polylines entirely outside the mask are discarded; those
    /// crossing the boundary are split at the intersection.
    pub masked: Option<Vec<Polyline>>,
    /// Stage 8: joined single continuous path (always the final output).
    ///
    /// When masking is enabled, this is the join of the masked polylines.
    /// When disabled, this is the join of the simplified polylines.
    pub joined: Polyline,
    /// Source image dimensions in pixels.
    pub dimensions: Dimensions,
}

impl StagedResult {
    /// Returns the final output polyline — always the joined path.
    ///
    /// Since masking now happens before joining, `joined` always contains
    /// the final single continuous path regardless of whether a circular
    /// mask was applied.
    #[must_use]
    pub const fn final_polyline(&self) -> &Polyline {
        &self.joined
    }
}

/// Serde-compatible proxy for `StagedResult`.
///
/// Raster images are represented as `(width, height, raw_pixel_bytes)`
/// tuples since `image::ImageBuffer` does not implement serde traits.
#[derive(Serialize, Deserialize)]
struct StagedResultProxy {
    original: (u32, u32, Vec<u8>),
    downsampled: (u32, u32, Vec<u8>),
    blurred: (u32, u32, Vec<u8>),
    edges: (u32, u32, Vec<u8>),
    contours: Vec<Polyline>,
    simplified: Vec<Polyline>,
    masked: Option<Vec<Polyline>>,
    joined: Polyline,
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
            downsampled: (
                self.downsampled.width(),
                self.downsampled.height(),
                self.downsampled.as_raw().clone(),
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
            masked: self.masked.clone(),
            joined: self.joined.clone(),
            dimensions: self.dimensions,
        };
        // Note: the proxy stores blurred as (w, h, Vec<u8>) — the raw
        // bytes now contain 4 bytes per pixel (RGBA) instead of 1.
        proxy.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for StagedResult {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let proxy = StagedResultProxy::deserialize(deserializer)?;

        let original = RgbaImage::from_raw(proxy.original.0, proxy.original.1, proxy.original.2)
            .ok_or_else(|| serde::de::Error::custom("invalid RGBA image dimensions"))?;
        let downsampled = RgbaImage::from_raw(
            proxy.downsampled.0,
            proxy.downsampled.1,
            proxy.downsampled.2,
        )
        .ok_or_else(|| serde::de::Error::custom("invalid downsampled image dimensions"))?;
        let blurred = RgbaImage::from_raw(proxy.blurred.0, proxy.blurred.1, proxy.blurred.2)
            .ok_or_else(|| serde::de::Error::custom("invalid blurred image dimensions"))?;
        let edges = GrayImage::from_raw(proxy.edges.0, proxy.edges.1, proxy.edges.2)
            .ok_or_else(|| serde::de::Error::custom("invalid edges image dimensions"))?;

        Ok(Self {
            original,
            downsampled,
            blurred,
            edges,
            contours: proxy.contours,
            simplified: proxy.simplified,
            masked: proxy.masked,
            joined: proxy.joined,
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
#[allow(clippy::unwrap_used)]
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
        assert!((config.canny_low - 15.0).abs() < f32::EPSILON);
        assert!((config.canny_high - 40.0).abs() < f32::EPSILON);
        assert!((config.canny_max - 60.0).abs() < f32::EPSILON);
        assert_eq!(config.contour_tracer, ContourTracerKind::BorderFollowing);
        assert!((config.simplify_tolerance - 2.0).abs() < f64::EPSILON);
        assert_eq!(config.path_joiner, PathJoinerKind::Mst);
        assert!(config.circular_mask);
        assert!((config.mask_diameter - 0.75).abs() < f64::EPSILON);
        assert!(!config.invert);
        assert_eq!(config.working_resolution, 1000);
        assert_eq!(config.downsample_filter, DownsampleFilter::Triangle);
        assert_eq!(config.mst_neighbours, 100);
        assert_eq!(config.edge_channels, EdgeChannels::default());
        assert!(config.edge_channels.luminance);
        assert!(!config.edge_channels.red);
        assert!(!config.edge_channels.green);
        assert!(!config.edge_channels.blue);
        assert!(!config.edge_channels.saturation);
    }

    #[test]
    fn pipeline_eq_ignores_canny_max() {
        let a = PipelineConfig::default();
        let mut b = a.clone();
        b.canny_max = a.canny_max + 100.0;

        // PartialEq sees the difference.
        assert_ne!(a, b);
        // pipeline_eq ignores it.
        assert!(a.pipeline_eq(&b));
    }

    #[test]
    fn pipeline_eq_detects_processing_field_change() {
        let a = PipelineConfig::default();

        let mut b = a.clone();
        b.canny_low += 1.0;
        assert!(!a.pipeline_eq(&b), "canny_low change should be detected");

        let mut b = a.clone();
        b.canny_high += 1.0;
        assert!(!a.pipeline_eq(&b), "canny_high change should be detected");

        let mut b = a.clone();
        b.blur_sigma += 0.1;
        assert!(!a.pipeline_eq(&b), "blur_sigma change should be detected");

        let mut b = a.clone();
        b.invert = !a.invert;
        assert!(!a.pipeline_eq(&b), "invert change should be detected");

        let mut b = a.clone();
        b.circular_mask = !a.circular_mask;
        assert!(
            !a.pipeline_eq(&b),
            "circular_mask change should be detected"
        );

        // ContourTracerKind currently has only one variant (BorderFollowing).
        // Uncomment when a second variant is added:
        // let mut b = a.clone();
        // b.contour_tracer = ContourTracerKind::NewVariant;
        // assert!(!a.pipeline_eq(&b), "contour_tracer change should be detected");

        let mut b = a.clone();
        b.simplify_tolerance += 0.5;
        assert!(
            !a.pipeline_eq(&b),
            "simplify_tolerance change should be detected"
        );

        let mut b = a.clone();
        b.path_joiner = PathJoinerKind::StraightLine;
        assert!(!a.pipeline_eq(&b), "path_joiner change should be detected");

        let mut b = a.clone();
        b.mask_diameter -= 0.1;
        assert!(
            !a.pipeline_eq(&b),
            "mask_diameter change should be detected"
        );

        let mut b = a.clone();
        b.mst_neighbours += 10;
        assert!(
            !a.pipeline_eq(&b),
            "mst_neighbours change should be detected"
        );

        let mut b = a.clone();
        b.edge_channels.red = true;
        assert!(
            !a.pipeline_eq(&b),
            "edge_channels change should be detected"
        );
    }

    #[test]
    fn validate_default_config_is_valid() {
        PipelineConfig::default().validate().unwrap();
    }

    #[test]
    fn validate_rejects_no_edge_channels() {
        let config = PipelineConfig {
            edge_channels: EdgeChannels {
                luminance: false,
                red: false,
                green: false,
                blue: false,
                saturation: false,
            },
            ..PipelineConfig::default()
        };
        let err = config.validate().unwrap_err();
        assert!(
            matches!(err, PipelineError::InvalidConfig(ref s) if s.contains("edge channel")),
            "expected InvalidConfig about edge channels, got {err:?}",
        );
    }

    #[test]
    fn validate_accepts_mask_diameter_zero() {
        let config = PipelineConfig {
            mask_diameter: 0.0,
            ..PipelineConfig::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn validate_accepts_mask_diameter_max() {
        let config = PipelineConfig {
            mask_diameter: 1.5,
            ..PipelineConfig::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn validate_accepts_mask_diameter_above_one() {
        let config = PipelineConfig {
            mask_diameter: 1.3,
            ..PipelineConfig::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn validate_rejects_mask_diameter_negative() {
        let config = PipelineConfig {
            mask_diameter: -0.01,
            ..PipelineConfig::default()
        };
        let err = config.validate().unwrap_err();
        assert!(
            matches!(err, PipelineError::InvalidConfig(ref s) if s.contains("mask_diameter")),
            "expected InvalidConfig about mask_diameter, got {err:?}",
        );
    }

    #[test]
    fn validate_rejects_mask_diameter_above_max() {
        let config = PipelineConfig {
            mask_diameter: 1.51,
            ..PipelineConfig::default()
        };
        let err = config.validate().unwrap_err();
        assert!(
            matches!(err, PipelineError::InvalidConfig(ref s) if s.contains("mask_diameter")),
            "expected InvalidConfig about mask_diameter, got {err:?}",
        );
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
        let p = Point::new(3.25, -2.71);
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
            canny_max: 200.0,
            contour_tracer: ContourTracerKind::BorderFollowing,
            simplify_tolerance: 1.5,
            path_joiner: PathJoinerKind::Retrace,
            circular_mask: true,
            mask_diameter: 0.85,
            invert: true,
            working_resolution: 256,
            downsample_filter: DownsampleFilter::Triangle,
            mst_neighbours: 20,
            edge_channels: EdgeChannels {
                luminance: true,
                red: true,
                green: false,
                blue: false,
                saturation: true,
            },
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: PipelineConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn pipeline_config_deserializes_without_edge_channels() {
        // Old configs from before edge_channels was added should still
        // deserialize, falling back to EdgeChannels::default() (luminance only).
        let json = r#"{
            "blur_sigma": 1.4,
            "canny_low": 15.0,
            "canny_high": 40.0,
            "canny_max": 60.0,
            "contour_tracer": "BorderFollowing",
            "simplify_tolerance": 2.0,
            "path_joiner": "Mst",
            "circular_mask": true,
            "mask_diameter": 1.0,
            "invert": false,
            "working_resolution": 256,
            "downsample_filter": "Disabled",
            "mst_neighbours": 100
        }"#;
        let config: PipelineConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.edge_channels, EdgeChannels::default());
        assert!(config.edge_channels.luminance);
        assert!(!config.edge_channels.red);
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
            downsampled: RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255])),
            blurred: RgbaImage::from_pixel(2, 2, image::Rgba([40, 45, 50, 255])),
            edges: GrayImage::from_pixel(2, 2, image::Luma([255])),
            contours: vec![Polyline::new(vec![
                Point::new(0.0, 0.0),
                Point::new(1.0, 1.0),
            ])],
            simplified: vec![Polyline::new(vec![
                Point::new(0.0, 0.0),
                Point::new(1.0, 1.0),
            ])],
            masked: None,
            joined: Polyline::new(vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)]),
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
        assert_eq!(
            staged.downsampled.as_raw(),
            deserialized.downsampled.as_raw()
        );
        assert_eq!(staged.blurred.as_raw(), deserialized.blurred.as_raw());
        assert_eq!(staged.edges.as_raw(), deserialized.edges.as_raw());

        // Verify vector data survived.
        assert_eq!(staged.contours, deserialized.contours);
        assert_eq!(staged.simplified, deserialized.simplified);
        assert_eq!(staged.masked, deserialized.masked);
        assert_eq!(staged.joined, deserialized.joined);
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
            downsampled: RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 255])),
            blurred: RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 255])),
            edges: GrayImage::from_pixel(1, 1, image::Luma([0])),
            contours: vec![],
            simplified: vec![],
            masked: None,
            joined: Polyline::new(vec![]),
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

    // ─────────── earliest_changed_stage tests ────────────────────

    #[test]
    fn earliest_changed_stage_identical_configs() {
        let a = PipelineConfig::default();
        let b = PipelineConfig::default();
        assert_eq!(a.earliest_changed_stage(&b), crate::pipeline::STAGE_COUNT,);
    }

    #[test]
    fn earliest_changed_stage_canny_max_is_ui_only() {
        let a = PipelineConfig::default();
        let b = PipelineConfig {
            canny_max: 200.0,
            ..PipelineConfig::default()
        };
        assert_eq!(a.earliest_changed_stage(&b), crate::pipeline::STAGE_COUNT,);
    }

    #[test]
    fn earliest_changed_stage_working_resolution() {
        let a = PipelineConfig::default();
        let b = PipelineConfig {
            working_resolution: 512,
            ..PipelineConfig::default()
        };
        assert_eq!(a.earliest_changed_stage(&b), 2);
    }

    #[test]
    fn earliest_changed_stage_downsample_filter() {
        let a = PipelineConfig::default();
        let b = PipelineConfig {
            downsample_filter: crate::DownsampleFilter::Lanczos3,
            ..PipelineConfig::default()
        };
        assert_eq!(a.earliest_changed_stage(&b), 2);
    }

    #[test]
    fn earliest_changed_stage_blur_sigma() {
        let a = PipelineConfig::default();
        let b = PipelineConfig {
            blur_sigma: 3.0,
            ..PipelineConfig::default()
        };
        assert_eq!(a.earliest_changed_stage(&b), 3);
    }

    #[test]
    fn earliest_changed_stage_canny_low() {
        let a = PipelineConfig::default();
        let b = PipelineConfig {
            canny_low: 20.0,
            ..PipelineConfig::default()
        };
        assert_eq!(a.earliest_changed_stage(&b), 4);
    }

    #[test]
    fn earliest_changed_stage_canny_high() {
        let a = PipelineConfig::default();
        let b = PipelineConfig {
            canny_high: 50.0,
            ..PipelineConfig::default()
        };
        assert_eq!(a.earliest_changed_stage(&b), 4);
    }

    #[test]
    fn earliest_changed_stage_invert() {
        let a = PipelineConfig::default();
        let b = PipelineConfig {
            invert: true,
            ..PipelineConfig::default()
        };
        assert_eq!(a.earliest_changed_stage(&b), 4);
    }

    #[test]
    fn earliest_changed_stage_edge_channels() {
        let a = PipelineConfig::default();
        let b = PipelineConfig {
            edge_channels: EdgeChannels {
                luminance: true,
                red: true,
                ..EdgeChannels::default()
            },
            ..PipelineConfig::default()
        };
        assert_eq!(a.earliest_changed_stage(&b), 4);
    }

    #[test]
    fn earliest_changed_stage_simplify_tolerance() {
        let a = PipelineConfig::default();
        let b = PipelineConfig {
            simplify_tolerance: 5.0,
            ..PipelineConfig::default()
        };
        assert_eq!(a.earliest_changed_stage(&b), 6);
    }

    #[test]
    fn earliest_changed_stage_circular_mask() {
        let a = PipelineConfig::default();
        let b = PipelineConfig {
            circular_mask: !a.circular_mask,
            ..PipelineConfig::default()
        };
        assert_eq!(a.earliest_changed_stage(&b), 7);
    }

    #[test]
    fn earliest_changed_stage_mask_diameter() {
        let a = PipelineConfig::default();
        let b = PipelineConfig {
            mask_diameter: 1.2,
            ..PipelineConfig::default()
        };
        assert_eq!(a.earliest_changed_stage(&b), 7);
    }

    #[test]
    fn earliest_changed_stage_path_joiner() {
        let a = PipelineConfig::default();
        let b = PipelineConfig {
            path_joiner: crate::PathJoinerKind::StraightLine,
            ..PipelineConfig::default()
        };
        assert_eq!(a.earliest_changed_stage(&b), 8);
    }

    #[test]
    fn earliest_changed_stage_mst_neighbours() {
        let a = PipelineConfig::default();
        let b = PipelineConfig {
            mst_neighbours: 50,
            ..PipelineConfig::default()
        };
        assert_eq!(a.earliest_changed_stage(&b), 8);
    }

    #[test]
    fn earliest_changed_stage_returns_earliest() {
        // When both blur_sigma (stage 3) and mask_diameter (stage 7)
        // change, the earliest stage should be 3.
        let a = PipelineConfig::default();
        let b = PipelineConfig {
            blur_sigma: 5.0,
            mask_diameter: 1.2,
            ..PipelineConfig::default()
        };
        assert_eq!(a.earliest_changed_stage(&b), 3);
    }

    #[test]
    fn earliest_changed_stage_consistent_with_pipeline_eq() {
        // If pipeline_eq returns true, earliest_changed_stage should
        // return STAGE_COUNT (no change).
        let a = PipelineConfig::default();
        let b = PipelineConfig {
            canny_max: 200.0,
            ..PipelineConfig::default()
        };
        assert!(a.pipeline_eq(&b));
        assert_eq!(a.earliest_changed_stage(&b), crate::pipeline::STAGE_COUNT,);

        // If pipeline_eq returns false, earliest_changed_stage should
        // return a value less than STAGE_COUNT.
        let c = PipelineConfig {
            blur_sigma: 5.0,
            ..PipelineConfig::default()
        };
        assert!(!a.pipeline_eq(&c));
        assert!(a.earliest_changed_stage(&c) < crate::pipeline::STAGE_COUNT);
    }
}
