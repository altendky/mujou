//! Incremental pipeline: advance stage-by-stage, inspecting each
//! intermediate result before continuing.
//!
//! Unlike [`crate::process_staged`] which runs the entire pipeline in one
//! call, [`Pipeline`] lets the caller drive execution one step at a time:
//!
//! ```rust
//! # use mujou_pipeline::{Pipeline, PipelineConfig, PipelineError};
//! # fn run(png: Vec<u8>) -> Result<(), PipelineError> {
//! let config = PipelineConfig::default();
//! let pipeline = Pipeline::new(png, config)
//!     .decode()?
//!     .downsample()
//!     .blur()
//!     .detect_edges()
//!     .trace_contours()?
//!     .simplify()
//!     .mask()
//!     .join();
//!
//! let staged = pipeline.into_result();
//! # Ok(())
//! # }
//! ```
//!
//! Each stage method consumes `self` and returns the next pipeline state
//! (or `Result` for fallible stages), carrying all previously computed
//! intermediates. The caller can inspect the current stage's output via
//! accessor methods at any point.
//!
//! # Memory
//!
//! Every stage from [`ContoursTraced`] onward retains the full raster
//! stack (original RGBA, blurred, and edge images) alongside
//! the growing vector data. For a 1000×1000 source image this is roughly
//! 7 MB of raster data pinned in memory until [`Joined::into_result`]
//! consumes the final stage. This is intentional: [`StagedResult`] needs
//! every intermediate for visualization and export.
//!
//! Callers that only need the final polyline (and not the intermediate
//! images) should prefer [`crate::process`], which discards the raster
//! intermediates and returns only the output path and dimensions.

use image::DynamicImage;

use crate::contour::ContourTracer;
use crate::diagnostics::StageMetrics;
use crate::join::PathJoiner;
use crate::types::{
    Dimensions, GrayImage, PipelineConfig, PipelineError, Point, Polyline, RgbaImage, StagedResult,
};

// ───────────────────────── Stage 0: Pending ──────────────────────────

/// Pipeline state before any processing has occurred.
///
/// The source image bytes and config are stored but not yet touched.
/// Call [`decode`](Self::decode) to advance to the next stage.
#[must_use = "pipeline stages are consumed by advancing — call .decode() to continue"]
pub struct Pending {
    config: PipelineConfig,
    source: Vec<u8>,
}

impl Pending {
    /// The raw source image bytes.
    #[must_use]
    pub fn source(&self) -> &[u8] {
        &self.source
    }

    /// Decode the source image and advance to the [`Decoded`] stage.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::EmptyInput`] if the source bytes are
    /// empty. Returns [`PipelineError::ImageDecode`] if the image
    /// format is unrecognized or the data is corrupt.
    pub fn decode(self) -> Result<Decoded, PipelineError> {
        let source_len = self.source.len();
        let image = crate::grayscale::decode(&self.source)?;
        let original = crate::grayscale::to_rgba(&image);
        Ok(Decoded {
            config: self.config,
            image,
            original,
            source_len,
        })
    }
}

// ───────────────────────── Stage 1: Decoded ──────────────────────────

/// Pipeline state after decoding the source image.
///
/// The raw image has been decoded into a [`DynamicImage`] and converted
/// to RGBA. Call [`downsample`](Self::downsample) to advance to the next
/// stage.
#[must_use = "pipeline stages are consumed by advancing — call .downsample() to continue"]
pub struct Decoded {
    config: PipelineConfig,
    image: DynamicImage,
    original: RgbaImage,
    source_len: usize,
}

impl Decoded {
    /// The original decoded RGBA image.
    #[must_use]
    pub const fn original(&self) -> &RgbaImage {
        &self.original
    }

    /// Advance to the downsample stage.
    pub fn downsample(self) -> Downsampled {
        let (downsampled_dynamic, applied) = crate::downsample::downsample(
            &self.image,
            self.config.working_resolution,
            self.config.downsample_filter,
        );
        let downsampled = crate::grayscale::to_rgba(&downsampled_dynamic);
        Downsampled {
            config: self.config,
            original: self.original,
            rgba: downsampled,
            applied,
        }
    }
}

// ───────────────────────── Stage 2: Downsampled ──────────────────────

/// Pipeline state after downsampling to working resolution.
///
/// The decoded image has been downsampled so the longest axis matches
/// `config.working_resolution`. Call [`blur`](Self::blur) to
/// advance to the next stage.
#[must_use = "pipeline stages are consumed by advancing — call .blur() to continue"]
#[allow(clippy::struct_field_names)]
pub struct Downsampled {
    config: PipelineConfig,
    original: RgbaImage,
    rgba: RgbaImage,
    applied: bool,
}

impl Downsampled {
    /// The downsampled RGBA image.
    #[must_use]
    pub const fn downsampled(&self) -> &RgbaImage {
        &self.rgba
    }

    /// Whether downsampling was actually applied (image was larger than
    /// `working_resolution`).
    #[must_use]
    pub const fn applied(&self) -> bool {
        self.applied
    }

    /// Advance to the blur stage.
    ///
    /// Applies Gaussian blur to the full RGBA image so the UI preview
    /// shows color (not grayscale). Downstream edge detection extracts
    /// channels from the already-blurred RGBA — no per-channel blur
    /// needed.
    pub fn blur(self) -> Blurred {
        let dimensions = Dimensions {
            width: self.rgba.width(),
            height: self.rgba.height(),
        };
        let smooth = crate::blur::gaussian_blur_rgba(&self.rgba, self.config.blur_sigma);
        Blurred {
            config: self.config,
            original: self.original,
            downsampled: self.rgba,
            smooth,
            dimensions,
        }
    }
}

// ───────────────────────── Stage 3: Blurred ──────────────────────────

/// Pipeline state after Gaussian blur.
///
/// The blur operates on the full RGBA image so the UI preview shows
/// color. Downstream edge detection extracts channels from this
/// already-blurred image.
///
/// Call [`detect_edges`](Self::detect_edges) to advance to the next stage.
#[must_use = "pipeline stages are consumed by advancing — call .detect_edges() to continue"]
pub struct Blurred {
    config: PipelineConfig,
    original: RgbaImage,
    downsampled: RgbaImage,
    smooth: RgbaImage,
    dimensions: Dimensions,
}

impl Blurred {
    /// The blurred RGBA image.
    #[must_use]
    pub const fn blurred(&self) -> &RgbaImage {
        &self.smooth
    }

    /// Advance to the edge detection stage.
    ///
    /// Runs Canny edge detection on each enabled channel (see
    /// [`EdgeChannels`](crate::types::EdgeChannels)) and combines the
    /// results via pixel-wise maximum. Optionally inverts the combined
    /// edge map when `config.invert` is `true`.
    ///
    /// All channels are extracted from the already-blurred RGBA image,
    /// so no per-channel blurring is needed.
    pub fn detect_edges(self) -> EdgesDetected {
        let edges_raw = crate::edge::canny_combined(
            &self.smooth,
            &self.config.edge_channels,
            self.config.canny_low,
            self.config.canny_high,
        );
        let pre_invert_edge_pixels = crate::diagnostics::count_edge_pixels(&edges_raw);
        let edge_map = if self.config.invert {
            crate::edge::invert_edge_map(&edges_raw)
        } else {
            edges_raw
        };
        EdgesDetected {
            config: self.config,
            original: self.original,
            downsampled: self.downsampled,
            blurred: self.smooth,
            edge_map,
            pre_invert_edge_pixels,
            dimensions: self.dimensions,
        }
    }
}

// ───────────────────────── Stage 4: EdgesDetected ────────────────────

/// Pipeline state after Canny edge detection (and optional inversion).
///
/// Call [`trace_contours`](Self::trace_contours) to advance to the next
/// stage. This is a fallible step — it returns `Err` if no contours are
/// found.
#[must_use = "pipeline stages are consumed by advancing — call .trace_contours() to continue"]
pub struct EdgesDetected {
    config: PipelineConfig,
    original: RgbaImage,
    downsampled: RgbaImage,
    blurred: RgbaImage,
    edge_map: GrayImage,
    /// Edge pixel count from Canny output, before optional inversion.
    pre_invert_edge_pixels: u64,
    dimensions: Dimensions,
}

impl EdgesDetected {
    /// The binary edge map.
    #[must_use]
    pub const fn edges(&self) -> &GrayImage {
        &self.edge_map
    }

    /// Advance to the contour tracing stage.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::NoContours`] if the edge map produces no
    /// traceable contours.
    pub fn trace_contours(self) -> Result<ContoursTraced, PipelineError> {
        let contours = self.config.contour_tracer.trace(&self.edge_map);
        if contours.is_empty() {
            return Err(PipelineError::NoContours);
        }
        Ok(ContoursTraced {
            config: self.config,
            original: self.original,
            downsampled: self.downsampled,
            blurred: self.blurred,
            edges: self.edge_map,
            contours,
            dimensions: self.dimensions,
        })
    }
}

// ───────────────────────── Stage 5: ContoursTraced ───────────────────

/// Pipeline state after contour tracing.
///
/// Call [`simplify`](Self::simplify) to advance to the next stage.
///
/// See the [module-level memory notes](self#memory) for the cost of
/// retaining all prior raster intermediates.
#[must_use = "pipeline stages are consumed by advancing — call .simplify() to continue"]
pub struct ContoursTraced {
    config: PipelineConfig,
    original: RgbaImage,
    downsampled: RgbaImage,
    blurred: RgbaImage,
    edges: GrayImage,
    contours: Vec<Polyline>,
    dimensions: Dimensions,
}

impl ContoursTraced {
    /// The traced contour polylines.
    #[must_use]
    pub fn contours(&self) -> &[Polyline] {
        &self.contours
    }

    /// Advance to the simplification stage.
    pub fn simplify(self) -> Simplified {
        let reduced =
            crate::simplify::simplify_paths(&self.contours, self.config.simplify_tolerance);
        Simplified {
            config: self.config,
            original: self.original,
            downsampled: self.downsampled,
            blurred: self.blurred,
            edges: self.edges,
            contours: self.contours,
            reduced,
            dimensions: self.dimensions,
        }
    }
}

// ───────────────────────── Stage 6: Simplified ───────────────────────

/// Pipeline state after path simplification (RDP).
///
/// Call [`mask`](Self::mask) to advance to the next stage.
///
/// See the [module-level memory notes](self#memory) for the cost of
/// retaining all prior raster intermediates.
#[must_use = "pipeline stages are consumed by advancing — call .mask() to continue"]
pub struct Simplified {
    config: PipelineConfig,
    original: RgbaImage,
    downsampled: RgbaImage,
    blurred: RgbaImage,
    edges: GrayImage,
    contours: Vec<Polyline>,
    reduced: Vec<Polyline>,
    dimensions: Dimensions,
}

impl Simplified {
    /// The simplified polylines.
    #[must_use]
    pub fn simplified(&self) -> &[Polyline] {
        &self.reduced
    }

    /// Advance to the masking stage.
    ///
    /// When `config.circular_mask` is `true`, polylines are clipped to a
    /// circular boundary. When `false`, this is a no-op pass-through.
    pub fn mask(self) -> Masked {
        let clipped = if self.config.circular_mask {
            let center = Point::new(
                f64::from(self.dimensions.width) / 2.0,
                f64::from(self.dimensions.height) / 2.0,
            );
            let w = f64::from(self.dimensions.width);
            let h = f64::from(self.dimensions.height);
            let diagonal = w.hypot(h);
            let radius = diagonal * self.config.mask_diameter / 2.0;
            Some(crate::mask::apply_circular_mask(
                &self.reduced,
                center,
                radius,
            ))
        } else {
            None
        };
        Masked {
            config: self.config,
            original: self.original,
            downsampled: self.downsampled,
            blurred: self.blurred,
            edges: self.edges,
            contours: self.contours,
            simplified: self.reduced,
            clipped,
            dimensions: self.dimensions,
        }
    }
}

// ───────────────────────── Stage 7: Masked ───────────────────────────

/// Pipeline state after optional circular masking.
///
/// Call [`join`](Self::join) to advance to the final stage.
///
/// See the [module-level memory notes](self#memory) for the cost of
/// retaining all prior raster intermediates.
#[must_use = "pipeline stages are consumed by advancing — call .join() to continue"]
pub struct Masked {
    config: PipelineConfig,
    original: RgbaImage,
    downsampled: RgbaImage,
    blurred: RgbaImage,
    edges: GrayImage,
    contours: Vec<Polyline>,
    simplified: Vec<Polyline>,
    clipped: Option<Vec<Polyline>>,
    dimensions: Dimensions,
}

impl Masked {
    /// The masked polylines, or `None` if masking was disabled.
    #[must_use]
    pub fn masked(&self) -> Option<&[Polyline]> {
        self.clipped.as_deref()
    }

    /// Advance to the joining stage — the final pipeline step.
    pub fn join(self) -> Joined {
        let join_input = self.clipped.as_deref().unwrap_or(&self.simplified);
        let path = self.config.path_joiner.join(join_input, &self.config);
        Joined {
            config: self.config,
            original: self.original,
            downsampled: self.downsampled,
            blurred: self.blurred,
            edges: self.edges,
            contours: self.contours,
            simplified: self.simplified,
            masked: self.clipped,
            path,
            dimensions: self.dimensions,
        }
    }
}

// ───────────────────────── Stage 8: Joined ───────────────────────────

/// Pipeline state after path joining — the final stage.
///
/// Call [`into_result`](Self::into_result) to extract the
/// [`StagedResult`] containing all intermediates.
///
/// See the [module-level memory notes](self#memory) for the cost of
/// retaining all prior raster intermediates.
#[must_use = "call .into_result() to extract the StagedResult"]
pub struct Joined {
    config: PipelineConfig,
    original: RgbaImage,
    downsampled: RgbaImage,
    blurred: RgbaImage,
    edges: GrayImage,
    contours: Vec<Polyline>,
    simplified: Vec<Polyline>,
    masked: Option<Vec<Polyline>>,
    path: Polyline,
    dimensions: Dimensions,
}

impl Joined {
    /// The single continuous output path.
    #[must_use]
    pub const fn joined(&self) -> &Polyline {
        &self.path
    }

    /// Image dimensions.
    #[must_use]
    pub const fn dimensions(&self) -> Dimensions {
        self.dimensions
    }

    /// Consume the pipeline and return the full [`StagedResult`].
    #[must_use]
    pub fn into_result(self) -> StagedResult {
        StagedResult {
            original: self.original,
            downsampled: self.downsampled,
            blurred: self.blurred,
            edges: self.edges,
            contours: self.contours,
            simplified: self.simplified,
            masked: self.masked,
            joined: self.path,
            dimensions: self.dimensions,
        }
    }
}

// ──────────────────── PipelineStage trait + Stage enum ────────────────

/// Total number of stages in the pipeline.
pub const STAGE_COUNT: usize = 9;

/// The output produced by a single pipeline stage.
///
/// Each variant borrows the data that the corresponding stage computed.
/// Use this with [`PipelineStage::output`] or [`Stage::output`] to
/// inspect intermediates in a uniform, type-erased way.
#[must_use]
pub enum StageOutput<'a> {
    /// Source image bytes (not yet decoded).
    Source {
        /// The raw image bytes.
        bytes: &'a [u8],
    },
    /// Decoded RGBA image.
    Decoded {
        /// The original image.
        original: &'a RgbaImage,
    },
    /// Downsampled RGBA image (working resolution).
    Downsampled {
        /// The downsampled image.
        downsampled: &'a RgbaImage,
    },
    /// Gaussian blur result.
    Blurred {
        /// The blurred RGBA image.
        blurred: &'a RgbaImage,
    },
    /// Edge detection result.
    EdgesDetected {
        /// The binary edge map.
        edges: &'a GrayImage,
    },
    /// Contour tracing result.
    ContoursTraced {
        /// The traced polylines.
        contours: &'a [Polyline],
    },
    /// Path simplification result.
    Simplified {
        /// The simplified polylines.
        simplified: &'a [Polyline],
    },
    /// Circular mask result.
    Masked {
        /// The masked polylines, or `None` if masking was disabled.
        masked: Option<&'a [Polyline]>,
    },
    /// Path joining result.
    Joined {
        /// The single continuous output path.
        joined: &'a Polyline,
        /// Image dimensions.
        dimensions: Dimensions,
    },
}

/// Trait implemented by every pipeline stage, enabling uniform iteration.
///
/// Both the typed API (individual stage structs) and the dynamic API
/// ([`Stage`] enum) are available. This trait bridges the two: each
/// stage struct implements it, and [`Stage`] delegates to whichever
/// variant it holds.
///
/// # Loop pattern
///
/// ```rust
/// # use mujou_pipeline::{Pipeline, PipelineConfig, PipelineError};
/// # use mujou_pipeline::pipeline::{Stage, PipelineStage, Advance};
/// # fn run(png: Vec<u8>) -> Result<(), PipelineError> {
/// let mut stage: Stage = Pipeline::new(png, PipelineConfig::default()).into();
/// loop {
///     match stage.advance()? {
///         Advance::Next(next) => stage = next,
///         Advance::Complete(done) => { stage = done; break; }
///     }
/// }
/// let result = stage.complete()?;
/// # Ok(())
/// # }
/// ```
pub trait PipelineStage: Sized {
    /// Human-readable name of this stage (e.g. `"source"`, `"blur"`).
    const NAME: &str;

    /// Zero-based index of this stage (`0` for Pending through `8` for
    /// Joined).
    const INDEX: usize;

    /// The output this stage produced.
    fn output(&self) -> StageOutput<'_>;

    /// Stage-specific metrics for diagnostics.
    ///
    /// Returns `None` for the initial [`Pending`] stage which has not
    /// yet performed any processing. All other stages return
    /// `Some(metrics)` describing the work done to reach this state.
    fn metrics(&self) -> Option<StageMetrics>;

    /// Invert-specific metrics, if this stage performed edge inversion.
    ///
    /// Only [`EdgesDetected`] returns `Some` (and only when
    /// `config.invert` was `true`). All other stages return `None`.
    fn invert_metrics(&self) -> Option<StageMetrics> {
        None
    }

    /// Advance to the next stage.
    ///
    /// Returns `Ok(Some(stage))` on success, `Ok(None)` if already at
    /// the final stage, or `Err` if the stage transition fails.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::EmptyInput`] or
    /// [`PipelineError::ImageDecode`] when decoding fails, and
    /// [`PipelineError::NoContours`] when contour tracing produces no
    /// contours.
    fn next(self) -> Result<Option<Stage>, PipelineError>;

    /// Run all remaining stages to completion and return the final
    /// [`StagedResult`].
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`] if any remaining fallible stage fails.
    fn complete(self) -> Result<StagedResult, PipelineError>;
}

impl PipelineStage for Pending {
    const NAME: &str = "source";
    const INDEX: usize = 0;

    fn output(&self) -> StageOutput<'_> {
        StageOutput::Source {
            bytes: &self.source,
        }
    }

    fn metrics(&self) -> Option<StageMetrics> {
        None
    }

    fn next(self) -> Result<Option<Stage>, PipelineError> {
        Ok(Some(Stage::Decoded(self.decode()?)))
    }

    fn complete(self) -> Result<StagedResult, PipelineError> {
        self.decode()?.complete()
    }
}

impl PipelineStage for Decoded {
    const NAME: &str = "decode";
    const INDEX: usize = 1;

    fn output(&self) -> StageOutput<'_> {
        StageOutput::Decoded {
            original: &self.original,
        }
    }

    fn metrics(&self) -> Option<StageMetrics> {
        Some(StageMetrics::Decode {
            input_bytes: self.source_len,
            width: self.original.width(),
            height: self.original.height(),
            pixel_count: u64::from(self.original.width()) * u64::from(self.original.height()),
        })
    }

    fn next(self) -> Result<Option<Stage>, PipelineError> {
        Ok(Some(Stage::Downsampled(self.downsample())))
    }

    fn complete(self) -> Result<StagedResult, PipelineError> {
        self.downsample().complete()
    }
}

impl PipelineStage for Downsampled {
    const NAME: &str = "downsample";
    const INDEX: usize = 2;

    fn output(&self) -> StageOutput<'_> {
        StageOutput::Downsampled {
            downsampled: &self.rgba,
        }
    }

    fn metrics(&self) -> Option<StageMetrics> {
        Some(StageMetrics::Downsample {
            original_width: self.original.width(),
            original_height: self.original.height(),
            width: self.rgba.width(),
            height: self.rgba.height(),
            max_dimension: self.config.working_resolution,
            filter: self.config.downsample_filter.to_string(),
            applied: self.applied,
        })
    }

    fn next(self) -> Result<Option<Stage>, PipelineError> {
        Ok(Some(Stage::Blurred(self.blur())))
    }

    fn complete(self) -> Result<StagedResult, PipelineError> {
        self.blur().complete()
    }
}

impl PipelineStage for Blurred {
    const NAME: &str = "blur";
    const INDEX: usize = 3;

    fn output(&self) -> StageOutput<'_> {
        StageOutput::Blurred {
            blurred: &self.smooth,
        }
    }

    fn metrics(&self) -> Option<StageMetrics> {
        Some(StageMetrics::Blur {
            sigma: self.config.blur_sigma,
        })
    }

    fn next(self) -> Result<Option<Stage>, PipelineError> {
        Ok(Some(Stage::EdgesDetected(self.detect_edges())))
    }

    fn complete(self) -> Result<StagedResult, PipelineError> {
        self.detect_edges().complete()
    }
}

impl PipelineStage for EdgesDetected {
    const NAME: &str = "edges";
    const INDEX: usize = 4;

    fn output(&self) -> StageOutput<'_> {
        StageOutput::EdgesDetected {
            edges: &self.edge_map,
        }
    }

    fn metrics(&self) -> Option<StageMetrics> {
        let total_pixel_count =
            u64::from(self.edge_map.width()) * u64::from(self.edge_map.height());
        let (low_threshold, high_threshold) =
            crate::edge::clamp_thresholds(self.config.canny_low, self.config.canny_high);
        Some(StageMetrics::EdgeDetection {
            low_threshold,
            high_threshold,
            edge_pixel_count: self.pre_invert_edge_pixels,
            total_pixel_count,
            channel_count: self.config.edge_channels.count(),
        })
    }

    fn invert_metrics(&self) -> Option<StageMetrics> {
        if self.config.invert {
            Some(StageMetrics::Invert {
                edge_pixel_count: crate::diagnostics::count_edge_pixels(&self.edge_map),
            })
        } else {
            None
        }
    }

    fn next(self) -> Result<Option<Stage>, PipelineError> {
        Ok(Some(Stage::ContoursTraced(self.trace_contours()?)))
    }

    fn complete(self) -> Result<StagedResult, PipelineError> {
        self.trace_contours()?.complete()
    }
}

impl PipelineStage for ContoursTraced {
    const NAME: &str = "contours";
    const INDEX: usize = 5;

    fn output(&self) -> StageOutput<'_> {
        StageOutput::ContoursTraced {
            contours: &self.contours,
        }
    }

    fn metrics(&self) -> Option<StageMetrics> {
        let stats = crate::diagnostics::contour_stats(&self.contours);
        Some(StageMetrics::ContourTracing {
            contour_count: self.contours.len(),
            total_point_count: stats.total,
            min_contour_points: stats.min,
            max_contour_points: stats.max,
            mean_contour_points: stats.mean,
        })
    }

    fn next(self) -> Result<Option<Stage>, PipelineError> {
        Ok(Some(Stage::Simplified(self.simplify())))
    }

    fn complete(self) -> Result<StagedResult, PipelineError> {
        self.simplify().complete()
    }
}

impl PipelineStage for Simplified {
    const NAME: &str = "simplify";
    const INDEX: usize = 6;

    fn output(&self) -> StageOutput<'_> {
        StageOutput::Simplified {
            simplified: &self.reduced,
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn metrics(&self) -> Option<StageMetrics> {
        let points_before: usize = self.contours.iter().map(Polyline::len).sum();
        let points_after: usize = self.reduced.iter().map(Polyline::len).sum();
        let reduction_ratio = if points_before > 0 {
            1.0 - (points_after as f64 / points_before as f64)
        } else {
            0.0
        };
        Some(StageMetrics::Simplification {
            tolerance: self.config.simplify_tolerance,
            polyline_count: self.reduced.len(),
            points_before,
            points_after,
            reduction_ratio,
        })
    }

    fn next(self) -> Result<Option<Stage>, PipelineError> {
        Ok(Some(Stage::Masked(self.mask())))
    }

    fn complete(self) -> Result<StagedResult, PipelineError> {
        self.mask().complete()
    }
}

impl PipelineStage for Masked {
    const NAME: &str = "mask";
    const INDEX: usize = 7;

    fn output(&self) -> StageOutput<'_> {
        StageOutput::Masked {
            masked: self.clipped.as_deref(),
        }
    }

    fn metrics(&self) -> Option<StageMetrics> {
        let clipped = self.clipped.as_ref()?;
        let w = f64::from(self.dimensions.width);
        let h = f64::from(self.dimensions.height);
        let diagonal = w.hypot(h);
        let radius_px = diagonal * self.config.mask_diameter / 2.0;
        Some(StageMetrics::Mask {
            diameter: self.config.mask_diameter,
            radius_px,
            polylines_before: self.simplified.len(),
            polylines_after: clipped.len(),
            points_before: self.simplified.iter().map(Polyline::len).sum(),
            points_after: clipped.iter().map(Polyline::len).sum(),
        })
    }

    fn next(self) -> Result<Option<Stage>, PipelineError> {
        Ok(Some(Stage::Joined(self.join())))
    }

    fn complete(self) -> Result<StagedResult, PipelineError> {
        Ok(self.join().into_result())
    }
}

impl PipelineStage for Joined {
    const NAME: &str = "join";
    const INDEX: usize = 8;

    fn output(&self) -> StageOutput<'_> {
        StageOutput::Joined {
            joined: &self.path,
            dimensions: self.dimensions,
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn metrics(&self) -> Option<StageMetrics> {
        let join_input = self.masked.as_deref().unwrap_or(&self.simplified);
        let input_polyline_count = join_input.len();
        let input_point_count: usize = join_input.iter().map(Polyline::len).sum();
        let output_point_count = self.path.len();
        let expansion_ratio = if input_point_count > 0 {
            output_point_count as f64 / input_point_count as f64
        } else {
            0.0
        };
        Some(StageMetrics::Join {
            strategy: self.config.path_joiner.to_string(),
            input_polyline_count,
            input_point_count,
            output_point_count,
            expansion_ratio,
        })
    }

    fn next(self) -> Result<Option<Stage>, PipelineError> {
        Ok(None)
    }

    fn complete(self) -> Result<StagedResult, PipelineError> {
        Ok(self.into_result())
    }
}

/// Enum wrapping all pipeline stages for uniform, loopable access.
///
/// Use [`From`] conversions to enter the dynamic API from any typed
/// stage, then call [`advance`](Self::advance) in a loop:
///
/// ```rust
/// # use mujou_pipeline::{Pipeline, PipelineConfig, PipelineError};
/// # use mujou_pipeline::pipeline::{Stage, PipelineStage, Advance, STAGE_COUNT};
/// # fn run(png: Vec<u8>) -> Result<(), PipelineError> {
/// let mut stage: Stage = Pipeline::new(png, PipelineConfig::default()).into();
/// loop {
///     match stage.advance()? {
///         Advance::Next(next) => stage = next,
///         Advance::Complete(done) => { stage = done; break; }
///     }
/// }
/// let result = stage.complete()?;
/// # Ok(())
/// # }
/// ```
#[must_use]
pub enum Stage {
    /// See [`Pending`].
    Pending(Pending),
    /// See [`Decoded`].
    Decoded(Decoded),
    /// See [`Downsampled`].
    Downsampled(Downsampled),
    /// See [`Blurred`].
    Blurred(Blurred),
    /// See [`EdgesDetected`].
    EdgesDetected(EdgesDetected),
    /// See [`ContoursTraced`].
    ContoursTraced(ContoursTraced),
    /// See [`Simplified`].
    Simplified(Simplified),
    /// See [`Masked`].
    Masked(Masked),
    /// See [`Joined`].
    Joined(Joined),
}

/// Compile-time guard: if a [`Stage`] variant is added, this match becomes
/// non-exhaustive and the build fails — reminding you to bump [`STAGE_COUNT`].
#[allow(dead_code, clippy::match_same_arms)]
const fn _stage_count_guard(s: &Stage) {
    match s {
        Stage::Pending(_)
        | Stage::Decoded(_)
        | Stage::Downsampled(_)
        | Stage::Blurred(_)
        | Stage::EdgesDetected(_)
        | Stage::ContoursTraced(_)
        | Stage::Simplified(_)
        | Stage::Masked(_)
        | Stage::Joined(_) => {}
    }
}

/// Result of [`Stage::advance`]: either the next stage or the
/// completed final stage returned unchanged.
#[must_use]
pub enum Advance {
    /// The pipeline advanced to this next stage.
    Next(Stage),
    /// The pipeline was already at the final stage — returned unchanged.
    Complete(Stage),
}

/// Delegate a method call to whichever `Stage` variant is active.
macro_rules! delegate {
    ($self:ident, $method:ident $(, $arg:expr)*) => {
        match $self {
             Self::Pending(s) => s.$method($($arg),*),
             Self::Decoded(s) => s.$method($($arg),*),
             Self::Downsampled(s) => s.$method($($arg),*),
             Self::Blurred(s) => s.$method($($arg),*),
            Self::EdgesDetected(s) => s.$method($($arg),*),
            Self::ContoursTraced(s) => s.$method($($arg),*),
            Self::Simplified(s) => s.$method($($arg),*),
            Self::Masked(s) => s.$method($($arg),*),
            Self::Joined(s) => s.$method($($arg),*),
        }
    };
}

impl Stage {
    /// Human-readable name of the current stage.
    #[must_use]
    pub fn name(&self) -> &'static str {
        delegate!(self, name)
    }

    /// Zero-based index of the current stage.
    #[must_use]
    pub fn index(&self) -> usize {
        delegate!(self, index)
    }

    /// The output this stage produced.
    pub fn output(&self) -> StageOutput<'_> {
        delegate!(self, output)
    }

    /// Stage-specific metrics for diagnostics.
    ///
    /// Returns `None` for the initial `Pending` stage and for optional
    /// stages that were not executed (e.g. mask when disabled).
    #[must_use]
    pub fn metrics(&self) -> Option<StageMetrics> {
        delegate!(self, metrics)
    }

    /// Invert-specific metrics, if the edge detection stage performed
    /// inversion. Returns `None` for all other stages.
    #[must_use]
    pub fn invert_metrics(&self) -> Option<StageMetrics> {
        delegate!(self, invert_metrics)
    }

    /// Whether the pipeline is at the final stage.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        matches!(self, Self::Joined(_))
    }

    /// Advance to the next stage.
    ///
    /// Returns `Ok(Some(next_stage))` on success, `Ok(None)` if
    /// already complete (the `Joined` value is consumed), or `Err` if
    /// the transition fails.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`] if a fallible stage transition fails.
    pub fn next(self) -> Result<Option<Self>, PipelineError> {
        delegate!(self, next)
    }

    /// Advance to the next stage, returning `self` unchanged if
    /// already complete.
    ///
    /// This is the loop-friendly version of [`next`](Self::next).
    /// Unlike `next()`, which consumes the final stage and returns
    /// `Ok(None)`, `advance()` returns [`Advance::Complete`] with
    /// the final stage so you can still call
    /// [`complete`](Self::complete) on it.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`] if a fallible stage transition fails.
    ///
    /// ```rust
    /// # use mujou_pipeline::{Pipeline, PipelineConfig, PipelineError};
    /// # use mujou_pipeline::pipeline::{Stage, Advance};
    /// # fn run(png: Vec<u8>) -> Result<(), PipelineError> {
    /// let mut stage: Stage = Pipeline::new(png, PipelineConfig::default()).into();
    /// loop {
    ///     match stage.advance()? {
    ///         Advance::Next(next) => stage = next,
    ///         Advance::Complete(done) => { stage = done; break; }
    ///     }
    /// }
    /// let result = stage.complete()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn advance(self) -> Result<Advance, PipelineError> {
        if self.is_complete() {
            return Ok(Advance::Complete(self));
        }
        // Non-complete stages always return Ok(Some(_)) from next().
        // The is_complete() guard above ensures we never reach None here.
        #[allow(clippy::unreachable)]
        let next = self
            .next()?
            .unwrap_or_else(|| unreachable!("non-complete stage returned None from next()"));
        Ok(Advance::Next(next))
    }

    /// Run all remaining stages to completion.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`] if any remaining fallible stage fails.
    pub fn complete(self) -> Result<StagedResult, PipelineError> {
        delegate!(self, complete)
    }
}

// Provide a private helper trait so the macro can call `.name()` and
// `.index()` on `&self` — the `PipelineStage` trait's associated
// constants aren't callable via `self.NAME`.
trait StageMetadata {
    fn name(&self) -> &'static str;
    fn index(&self) -> usize;
}

impl<T: PipelineStage> StageMetadata for T {
    fn name(&self) -> &'static str {
        T::NAME
    }

    fn index(&self) -> usize {
        T::INDEX
    }
}

impl From<Pending> for Stage {
    fn from(s: Pending) -> Self {
        Self::Pending(s)
    }
}

impl From<Decoded> for Stage {
    fn from(s: Decoded) -> Self {
        Self::Decoded(s)
    }
}

impl From<Downsampled> for Stage {
    fn from(s: Downsampled) -> Self {
        Self::Downsampled(s)
    }
}

impl From<Blurred> for Stage {
    fn from(s: Blurred) -> Self {
        Self::Blurred(s)
    }
}

impl From<EdgesDetected> for Stage {
    fn from(s: EdgesDetected) -> Self {
        Self::EdgesDetected(s)
    }
}

impl From<ContoursTraced> for Stage {
    fn from(s: ContoursTraced) -> Self {
        Self::ContoursTraced(s)
    }
}

impl From<Simplified> for Stage {
    fn from(s: Simplified) -> Self {
        Self::Simplified(s)
    }
}

impl From<Masked> for Stage {
    fn from(s: Masked) -> Self {
        Self::Masked(s)
    }
}

impl From<Joined> for Stage {
    fn from(s: Joined) -> Self {
        Self::Joined(s)
    }
}

// ───────────────────── Pipeline entry point ──────────────────────────

/// Incremental image processing pipeline.
///
/// Created via [`Pipeline::new`], which stores the source image and
/// config without doing any processing. The caller then chains stage
/// methods to advance through the pipeline:
///
/// ```rust
/// # use mujou_pipeline::{Pipeline, PipelineConfig, PipelineError};
/// # fn run(png: Vec<u8>) -> Result<(), PipelineError> {
/// let result = Pipeline::new(png, PipelineConfig::default())
///     .decode()?
///     .downsample()
///     .blur()
///     .detect_edges()
///     .trace_contours()?
///     .simplify()
///     .mask()
///     .join()
///     .into_result();
/// # Ok(())
/// # }
/// ```
///
/// Each stage method consumes the current state and returns the next,
/// making it a compile-time error to skip stages or call them out of
/// order.
pub struct Pipeline;

impl Pipeline {
    /// Create a new pipeline from source image bytes and config.
    ///
    /// No processing is performed — the bytes and config are simply
    /// stored. Call [`.decode()`](Pending::decode) (or convert to a
    /// [`Stage`] and loop) to begin processing.
    #[allow(clippy::new_ret_no_self)]
    pub const fn new(image_bytes: Vec<u8>, config: PipelineConfig) -> Pending {
        Pending {
            config,
            source: image_bytes,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Create a minimal PNG with a sharp black/white boundary for testing.
    fn sharp_edge_png(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(width, height, |x, _y| {
            if x < width / 2 {
                image::Rgba([0, 0, 0, 255])
            } else {
                image::Rgba([255, 255, 255, 255])
            }
        });
        let mut buf = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut buf);
        image::ImageEncoder::write_image(
            encoder,
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
        buf
    }

    // ─────────── Typed API tests ─────────────────────────────────

    #[test]
    fn pending_exposes_source_bytes() {
        let png = sharp_edge_png(20, 20);
        let expected_len = png.len();
        let pending = Pipeline::new(png, PipelineConfig::default());
        assert_eq!(pending.source().len(), expected_len);
    }

    #[test]
    fn decode_empty_input_returns_error() {
        let result = Pipeline::new(vec![], PipelineConfig::default()).decode();
        assert!(matches!(result, Err(PipelineError::EmptyInput)));
    }

    #[test]
    fn decode_corrupt_input_returns_error() {
        let result = Pipeline::new(vec![0xFF, 0x00], PipelineConfig::default()).decode();
        assert!(matches!(result, Err(PipelineError::ImageDecode(_))));
    }

    #[test]
    fn decoded_exposes_original() {
        let png = sharp_edge_png(20, 20);
        let decoded = Pipeline::new(png, PipelineConfig::default())
            .decode()
            .unwrap();
        assert_eq!(decoded.original().width(), 20);
        assert_eq!(decoded.original().height(), 20);
    }

    #[test]
    fn downsampled_exposes_downsampled() {
        let png = sharp_edge_png(20, 20);
        let downsampled = Pipeline::new(png, PipelineConfig::default())
            .decode()
            .unwrap()
            .downsample();
        // 20x20 is below the default 256 working resolution, so no actual downsampling.
        assert!(!downsampled.applied());
        assert_eq!(downsampled.downsampled().width(), 20);
        assert_eq!(downsampled.downsampled().height(), 20);
    }

    #[test]
    fn blurred_exposes_blurred() {
        let png = sharp_edge_png(20, 20);
        let blurred = Pipeline::new(png, PipelineConfig::default())
            .decode()
            .unwrap()
            .downsample()
            .blur();
        assert_eq!(blurred.blurred().width(), 20);
        assert_eq!(blurred.blurred().height(), 20);
    }

    #[test]
    fn edges_detected_exposes_edges() {
        let png = sharp_edge_png(20, 20);
        let edges = Pipeline::new(png, PipelineConfig::default())
            .decode()
            .unwrap()
            .downsample()
            .blur()
            .detect_edges();
        assert_eq!(edges.edges().width(), 20);
        assert_eq!(edges.edges().height(), 20);
    }

    #[test]
    fn contours_traced_exposes_contours() {
        let png = sharp_edge_png(40, 40);
        let contours = Pipeline::new(png, PipelineConfig::default())
            .decode()
            .unwrap()
            .downsample()
            .blur()
            .detect_edges()
            .trace_contours()
            .unwrap();
        assert!(!contours.contours().is_empty());
    }

    #[test]
    fn trace_contours_returns_no_contours_for_uniform_image() {
        let img = image::RgbaImage::from_fn(20, 20, |_, _| image::Rgba([128, 128, 128, 255]));
        let mut buf = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut buf);
        image::ImageEncoder::write_image(
            encoder,
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();

        let result = Pipeline::new(buf, PipelineConfig::default())
            .decode()
            .unwrap()
            .downsample()
            .blur()
            .detect_edges()
            .trace_contours();
        assert!(matches!(result, Err(PipelineError::NoContours)));
    }

    #[test]
    fn simplified_exposes_simplified() {
        let png = sharp_edge_png(40, 40);
        let simplified = Pipeline::new(png, PipelineConfig::default())
            .decode()
            .unwrap()
            .downsample()
            .blur()
            .detect_edges()
            .trace_contours()
            .unwrap()
            .simplify();
        assert!(!simplified.simplified().is_empty());
    }

    #[test]
    fn masked_with_circular_mask_enabled() {
        let png = sharp_edge_png(40, 40);
        let config = PipelineConfig {
            circular_mask: true,
            mask_diameter: 0.8,
            ..PipelineConfig::default()
        };
        let masked = Pipeline::new(png, config)
            .decode()
            .unwrap()
            .downsample()
            .blur()
            .detect_edges()
            .trace_contours()
            .unwrap()
            .simplify()
            .mask();
        assert!(masked.masked().is_some());
    }

    #[test]
    fn masked_with_circular_mask_disabled() {
        let png = sharp_edge_png(40, 40);
        let config = PipelineConfig {
            circular_mask: false,
            ..PipelineConfig::default()
        };
        let masked = Pipeline::new(png, config)
            .decode()
            .unwrap()
            .downsample()
            .blur()
            .detect_edges()
            .trace_contours()
            .unwrap()
            .simplify()
            .mask();
        assert!(masked.masked().is_none());
    }

    #[test]
    fn joined_exposes_joined() {
        let png = sharp_edge_png(40, 40);
        let joined = Pipeline::new(png, PipelineConfig::default())
            .decode()
            .unwrap()
            .downsample()
            .blur()
            .detect_edges()
            .trace_contours()
            .unwrap()
            .simplify()
            .mask()
            .join();
        assert!(!joined.joined().is_empty());
    }

    #[test]
    fn full_pipeline_produces_same_result_as_process_staged() {
        let png = sharp_edge_png(40, 40);
        let config = PipelineConfig::default();

        let staged = crate::process_staged(&png, &config).unwrap();
        let pipeline_result = Pipeline::new(png, config)
            .decode()
            .unwrap()
            .downsample()
            .blur()
            .detect_edges()
            .trace_contours()
            .unwrap()
            .simplify()
            .mask()
            .join()
            .into_result();

        assert_eq!(staged.original, pipeline_result.original);
        assert_eq!(staged.downsampled, pipeline_result.downsampled);
        assert_eq!(staged.blurred, pipeline_result.blurred);
        assert_eq!(staged.edges, pipeline_result.edges);
        assert_eq!(staged.contours, pipeline_result.contours);
        assert_eq!(staged.simplified, pipeline_result.simplified);
        assert_eq!(staged.masked, pipeline_result.masked);
        assert_eq!(staged.joined, pipeline_result.joined);
        assert_eq!(staged.dimensions, pipeline_result.dimensions);
    }

    #[test]
    fn full_pipeline_with_invert() {
        let png = sharp_edge_png(40, 40);
        let config = PipelineConfig {
            invert: true,
            ..PipelineConfig::default()
        };

        let staged = crate::process_staged(&png, &config).unwrap();
        let pipeline_result = Pipeline::new(png, config)
            .decode()
            .unwrap()
            .downsample()
            .blur()
            .detect_edges()
            .trace_contours()
            .unwrap()
            .simplify()
            .mask()
            .join()
            .into_result();

        assert_eq!(staged.edges, pipeline_result.edges);
        assert_eq!(staged.joined, pipeline_result.joined);
    }

    #[test]
    fn full_pipeline_with_mask() {
        let png = sharp_edge_png(40, 40);
        let config = PipelineConfig {
            circular_mask: true,
            mask_diameter: 0.8,
            ..PipelineConfig::default()
        };

        let staged = crate::process_staged(&png, &config).unwrap();
        let pipeline_result = Pipeline::new(png, config)
            .decode()
            .unwrap()
            .downsample()
            .blur()
            .detect_edges()
            .trace_contours()
            .unwrap()
            .simplify()
            .mask()
            .join()
            .into_result();

        assert_eq!(staged.masked, pipeline_result.masked);
        assert_eq!(staged.joined, pipeline_result.joined);
    }

    #[test]
    fn joined_dimensions_accessor() {
        let png = sharp_edge_png(40, 40);
        let joined = Pipeline::new(png, PipelineConfig::default())
            .decode()
            .unwrap()
            .downsample()
            .blur()
            .detect_edges()
            .trace_contours()
            .unwrap()
            .simplify()
            .mask()
            .join();
        assert_eq!(
            joined.dimensions(),
            Dimensions {
                width: 40,
                height: 40,
            },
        );
    }

    // ─────────── Helper: drive a Stage to completion ────────────

    /// Advance a [`Stage`] to completion, returning the final stage
    /// and a log of `(index, name)` pairs visited along the way.
    #[allow(clippy::type_complexity)]
    fn drive_to_end(start: Stage) -> Result<(Stage, Vec<(usize, &'static str)>), PipelineError> {
        let mut log = vec![(start.index(), start.name())];
        let mut stage = start;
        loop {
            match stage.advance()? {
                Advance::Next(next) => {
                    log.push((next.index(), next.name()));
                    stage = next;
                }
                Advance::Complete(done) => return Ok((done, log)),
            }
        }
    }

    // ─────────── PipelineStage trait + Stage enum tests ───────────

    #[test]
    fn stage_names_and_indices() {
        let png = sharp_edge_png(40, 40);
        let start: Stage = Pipeline::new(png, PipelineConfig::default()).into();

        let (_, log) = drive_to_end(start).unwrap();
        let expected = [
            (0, "source"),
            (1, "decode"),
            (2, "downsample"),
            (3, "blur"),
            (4, "edges"),
            (5, "contours"),
            (6, "simplify"),
            (7, "mask"),
            (8, "join"),
        ];
        assert_eq!(log.as_slice(), &expected);
    }

    #[test]
    fn loop_to_completion_matches_chained_api() {
        let png = sharp_edge_png(40, 40);
        let config = PipelineConfig::default();

        // Chained API.
        let chained = Pipeline::new(png.clone(), config.clone())
            .decode()
            .unwrap()
            .downsample()
            .blur()
            .detect_edges()
            .trace_contours()
            .unwrap()
            .simplify()
            .mask()
            .join()
            .into_result();

        // Loop API.
        let start: Stage = Pipeline::new(png, config).into();
        let (final_stage, _) = drive_to_end(start).unwrap();
        let looped = final_stage.complete().unwrap();

        assert_eq!(chained.original, looped.original);
        assert_eq!(chained.downsampled, looped.downsampled);
        assert_eq!(chained.blurred, looped.blurred);
        assert_eq!(chained.edges, looped.edges);
        assert_eq!(chained.contours, looped.contours);
        assert_eq!(chained.simplified, looped.simplified);
        assert_eq!(chained.masked, looped.masked);
        assert_eq!(chained.joined, looped.joined);
        assert_eq!(chained.dimensions, looped.dimensions);
    }

    #[test]
    fn complete_from_pending() {
        let png = sharp_edge_png(40, 40);
        let pending = Pipeline::new(png, PipelineConfig::default());
        let result = pending.complete().unwrap();
        assert!(!result.joined.is_empty());
    }

    #[test]
    fn complete_from_decoded() {
        let png = sharp_edge_png(40, 40);
        let decoded = Pipeline::new(png, PipelineConfig::default())
            .decode()
            .unwrap();
        let result = decoded.complete().unwrap();
        assert!(!result.joined.is_empty());
    }

    #[test]
    fn complete_from_mid_stage() {
        let png = sharp_edge_png(40, 40);
        let blurred = Pipeline::new(png, PipelineConfig::default())
            .decode()
            .unwrap()
            .downsample()
            .blur();
        let result = blurred.complete().unwrap();
        assert!(!result.joined.is_empty());
    }

    #[test]
    fn complete_from_joined_is_into_result() {
        let png = sharp_edge_png(40, 40);
        let joined = Pipeline::new(png, PipelineConfig::default())
            .decode()
            .unwrap()
            .downsample()
            .blur()
            .detect_edges()
            .trace_contours()
            .unwrap()
            .simplify()
            .mask()
            .join();
        let result = joined.complete().unwrap();
        assert!(!result.joined.is_empty());
    }

    #[test]
    fn next_on_joined_returns_none() {
        let png = sharp_edge_png(40, 40);
        let joined = Pipeline::new(png, PipelineConfig::default())
            .decode()
            .unwrap()
            .downsample()
            .blur()
            .detect_edges()
            .trace_contours()
            .unwrap()
            .simplify()
            .mask()
            .join();
        assert!(joined.next().unwrap().is_none());
    }

    #[test]
    fn stage_is_complete() {
        let png = sharp_edge_png(40, 40);
        let start: Stage = Pipeline::new(png, PipelineConfig::default()).into();
        assert!(!start.is_complete());

        let (final_stage, _) = drive_to_end(start).unwrap();
        assert!(final_stage.is_complete());
    }

    #[test]
    fn output_variant_matches_stage() {
        let png = sharp_edge_png(40, 40);
        let start: Stage = Pipeline::new(png, PipelineConfig::default()).into();

        let mut stage = start;
        let mut visited = 0;
        loop {
            let idx = stage.index();
            let variant_idx = match stage.output() {
                StageOutput::Source { .. } => 0,
                StageOutput::Decoded { .. } => 1,
                StageOutput::Downsampled { .. } => 2,
                StageOutput::Blurred { .. } => 3,
                StageOutput::EdgesDetected { .. } => 4,
                StageOutput::ContoursTraced { .. } => 5,
                StageOutput::Simplified { .. } => 6,
                StageOutput::Masked { .. } => 7,
                StageOutput::Joined { .. } => 8,
            };
            assert_eq!(idx, variant_idx, "output variant mismatch at index {idx}");
            visited += 1;
            match stage.advance().unwrap() {
                Advance::Next(next) => stage = next,
                Advance::Complete(_) => break,
            }
        }
        assert_eq!(visited, STAGE_COUNT);
    }

    #[test]
    fn from_conversions_preserve_index() {
        let png = sharp_edge_png(40, 40);
        let pending = Pipeline::new(png.clone(), PipelineConfig::default());
        let stage: Stage = pending.into();
        assert_eq!(stage.index(), 0);

        let decoded = Pipeline::new(png.clone(), PipelineConfig::default())
            .decode()
            .unwrap();
        let stage: Stage = decoded.into();
        assert_eq!(stage.index(), 1);

        let downsampled = Pipeline::new(png.clone(), PipelineConfig::default())
            .decode()
            .unwrap()
            .downsample();
        let stage: Stage = downsampled.into();
        assert_eq!(stage.index(), 2);

        let blurred = Pipeline::new(png, PipelineConfig::default())
            .decode()
            .unwrap()
            .downsample()
            .blur();
        let stage: Stage = blurred.into();
        assert_eq!(stage.index(), 3);
    }

    #[test]
    fn stage_complete_from_enum() {
        let png = sharp_edge_png(40, 40);
        let stage: Stage = Pipeline::new(png, PipelineConfig::default()).into();
        let result = stage.complete().unwrap();
        assert!(!result.joined.is_empty());
    }

    #[test]
    fn loop_with_progress_tracking() {
        let png = sharp_edge_png(40, 40);
        let start: Stage = Pipeline::new(png, PipelineConfig::default()).into();

        let (_, log) = drive_to_end(start).unwrap();
        let indices: Vec<usize> = log.iter().map(|(idx, _)| *idx).collect();
        assert_eq!(indices, vec![0, 1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn pending_decode_error_via_advance() {
        let stage: Stage = Pipeline::new(vec![], PipelineConfig::default()).into();
        let result = stage.advance();
        assert!(matches!(result, Err(PipelineError::EmptyInput)));
    }
}
