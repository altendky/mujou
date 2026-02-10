//! Pipeline stage identifiers and metadata.
//!
//! Each [`StageId`] variant represents one logical step in the image
//! processing pipeline, combining related internal stages into
//! user-facing tiles.

use std::fmt;

/// Identifier for a logical pipeline stage in the filmstrip UI.
///
/// Some pipeline stages are combined for UI purposes:
/// - **Edges** combines Canny edge detection + optional inversion
/// - **Path optimization** (stage 7) is omitted — its output is
///   visually indistinguishable from simplification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StageId {
    /// Stage 0: original source image (RGBA, pre-processing).
    Original,
    /// Stage 1: decode + grayscale conversion.
    Grayscale,
    /// Stage 2: Gaussian blur.
    Blur,
    /// Stages 3+4: Canny edge detection + optional inversion.
    Edges,
    /// Stage 5: contour tracing.
    Contours,
    /// Stage 6: RDP simplification.
    Simplified,
    /// Stage 8: path joining.
    Path,
    /// Stage 9: circular mask.
    Masked,
}

impl StageId {
    /// All stages in pipeline order, for iterating the filmstrip.
    pub const ALL: [Self; 8] = [
        Self::Original,
        Self::Grayscale,
        Self::Blur,
        Self::Edges,
        Self::Contours,
        Self::Simplified,
        Self::Path,
        Self::Masked,
    ];

    /// Full display label for the stage.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::Grayscale => "Grayscale",
            Self::Blur => "Blur",
            Self::Edges => "Edges",
            Self::Contours => "Contours",
            Self::Simplified => "Simplified",
            Self::Path => "Path",
            Self::Masked => "Masked",
        }
    }

    /// Short abbreviation for compact mobile display.
    #[must_use]
    pub const fn abbreviation(self) -> &'static str {
        match self {
            Self::Original => "O",
            Self::Grayscale => "G",
            Self::Blur => "B",
            Self::Edges => "E",
            Self::Contours => "C",
            Self::Simplified => "S",
            Self::Path => "P",
            Self::Masked => "M",
        }
    }

    /// Whether this stage produces raster image output.
    ///
    /// Raster stages are displayed as `<img>` elements via Blob URLs.
    /// Vector stages are displayed as inline SVG.
    #[must_use]
    pub const fn is_raster(self) -> bool {
        matches!(
            self,
            Self::Original | Self::Grayscale | Self::Blur | Self::Edges
        )
    }
}

impl fmt::Display for StageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_contains_every_variant() {
        // If you add a variant to StageId, update ALL and this count.
        assert_eq!(
            StageId::ALL.len(),
            8,
            "StageId::ALL length must match variant count"
        );
        // Verify no duplicates.
        let mut seen = std::collections::HashSet::new();
        for stage in StageId::ALL {
            assert!(seen.insert(stage), "Duplicate stage in ALL: {stage}");
        }
    }
}
