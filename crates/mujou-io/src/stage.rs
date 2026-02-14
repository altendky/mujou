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
    /// Stage 1: downsampled to working resolution.
    Downsampled,
    /// Stage 3: Gaussian blur.
    Blur,
    /// Stages 4+5: Canny edge detection + optional inversion.
    Edges,
    /// Stage 6: contour tracing.
    Contours,
    /// Stage 7: RDP simplification.
    Simplified,
    /// Stage 8: circular mask.
    Masked,
    /// Stage 9: path joining.
    Path,
}

impl StageId {
    /// All stages in pipeline order, for iterating the filmstrip.
    pub const ALL: [Self; 8] = [
        Self::Original,
        Self::Downsampled,
        Self::Blur,
        Self::Edges,
        Self::Contours,
        Self::Simplified,
        Self::Masked,
        Self::Path,
    ];

    /// Full display label for the stage.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::Downsampled => "Downsampled",
            Self::Blur => "Blur",
            Self::Edges => "Edges",
            Self::Contours => "Contours",
            Self::Simplified => "Simplified",
            Self::Path => "Path",
            Self::Masked => "Masked",
        }
    }

    /// Map a pipeline-internal stage index to the corresponding UI stage.
    ///
    /// The pipeline has 9 internal stages (indices 0–8) while the UI
    /// presents 8 stages. Backend stages 0 (`Pending`/source) and 1
    /// (`Decoded`/decode) both map to [`StageId::Original`] because
    /// decode is the operation that produces the original preview image.
    ///
    /// Returns `None` for out-of-range indices.
    ///
    /// See also: <https://github.com/altendky/mujou/issues/122>
    #[must_use]
    pub const fn from_pipeline_index(index: usize) -> Option<Self> {
        match index {
            0 | 1 => Some(Self::Original), // Pending + Decoded
            2 => Some(Self::Downsampled),  // Downsampled
            3 => Some(Self::Blur),         // Blurred
            4 => Some(Self::Edges),        // EdgesDetected
            5 => Some(Self::Contours),     // ContoursTraced
            6 => Some(Self::Simplified),   // Simplified
            7 => Some(Self::Masked),       // Masked
            8 => Some(Self::Path),         // Joined
            _ => None,
        }
    }

    /// Short label for compact display at small and medium viewports.
    #[must_use]
    pub const fn short_label(self) -> &'static str {
        match self {
            Self::Original => "Orig",
            Self::Downsampled => "Down",
            Self::Blur => "Blur",
            Self::Edges => "Edge",
            Self::Contours => "Cont",
            Self::Simplified => "Simp",
            Self::Masked => "Mask",
            Self::Path => "Path",
        }
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

    #[test]
    fn from_pipeline_index_maps_all_backend_stages() {
        // Backend indices 0-8 should all map to a valid StageId.
        for i in 0..=8 {
            assert!(
                StageId::from_pipeline_index(i).is_some(),
                "pipeline index {i} should map to a StageId"
            );
        }
        // Out-of-range returns None.
        assert_eq!(StageId::from_pipeline_index(9), None);
        assert_eq!(StageId::from_pipeline_index(usize::MAX), None);
    }

    #[test]
    fn from_pipeline_index_merges_source_and_decode() {
        // Backend stages 0 (Pending) and 1 (Decoded) both map to Original.
        assert_eq!(StageId::from_pipeline_index(0), Some(StageId::Original));
        assert_eq!(StageId::from_pipeline_index(1), Some(StageId::Original));
    }

    #[test]
    fn short_labels_are_compact() {
        // Short labels should be at most 4 characters to fit in compact tiles.
        for stage in StageId::ALL {
            let short = stage.short_label();
            assert!(
                !short.is_empty(),
                "StageId::{stage} short_label() must not be empty"
            );
            assert!(
                short.len() <= 4,
                "StageId::{stage} short_label() {short:?} exceeds 4 characters"
            );
        }
    }

    #[test]
    fn from_pipeline_index_covers_all_ui_stages() {
        // Every StageId variant should be reachable from some pipeline index.
        let mut reachable = std::collections::HashSet::new();
        for i in 0..=8 {
            if let Some(stage) = StageId::from_pipeline_index(i) {
                reachable.insert(stage);
            }
        }
        for stage in StageId::ALL {
            assert!(
                reachable.contains(&stage),
                "StageId::{stage} is not reachable from any pipeline index"
            );
        }
    }
}
