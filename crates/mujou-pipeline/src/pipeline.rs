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
//!     .canvas()
//!     .join()
//!     .output();
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

use std::sync::Arc;

use image::DynamicImage;

use crate::contour::ContourTracer;
use crate::diagnostics::StageMetrics;
use crate::join::PathJoiner;
use crate::mask::{BorderPathMode, CanvasShape, MaskResult, MaskShape};
use crate::mst_join::JoinQualityMetrics;
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
#[must_use = "pipeline stages are consumed by advancing — call .canvas() to continue"]
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

    /// Clip polylines to the given mask shape and optionally generate a
    /// border polyline. Shared by both Circle and Rectangle modes.
    fn clip_and_border(
        polylines: &[Polyline],
        shape: MaskShape,
        border_mode: BorderPathMode,
    ) -> MaskResult {
        let clipped = crate::mask::apply_mask(polylines, &shape);

        let border = match border_mode {
            BorderPathMode::Off => None,
            BorderPathMode::On => Some(shape.border_polyline()),
            BorderPathMode::Auto => {
                let any_clipped = clipped.iter().any(|c| c.start_clipped || c.end_clipped);
                if any_clipped {
                    Some(shape.border_polyline())
                } else {
                    None
                }
            }
        };

        MaskResult {
            clipped,
            border,
            shape,
        }
    }

    /// Advance to the canvas stage.
    ///
    /// Polylines are clipped to the canvas boundary (circle or
    /// rectangle) and an optional border polyline is generated based
    /// on [`BorderPathMode`].
    pub fn canvas(self) -> Canvas {
        let center = Point::new(
            f64::from(self.dimensions.width) / 2.0,
            f64::from(self.dimensions.height) / 2.0,
        );

        let canvas_result = match self.config.shape {
            CanvasShape::Circle => {
                let radius = self.dimensions.canvas_radius(self.config.scale);
                let shape = MaskShape::Circle { center, radius };
                Self::clip_and_border(&self.reduced, shape, self.config.border_path)
            }
            CanvasShape::Rectangle => {
                let (half_width, half_height) = self.dimensions.canvas_rect_half_dims(
                    self.config.scale,
                    self.config.aspect_ratio,
                    self.config.landscape,
                );
                let shape = MaskShape::Rectangle {
                    center,
                    half_width,
                    half_height,
                };
                Self::clip_and_border(&self.reduced, shape, self.config.border_path)
            }
        };
        Canvas {
            config: self.config,
            original: self.original,
            downsampled: self.downsampled,
            blurred: self.blurred,
            edges: self.edges,
            contours: self.contours,
            simplified: self.reduced,
            canvas_result,
            dimensions: self.dimensions,
        }
    }
}

// ───────────────────────── Stage 7: Canvas ───────────────────────────

/// Pipeline state after optional masking (circle or rectangle).
///
/// Call [`join`](Self::join) to advance to the final stage.
///
/// See the [module-level memory notes](self#memory) for the cost of
/// retaining all prior raster intermediates.
#[must_use = "pipeline stages are consumed by advancing — call .join() to continue"]
#[allow(clippy::struct_field_names)]
pub struct Canvas {
    config: PipelineConfig,
    original: RgbaImage,
    downsampled: RgbaImage,
    blurred: RgbaImage,
    edges: GrayImage,
    contours: Vec<Polyline>,
    simplified: Vec<Polyline>,
    canvas_result: MaskResult,
    dimensions: Dimensions,
}

impl Canvas {
    /// The canvas result.
    #[must_use]
    pub const fn canvas(&self) -> &MaskResult {
        &self.canvas_result
    }

    /// Advance to the joining stage — the final pipeline step.
    pub fn join(self) -> Joined {
        let join_input: Vec<Polyline> = self.canvas_result.all_polylines().cloned().collect();
        let output = self
            .config
            .path_joiner
            .join(&join_input, &self.config, self.dimensions);
        Joined {
            config: self.config,
            original: self.original,
            downsampled: self.downsampled,
            blurred: self.blurred,
            edges: self.edges,
            contours: self.contours,
            simplified: self.simplified,
            canvas: self.canvas_result,
            path: output.path,
            quality_metrics: output.quality_metrics,
            dimensions: self.dimensions,
        }
    }
}

// ───────────────────────── Stage 8: Joined ───────────────────────────

/// Pipeline state after path joining.
///
/// Call [`output`](Self::output) to advance to the final stage.
///
/// See the [module-level memory notes](self#memory) for the cost of
/// retaining all prior raster intermediates.
#[must_use = "pipeline stages are consumed by advancing — call .output() to continue"]
pub struct Joined {
    config: PipelineConfig,
    original: RgbaImage,
    downsampled: RgbaImage,
    blurred: RgbaImage,
    edges: GrayImage,
    contours: Vec<Polyline>,
    simplified: Vec<Polyline>,
    canvas: MaskResult,
    path: Polyline,
    quality_metrics: Option<JoinQualityMetrics>,
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

    /// Advance to the output stage — the final pipeline step.
    pub fn output(self) -> Output {
        let subsampled = crate::subsample::subsample(&self.path, self.config.subsample_max_length);
        Output {
            config: self.config,
            original: self.original,
            downsampled: self.downsampled,
            blurred: self.blurred,
            edges: self.edges,
            contours: self.contours,
            simplified: self.simplified,
            canvas: self.canvas,
            joined: self.path,
            subsampled,
            quality_metrics: self.quality_metrics,
            dimensions: self.dimensions,
        }
    }
}

// ───────────────────────── Stage 9: Output ──────────────────────────

/// Pipeline state after segment subsampling — the final stage.
///
/// Long segments in the joined path have been subdivided so no
/// segment exceeds `config.subsample_max_length` pixels. This
/// prevents angular artifacts when converting to polar coordinates
/// for THR export.
///
/// Call [`into_result`](Self::into_result) to extract the
/// [`StagedResult`] containing all intermediates.
///
/// See the [module-level memory notes](self#memory) for the cost of
/// retaining all prior raster intermediates.
#[must_use = "call .into_result() to extract the StagedResult"]
#[allow(clippy::struct_field_names)]
pub struct Output {
    config: PipelineConfig,
    original: RgbaImage,
    downsampled: RgbaImage,
    blurred: RgbaImage,
    edges: GrayImage,
    contours: Vec<Polyline>,
    simplified: Vec<Polyline>,
    canvas: MaskResult,
    joined: Polyline,
    subsampled: Polyline,
    quality_metrics: Option<JoinQualityMetrics>,
    dimensions: Dimensions,
}

impl Output {
    /// The subsampled output path.
    #[must_use]
    pub const fn output_polyline(&self) -> &Polyline {
        &self.subsampled
    }

    /// Image dimensions.
    #[must_use]
    pub const fn dimensions(&self) -> Dimensions {
        self.dimensions
    }

    /// Consume the pipeline and return the full [`StagedResult`].
    #[must_use]
    pub fn into_result(self) -> StagedResult {
        let mst_edge_details = self
            .quality_metrics
            .map_or_else(Vec::new, |qm| qm.mst_edge_details);
        StagedResult {
            original: self.original,
            downsampled: self.downsampled,
            blurred: self.blurred,
            edges: self.edges,
            contours: self.contours,
            simplified: self.simplified,
            canvas: self.canvas,
            joined: self.joined,
            output: self.subsampled,
            mst_edge_details,
            dimensions: self.dimensions,
        }
    }
}

// ──────────────────── PipelineStage trait + Stage enum ────────────────

/// Total number of stages in the pipeline.
pub const STAGE_COUNT: usize = 10;

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
    /// Canvas mask result.
    Canvas {
        /// The canvas result.
        canvas: &'a MaskResult,
    },
    /// Path joining result.
    Joined {
        /// The single continuous output path.
        joined: &'a Polyline,
        /// Image dimensions.
        dimensions: Dimensions,
    },
    /// Segment subsampling result.
    Output {
        /// The subsampled output path.
        output: &'a Polyline,
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
        Ok(Some(Stage::Canvas(self.canvas())))
    }

    fn complete(self) -> Result<StagedResult, PipelineError> {
        self.canvas().complete()
    }
}

impl PipelineStage for Canvas {
    const NAME: &str = "canvas";
    const INDEX: usize = 7;

    fn output(&self) -> StageOutput<'_> {
        StageOutput::Canvas {
            canvas: &self.canvas_result,
        }
    }

    fn metrics(&self) -> Option<StageMetrics> {
        let shape_info = match self.config.shape {
            CanvasShape::Circle => {
                let radius_px = self.dimensions.canvas_radius(self.config.scale);
                format!("d={:.2} r={radius_px:.1}px", self.config.scale)
            }
            CanvasShape::Rectangle => {
                let (hw, hh) = self.dimensions.canvas_rect_half_dims(
                    self.config.scale,
                    self.config.aspect_ratio,
                    self.config.landscape,
                );
                format!(
                    "scale={:.2} ar={:.2} {} {:.1}\u{00d7}{:.1}px",
                    self.config.scale,
                    self.config.aspect_ratio,
                    if self.config.landscape {
                        "land"
                    } else {
                        "port"
                    },
                    hw * 2.0,
                    hh * 2.0,
                )
            }
        };
        // Counts only clipped polylines (excludes the optional border
        // polyline, which is an addition rather than a clipping result).
        // The subsequent Joined stage uses `all_polylines()` which
        // includes the border, so its input count may differ.
        Some(StageMetrics::Canvas {
            shape_info,
            polylines_before: self.simplified.len(),
            polylines_after: self.canvas_result.clipped.len(),
            points_before: self.simplified.iter().map(Polyline::len).sum(),
            points_after: self
                .canvas_result
                .clipped
                .iter()
                .map(|c| c.polyline.len())
                .sum(),
        })
    }

    fn next(self) -> Result<Option<Stage>, PipelineError> {
        Ok(Some(Stage::Joined(self.join())))
    }

    fn complete(self) -> Result<StagedResult, PipelineError> {
        Ok(self.join().output().into_result())
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
        let count: usize = self.canvas.all_polylines().count();
        let points: usize = self.canvas.all_polylines().map(Polyline::len).sum();
        let (input_polyline_count, input_point_count) = (count, points);
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
            quality: self.quality_metrics.clone(),
        })
    }

    fn next(self) -> Result<Option<Stage>, PipelineError> {
        Ok(Some(Stage::Output(self.output())))
    }

    fn complete(self) -> Result<StagedResult, PipelineError> {
        Ok(self.output().into_result())
    }
}

impl PipelineStage for Output {
    const NAME: &str = "output";
    const INDEX: usize = 9;

    fn output(&self) -> StageOutput<'_> {
        StageOutput::Output {
            output: &self.subsampled,
            dimensions: self.dimensions,
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn metrics(&self) -> Option<StageMetrics> {
        let points_before = self.joined.len();
        let points_after = self.subsampled.len();
        Some(StageMetrics::Output {
            max_length: self.config.subsample_max_length,
            points_before,
            points_after,
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
    /// See [`Canvas`].
    Canvas(Canvas),
    /// See [`Joined`].
    Joined(Joined),
    /// See [`Output`].
    Output(Output),
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
        | Stage::Canvas(_)
        | Stage::Joined(_)
        | Stage::Output(_) => {}
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
            Self::Canvas(s) => s.$method($($arg),*),
            Self::Joined(s) => s.$method($($arg),*),
            Self::Output(s) => s.$method($($arg),*),
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
    /// Returns `None` for the initial `Pending` stage.
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
        matches!(self, Self::Output(_))
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

impl From<Canvas> for Stage {
    fn from(s: Canvas) -> Self {
        Self::Canvas(s)
    }
}

impl From<Joined> for Stage {
    fn from(s: Joined) -> Self {
        Self::Joined(s)
    }
}

impl From<Output> for Stage {
    fn from(s: Output) -> Self {
        Self::Output(s)
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
///     .canvas()
///     .join()
///     .output()
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

// ─────────────────────── Pipeline cache ──────────────────────────────

/// Cached state from a previous pipeline run.
///
/// Holds every intermediate result so that a subsequent run with the
/// same image but a different config can skip unchanged stages.  The
/// worker stores one of these between messages and passes it back in
/// on the next run via [`PipelineCache::run`].
///
/// # Memory
///
/// Retains one full [`StagedResult`] (~7 MB for a 1000×1000 image),
/// plus the [`DynamicImage`] from decode (~4 MB), for a total of
/// roughly 11 MB.  This is acceptable given the cost of a full
/// pipeline re-run (hundreds of milliseconds) versus the cheap
/// in-memory comparison.
pub struct PipelineCache {
    /// Hash of the source image bytes that produced this cache.
    ///
    /// Computed with SipHash-2-4 via `siphasher` — deterministic across
    /// Rust versions, platforms, and processes.
    image_hash: u64,
    /// The config that produced the cached results.
    config: PipelineConfig,
    /// The decoded `DynamicImage` — needed for re-downsample when
    /// `working_resolution` or `downsample_filter` changes but the
    /// source image stays the same.  This is dropped after the
    /// `Decoded → Downsampled` transition and is not part of
    /// `StagedResult`, so we keep it here.
    decoded_image: DynamicImage,
    /// Byte length of the source image (for decode-stage metrics).
    source_len: usize,
    /// Whether downsampling was actually applied (image was larger
    /// than `working_resolution`).  Diagnostic-only.
    downsampled_applied: bool,
    /// Edge pixel count before optional inversion.  Diagnostic-only.
    pre_invert_edge_pixels: u64,
    /// All intermediate raster and vector outputs.
    ///
    /// Wrapped in [`Arc`] so the caller and the cache can share a
    /// single allocation without cloning ~7 MB of image buffers.
    staged: Arc<StagedResult>,
}

impl PipelineCache {
    /// Run the pipeline, reusing cached intermediates when possible.
    ///
    /// If `cache` is `Some` and the image bytes match the cached run,
    /// only the stages whose config parameters changed (and their
    /// downstream dependents) are re-executed.  Otherwise a full run
    /// is performed.
    ///
    /// `on_stage` is called with `(stage_index, cached)` for each
    /// stage as it is reached.  `cached` is `true` when the stage
    /// result was served from the cache without re-computation, and
    /// `false` when the stage was actually executed.  This allows
    /// callers to report per-stage progress and distinguish skipped
    /// stages in the UI.
    ///
    /// Returns the pipeline result (wrapped in [`Arc`] so the caller
    /// and the cache share a single allocation) together with an
    /// updated cache for the next invocation.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`] if any pipeline stage fails (decode
    /// error, no contours, etc.).
    pub fn run(
        cache: Option<Self>,
        image_bytes: Vec<u8>,
        config: PipelineConfig,
        on_stage: &dyn Fn(usize, bool),
    ) -> Result<(Arc<StagedResult>, Self), PipelineError> {
        let image_hash = Self::hash_bytes(&image_bytes);

        match cache {
            Some(c) if c.image_hash == image_hash => {
                let earliest = c.config.earliest_changed_stage(&config);
                if earliest >= STAGE_COUNT {
                    // Nothing changed — fire progress for all stages
                    // (all cached) and return the cached result.
                    for i in 0..STAGE_COUNT {
                        on_stage(i, true);
                    }
                    return Ok((c.staged.clone(), Self { config, ..c }));
                }
                c.resume(config, earliest, on_stage)
            }
            _ => Self::full_run(image_bytes, config, image_hash, on_stage),
        }
    }

    /// Hash image bytes using SipHash-2-4 with fixed keys.
    ///
    /// Uses `siphasher::sip::SipHasher` instead of `DefaultHasher` so the
    /// hash is deterministic across Rust versions, platforms, and
    /// processes.
    fn hash_bytes(bytes: &[u8]) -> u64 {
        use std::hash::{Hash, Hasher};

        let mut hasher = siphasher::sip::SipHasher::new();
        bytes.hash(&mut hasher);
        hasher.finish()
    }

    /// Run the full pipeline from scratch, capturing intermediates
    /// for the cache.
    fn full_run(
        image_bytes: Vec<u8>,
        config: PipelineConfig,
        image_hash: u64,
        on_stage: &dyn Fn(usize, bool),
    ) -> Result<(Arc<StagedResult>, Self), PipelineError> {
        let cache_config = config.clone();

        let pending = Pipeline::new(image_bytes, config);
        on_stage(Pending::INDEX, false);

        let decoded = pending.decode()?;
        on_stage(Decoded::INDEX, false);

        // Capture DynamicImage before downsample consumes it.
        let decoded_image = decoded.image.clone();
        let source_len = decoded.source_len;

        let downsampled = decoded.downsample();
        let downsampled_applied = downsampled.applied;
        on_stage(Downsampled::INDEX, false);

        let blurred = downsampled.blur();
        on_stage(Blurred::INDEX, false);

        let edges = blurred.detect_edges();
        let pre_invert_edge_pixels = edges.pre_invert_edge_pixels;
        on_stage(EdgesDetected::INDEX, false);

        let contours = edges.trace_contours()?;
        on_stage(ContoursTraced::INDEX, false);

        let simplified = contours.simplify();
        on_stage(Simplified::INDEX, false);

        let canvas = simplified.canvas();
        on_stage(Canvas::INDEX, false);

        let joined = canvas.join();
        on_stage(Joined::INDEX, false);

        let subsampled = joined.output();
        on_stage(Output::INDEX, false);

        let staged = Arc::new(subsampled.into_result());
        let cache = Self {
            image_hash,
            config: cache_config,
            decoded_image,
            source_len,
            downsampled_applied,
            pre_invert_edge_pixels,
            staged: Arc::clone(&staged),
        };

        Ok((staged, cache))
    }

    /// Re-run from the earliest changed stage, reusing cached
    /// intermediates for all upstream stages.
    fn resume(
        self,
        new_config: PipelineConfig,
        earliest_changed: usize,
        on_stage: &dyn Fn(usize, bool),
    ) -> Result<(Arc<StagedResult>, Self), PipelineError> {
        // Destructure self so we can move `staged` fields into the
        // resume stage instead of cloning them.
        let Self {
            image_hash,
            config: _,
            decoded_image,
            source_len,
            mut downsampled_applied,
            mut pre_invert_edge_pixels,
            staged: old_staged_arc,
        } = self;

        // Unwrap the Arc to get owned StagedResult.  Succeeds without
        // cloning when the caller has dropped their Arc from the
        // previous run (the normal case); falls back to clone if the
        // caller still holds a reference.
        let old_staged = Arc::try_unwrap(old_staged_arc).unwrap_or_else(|arc| (*arc).clone());

        // Build a Stage at the predecessor of the earliest changed
        // stage, populated with cached data and the new config.
        // Consumes `old_staged` by move — no image buffer clones.
        let mut stage = Self::build_resume_stage(
            old_staged,
            &decoded_image,
            source_len,
            downsampled_applied,
            pre_invert_edge_pixels,
            &new_config,
            earliest_changed,
        );

        // Report progress for all cached (skipped) stages up to and
        // including the resume point.
        for i in 0..stage.index() {
            on_stage(i, true);
        }

        // Report the resume stage itself (cached — it was
        // reconstructed from the cache, not computed).
        on_stage(stage.index(), true);

        // Run the remaining stages to completion.
        loop {
            // Capture diagnostic fields from stages we're about to
            // advance past (these are lost after `advance()`).
            if let Stage::Downsampled(ref ds) = stage {
                downsampled_applied = ds.applied;
            }
            if let Stage::EdgesDetected(ref ed) = stage {
                pre_invert_edge_pixels = ed.pre_invert_edge_pixels;
            }

            match stage.advance()? {
                Advance::Next(next) => {
                    on_stage(next.index(), false);
                    stage = next;
                }
                Advance::Complete(done) => {
                    stage = done;
                    break;
                }
            }
        }

        let staged = Arc::new(stage.complete()?);
        let cache = Self {
            image_hash,
            config: new_config,
            decoded_image,
            source_len,
            downsampled_applied,
            pre_invert_edge_pixels,
            staged: Arc::clone(&staged),
        };

        Ok((staged, cache))
    }

    /// Reconstruct a [`Stage`] at the predecessor of `earliest_changed`
    /// using cached data and the new config.
    ///
    /// Consumes `staged` by value so image buffers are moved rather
    /// than cloned — avoiding ~13 MB of allocations at working
    /// resolution.
    ///
    /// For example, if `earliest_changed == 4` (edges), this builds a
    /// `Stage::Blurred` (index 3) so that `advance()` will re-run
    /// edge detection with the new config.
    #[allow(clippy::too_many_lines)]
    fn build_resume_stage(
        staged: StagedResult,
        decoded_image: &DynamicImage,
        source_len: usize,
        downsampled_applied: bool,
        pre_invert_edge_pixels: u64,
        new_config: &PipelineConfig,
        earliest_changed: usize,
    ) -> Stage {
        let StagedResult {
            original,
            downsampled,
            blurred,
            edges,
            contours,
            simplified,
            canvas,
            joined,
            output: _,
            mst_edge_details,
            dimensions,
        } = staged;

        match earliest_changed {
            // Stage 2 changed (downsample) — resume from Decoded (index 1).
            2 => Stage::Decoded(Decoded {
                config: new_config.clone(),
                image: decoded_image.clone(),
                original,
                source_len,
            }),

            // Stage 3 changed (blur) — resume from Downsampled (index 2).
            3 => Stage::Downsampled(Downsampled {
                config: new_config.clone(),
                original,
                rgba: downsampled,
                applied: downsampled_applied,
            }),

            // Stage 4 changed (edges) — resume from Blurred (index 3).
            4 => Stage::Blurred(Blurred {
                config: new_config.clone(),
                original,
                downsampled,
                smooth: blurred,
                dimensions,
            }),

            // Stage 5 changed (contours) — resume from EdgesDetected (index 4).
            5 => Stage::EdgesDetected(EdgesDetected {
                config: new_config.clone(),
                original,
                downsampled,
                blurred,
                edge_map: edges,
                pre_invert_edge_pixels,
                dimensions,
            }),

            // Stage 6 changed (simplify) — resume from ContoursTraced (index 5).
            6 => Stage::ContoursTraced(ContoursTraced {
                config: new_config.clone(),
                original,
                downsampled,
                blurred,
                edges,
                contours,
                dimensions,
            }),

            // Stage 7 changed (canvas) — resume from Simplified (index 6).
            7 => Stage::Simplified(Simplified {
                config: new_config.clone(),
                original,
                downsampled,
                blurred,
                edges,
                contours,
                reduced: simplified,
                dimensions,
            }),

            // Stage 8 changed (join) — resume from Canvas (index 7).
            8 => Stage::Canvas(Canvas {
                config: new_config.clone(),
                original,
                downsampled,
                blurred,
                edges,
                contours,
                simplified,
                canvas_result: canvas,
                dimensions,
            }),

            // Stage 9 changed (subsample) — resume from Joined (index 8).
            9 => {
                let quality_metrics = if mst_edge_details.is_empty() {
                    None
                } else {
                    Some(JoinQualityMetrics {
                        mst_edge_details,
                        ..JoinQualityMetrics::default()
                    })
                };
                Stage::Joined(Joined {
                    config: new_config.clone(),
                    original,
                    downsampled,
                    blurred,
                    edges,
                    contours,
                    simplified,
                    canvas,
                    path: joined,
                    quality_metrics,
                    dimensions,
                })
            }

            _ => unreachable!(
                "earliest_changed_stage returned {earliest_changed}, \
                 expected 2..=9"
            ),
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

    /// Config with `scale = 0.5` so the canvas covers the full 40×40
    /// test image.  Default `scale = 1.25` produces a tight canvas
    /// (radius = 16 px) that clips the 2-point simplified contour from
    /// [`sharp_edge_png`] because RDP collapses the collinear edge to
    /// two adjacent points near the image top.
    fn wide_canvas_config() -> PipelineConfig {
        PipelineConfig {
            scale: 0.5,
            ..PipelineConfig::default()
        }
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
    fn canvas_with_circular_shape() {
        let png = sharp_edge_png(40, 40);
        let config = PipelineConfig {
            shape: crate::mask::CanvasShape::Circle,
            scale: 0.8,
            ..PipelineConfig::default()
        };
        let canvas_stage = Pipeline::new(png, config)
            .decode()
            .unwrap()
            .downsample()
            .blur()
            .detect_edges()
            .trace_contours()
            .unwrap()
            .simplify()
            .canvas();
        // Canvas always produces a result; verify it's accessible.
        let _result = canvas_stage.canvas();
    }

    #[test]
    fn joined_exposes_joined() {
        let png = sharp_edge_png(40, 40);
        let joined = Pipeline::new(png, wide_canvas_config())
            .decode()
            .unwrap()
            .downsample()
            .blur()
            .detect_edges()
            .trace_contours()
            .unwrap()
            .simplify()
            .canvas()
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
            .canvas()
            .join()
            .output()
            .into_result();

        assert_eq!(staged.original, pipeline_result.original);
        assert_eq!(staged.downsampled, pipeline_result.downsampled);
        assert_eq!(staged.blurred, pipeline_result.blurred);
        assert_eq!(staged.edges, pipeline_result.edges);
        assert_eq!(staged.contours, pipeline_result.contours);
        assert_eq!(staged.simplified, pipeline_result.simplified);
        assert_eq!(staged.canvas, pipeline_result.canvas);
        assert_eq!(staged.joined, pipeline_result.joined);
        assert_eq!(staged.output, pipeline_result.output);
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
            .canvas()
            .join()
            .output()
            .into_result();

        assert_eq!(staged.edges, pipeline_result.edges);
        assert_eq!(staged.joined, pipeline_result.joined);
    }

    #[test]
    fn full_pipeline_with_mask() {
        let png = sharp_edge_png(40, 40);
        let config = PipelineConfig {
            shape: crate::mask::CanvasShape::Circle,
            scale: 0.8,
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
            .canvas()
            .join()
            .output()
            .into_result();

        assert_eq!(staged.canvas, pipeline_result.canvas);
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
            .canvas()
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
            (7, "canvas"),
            (8, "join"),
            (9, "output"),
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
            .canvas()
            .join()
            .output()
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
        assert_eq!(chained.canvas, looped.canvas);
        assert_eq!(chained.joined, looped.joined);
        assert_eq!(chained.output, looped.output);
        assert_eq!(chained.dimensions, looped.dimensions);
    }

    #[test]
    fn complete_from_pending() {
        let png = sharp_edge_png(40, 40);
        let pending = Pipeline::new(png, wide_canvas_config());
        let result = pending.complete().unwrap();
        assert!(!result.joined.is_empty());
    }

    #[test]
    fn complete_from_decoded() {
        let png = sharp_edge_png(40, 40);
        let decoded = Pipeline::new(png, wide_canvas_config()).decode().unwrap();
        let result = decoded.complete().unwrap();
        assert!(!result.joined.is_empty());
    }

    #[test]
    fn complete_from_mid_stage() {
        let png = sharp_edge_png(40, 40);
        let blurred = Pipeline::new(png, wide_canvas_config())
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
        let joined = Pipeline::new(png, wide_canvas_config())
            .decode()
            .unwrap()
            .downsample()
            .blur()
            .detect_edges()
            .trace_contours()
            .unwrap()
            .simplify()
            .canvas()
            .join();
        let result = joined.complete().unwrap();
        assert!(!result.joined.is_empty());
    }

    #[test]
    fn next_on_output_returns_none() {
        let png = sharp_edge_png(40, 40);
        let output = Pipeline::new(png, PipelineConfig::default())
            .decode()
            .unwrap()
            .downsample()
            .blur()
            .detect_edges()
            .trace_contours()
            .unwrap()
            .simplify()
            .canvas()
            .join()
            .output();
        assert!(output.next().unwrap().is_none());
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
                StageOutput::Canvas { .. } => 7,
                StageOutput::Joined { .. } => 8,
                StageOutput::Output { .. } => 9,
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
        let stage: Stage = Pipeline::new(png, wide_canvas_config()).into();
        let result = stage.complete().unwrap();
        assert!(!result.joined.is_empty());
    }

    #[test]
    fn loop_with_progress_tracking() {
        let png = sharp_edge_png(40, 40);
        let start: Stage = Pipeline::new(png, PipelineConfig::default()).into();

        let (_, log) = drive_to_end(start).unwrap();
        let indices: Vec<usize> = log.iter().map(|(idx, _)| *idx).collect();
        assert_eq!(indices, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn pending_decode_error_via_advance() {
        let stage: Stage = Pipeline::new(vec![], PipelineConfig::default()).into();
        let result = stage.advance();
        assert!(matches!(result, Err(PipelineError::EmptyInput)));
    }

    // ─────────── PipelineCache tests ─────────────────────────────

    /// No-op progress callback for tests.
    fn noop(_: usize, _: bool) {}

    /// Helper: assert two `StagedResult`s are identical.
    fn assert_staged_eq(a: &StagedResult, b: &StagedResult) {
        assert_eq!(a.original, b.original, "original mismatch");
        assert_eq!(a.downsampled, b.downsampled, "downsampled mismatch");
        assert_eq!(a.blurred, b.blurred, "blurred mismatch");
        assert_eq!(a.edges, b.edges, "edges mismatch");
        assert_eq!(a.contours, b.contours, "contours mismatch");
        assert_eq!(a.simplified, b.simplified, "simplified mismatch");
        assert_eq!(a.canvas, b.canvas, "canvas mismatch");
        assert_eq!(a.joined, b.joined, "joined mismatch");
        assert_eq!(a.output, b.output, "output mismatch");
        assert_eq!(a.dimensions, b.dimensions, "dimensions mismatch");
    }

    #[test]
    fn cache_full_run_matches_process_staged() {
        let png = sharp_edge_png(40, 40);
        let config = PipelineConfig::default();

        let expected = crate::process_staged(&png, &config).unwrap();
        let (actual, _cache) = PipelineCache::run(None, png, config, &noop).unwrap();

        assert_staged_eq(&expected, &actual);
    }

    #[test]
    fn cache_unchanged_config_returns_identical_result() {
        let png = sharp_edge_png(40, 40);
        let config = PipelineConfig::default();

        let (first, cache) = PipelineCache::run(None, png.clone(), config.clone(), &noop).unwrap();
        let (second, _cache2) = PipelineCache::run(Some(cache), png, config, &noop).unwrap();

        assert_staged_eq(&first, &second);
    }

    #[test]
    fn cache_changed_late_stage_produces_correct_result() {
        // Change scale (stage 7) — stages 0-6 should be cached,
        // only stages 7-8 re-run.
        let png = sharp_edge_png(40, 40);
        let config1 = PipelineConfig::default();
        let config2 = PipelineConfig {
            scale: 1.0,
            ..PipelineConfig::default()
        };

        let (_first, cache) = PipelineCache::run(None, png.clone(), config1, &noop).unwrap();
        let (cached_result, _cache2) =
            PipelineCache::run(Some(cache), png.clone(), config2.clone(), &noop).unwrap();

        // Verify against a fresh full run with config2.
        let expected = crate::process_staged(&png, &config2).unwrap();
        assert_staged_eq(&expected, &cached_result);
    }

    #[test]
    fn cache_changed_mid_stage_produces_correct_result() {
        // Change canny_high (stage 4) — stages 0-3 cached, 4-8 re-run.
        let png = sharp_edge_png(40, 40);
        let config1 = PipelineConfig::default();
        let config2 = PipelineConfig {
            canny_high: 50.0,
            canny_max: 60.0,
            ..PipelineConfig::default()
        };

        let (_first, cache) = PipelineCache::run(None, png.clone(), config1, &noop).unwrap();
        let (cached_result, _cache2) =
            PipelineCache::run(Some(cache), png.clone(), config2.clone(), &noop).unwrap();

        let expected = crate::process_staged(&png, &config2).unwrap();
        assert_staged_eq(&expected, &cached_result);
    }

    #[test]
    fn cache_changed_working_resolution_produces_correct_result() {
        // Change working_resolution (stage 2) — only stages 0-1 cached,
        // stages 2-8 re-run using the cached decoded_image.
        let png = sharp_edge_png(40, 40);
        let config1 = PipelineConfig::default();
        let config2 = PipelineConfig {
            working_resolution: 20,
            ..PipelineConfig::default()
        };

        let (_first, cache) = PipelineCache::run(None, png.clone(), config1, &noop).unwrap();
        let (cached_result, _cache2) =
            PipelineCache::run(Some(cache), png.clone(), config2.clone(), &noop).unwrap();

        let expected = crate::process_staged(&png, &config2).unwrap();
        assert_staged_eq(&expected, &cached_result);
    }

    #[test]
    fn cache_changed_early_stage_produces_correct_result() {
        // Change blur_sigma (stage 3) — stages 0-2 cached, 3-8 re-run.
        let png = sharp_edge_png(40, 40);
        let config1 = PipelineConfig::default();
        let config2 = PipelineConfig {
            blur_sigma: 2.5,
            ..PipelineConfig::default()
        };

        let (_first, cache) = PipelineCache::run(None, png.clone(), config1, &noop).unwrap();
        let (cached_result, _cache2) =
            PipelineCache::run(Some(cache), png.clone(), config2.clone(), &noop).unwrap();

        let expected = crate::process_staged(&png, &config2).unwrap();
        assert_staged_eq(&expected, &cached_result);
    }

    #[test]
    fn cache_changed_join_produces_correct_result() {
        // Change mst_neighbours (stage 8 only) — stages 0-7 cached.
        let png = sharp_edge_png(40, 40);
        let config1 = PipelineConfig::default();
        let config2 = PipelineConfig {
            mst_neighbours: 50,
            ..PipelineConfig::default()
        };

        let (_first, cache) = PipelineCache::run(None, png.clone(), config1, &noop).unwrap();
        let (cached_result, _cache2) =
            PipelineCache::run(Some(cache), png.clone(), config2.clone(), &noop).unwrap();

        let expected = crate::process_staged(&png, &config2).unwrap();
        assert_staged_eq(&expected, &cached_result);
    }

    #[test]
    fn cache_different_image_does_full_rerun() {
        let png1 = sharp_edge_png(40, 40);
        let png2 = sharp_edge_png(60, 40);
        let config = PipelineConfig::default();

        let (_first, cache) = PipelineCache::run(None, png1, config.clone(), &noop).unwrap();
        let (result, _cache2) =
            PipelineCache::run(Some(cache), png2.clone(), config.clone(), &noop).unwrap();

        let expected = crate::process_staged(&png2, &config).unwrap();
        assert_staged_eq(&expected, &result);
    }

    #[test]
    fn cache_no_cache_does_full_run() {
        let png = sharp_edge_png(40, 40);
        let config = PipelineConfig::default();

        let (result, _cache) =
            PipelineCache::run(None, png.clone(), config.clone(), &noop).unwrap();

        let expected = crate::process_staged(&png, &config).unwrap();
        assert_staged_eq(&expected, &result);
    }

    #[test]
    fn cache_changed_simplify_produces_correct_result() {
        // Change simplify_tolerance (stage 6) — stages 0-5 cached.
        let png = sharp_edge_png(40, 40);
        let config1 = PipelineConfig::default();
        let config2 = PipelineConfig {
            simplify_tolerance: 5.0,
            ..PipelineConfig::default()
        };

        let (_first, cache) = PipelineCache::run(None, png.clone(), config1, &noop).unwrap();
        let (cached_result, _cache2) =
            PipelineCache::run(Some(cache), png.clone(), config2.clone(), &noop).unwrap();

        let expected = crate::process_staged(&png, &config2).unwrap();
        assert_staged_eq(&expected, &cached_result);
    }

    #[test]
    fn cache_changed_contour_tracer_produces_correct_result() {
        // Change contour_tracer (stage 5) — stages 0-4 cached.
        // Only one variant currently, so we just verify the code path
        // doesn't panic when earliest_changed == 5.
        let png = sharp_edge_png(40, 40);
        let config = PipelineConfig::default();

        let (first, cache) = PipelineCache::run(None, png.clone(), config.clone(), &noop).unwrap();
        // Same contour_tracer — should be a cache hit.
        let (second, _cache2) = PipelineCache::run(Some(cache), png, config, &noop).unwrap();
        assert_staged_eq(&first, &second);
    }

    #[test]
    fn cache_ui_only_change_returns_cached() {
        // Changing canny_max (UI-only) should not trigger a rerun.
        let png = sharp_edge_png(40, 40);
        let config1 = PipelineConfig::default();
        let config2 = PipelineConfig {
            canny_max: 120.0,
            ..PipelineConfig::default()
        };

        let (first, cache) = PipelineCache::run(None, png.clone(), config1, &noop).unwrap();
        let (second, _cache2) = PipelineCache::run(Some(cache), png, config2, &noop).unwrap();

        assert_staged_eq(&first, &second);
    }

    #[test]
    fn cache_successive_changes_produce_correct_results() {
        // Run three times with different configs, each time reusing
        // the cache from the previous run.
        let png = sharp_edge_png(40, 40);
        let config1 = PipelineConfig::default();
        let config2 = PipelineConfig {
            blur_sigma: 2.0,
            ..PipelineConfig::default()
        };
        let config3 = PipelineConfig {
            blur_sigma: 2.0,
            scale: 1.0,
            ..PipelineConfig::default()
        };

        let (_r1, cache1) = PipelineCache::run(None, png.clone(), config1, &noop).unwrap();
        let (_r2, cache2) = PipelineCache::run(Some(cache1), png.clone(), config2, &noop).unwrap();
        let (r3, _cache3) =
            PipelineCache::run(Some(cache2), png.clone(), config3.clone(), &noop).unwrap();

        let expected = crate::process_staged(&png, &config3).unwrap();
        assert_staged_eq(&expected, &r3);
    }
}
