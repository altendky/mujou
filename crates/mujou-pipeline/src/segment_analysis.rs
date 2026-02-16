//! Segment analysis utilities for diagnostic overlays and exports.
//!
//! Identifies the longest individual segments across a set of polylines,
//! returning ranked results suitable for both SVG export diagnostics
//! and live UI overlays.

use crate::Polyline;

/// Distinct colors for the top-N highlighted segments.
///
/// Chosen for visibility against a dark background and mutual
/// distinguishability. The palette cycles if `top_n` exceeds the
/// array length.
pub const SEGMENT_COLORS: &[&str] = &[
    "#ff3333", // red
    "#ff8800", // orange
    "#ffdd00", // yellow
    "#33cc33", // green
    "#3399ff", // blue
];

/// A single segment identified for diagnostic highlighting.
///
/// Represents the line between two consecutive points in a polyline,
/// ranked by Euclidean length among all segments across the input
/// polylines.
pub struct RankedSegment {
    /// Polyline index within the input slice.
    pub poly_idx: usize,
    /// Segment index within the polyline (from point `seg_idx` to `seg_idx + 1`).
    pub seg_idx: usize,
    /// Start point `(x, y)`.
    pub from: (f64, f64),
    /// End point `(x, y)`.
    pub to: (f64, f64),
    /// Euclidean length in pixels.
    pub length: f64,
}

/// Find the top N longest segments across all polylines.
///
/// Returns segments sorted by length descending, truncated to `top_n`.
/// Each segment records which polyline and segment index it came from,
/// enabling both visual highlighting and textual diagnostics.
///
/// # Examples
///
/// ```
/// use mujou_pipeline::{Point, Polyline};
/// use mujou_pipeline::segment_analysis::find_top_segments;
///
/// let polyline = Polyline::new(vec![
///     Point::new(0.0, 0.0),
///     Point::new(3.0, 4.0),  // length 5
///     Point::new(3.0, 5.0),  // length 1
/// ]);
/// let top = find_top_segments(&[polyline], 1);
/// assert_eq!(top.len(), 1);
/// assert!((top[0].length - 5.0).abs() < 1e-10);
/// ```
#[must_use]
pub fn find_top_segments(polylines: &[Polyline], top_n: usize) -> Vec<RankedSegment> {
    let total: usize = polylines
        .iter()
        .map(|p| p.points().len().saturating_sub(1))
        .sum();
    let mut all_segments: Vec<RankedSegment> = Vec::with_capacity(total);

    for (poly_idx, polyline) in polylines.iter().enumerate() {
        let pts = polyline.points();
        for seg_idx in 0..pts.len().saturating_sub(1) {
            let from = pts[seg_idx];
            let to = pts[seg_idx + 1];
            let length = from.distance(to);
            all_segments.push(RankedSegment {
                poly_idx,
                seg_idx,
                from: (from.x, from.y),
                to: (to.x, to.y),
                length,
            });
        }
    }

    // Sort descending by length, take top N.
    all_segments.sort_by(|a, b| {
        b.length
            .partial_cmp(&a.length)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    all_segments.truncate(top_n);

    all_segments
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::Point;

    #[test]
    fn empty_input_returns_empty() {
        let result = find_top_segments(&[], 5);
        assert!(result.is_empty());
    }

    #[test]
    fn single_point_polyline_returns_empty() {
        let poly = Polyline::new(vec![Point::new(0.0, 0.0)]);
        let result = find_top_segments(&[poly], 5);
        assert!(result.is_empty());
    }

    #[test]
    fn single_segment_returns_one() {
        let poly = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(3.0, 4.0)]);
        let result = find_top_segments(&[poly], 5);
        assert_eq!(result.len(), 1);
        assert!((result[0].length - 5.0).abs() < 1e-10);
        assert_eq!(result[0].poly_idx, 0);
        assert_eq!(result[0].seg_idx, 0);
    }

    #[test]
    fn top_n_truncates() {
        let poly = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0), // length 10
            Point::new(10.0, 5.0), // length 5
            Point::new(10.0, 8.0), // length 3
            Point::new(10.0, 9.0), // length 1
        ]);
        let result = find_top_segments(&[poly], 2);
        assert_eq!(result.len(), 2);
        assert!((result[0].length - 10.0).abs() < 1e-10);
        assert!((result[1].length - 5.0).abs() < 1e-10);
    }

    #[test]
    fn multiple_polylines_tracks_poly_idx() {
        let p0 = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(1.0, 0.0)]); // length 1
        let p1 = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(0.0, 100.0)]); // length 100
        let result = find_top_segments(&[p0, p1], 1);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].poly_idx, 1);
        assert!((result[0].length - 100.0).abs() < 1e-10);
    }

    #[test]
    fn sorted_descending_by_length() {
        let poly = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0), // length 1
            Point::new(1.0, 7.0), // length 7
            Point::new(4.0, 7.0), // length 3
        ]);
        let result = find_top_segments(&[poly], 10);
        assert_eq!(result.len(), 3);
        assert!(result[0].length >= result[1].length);
        assert!(result[1].length >= result[2].length);
    }
}
