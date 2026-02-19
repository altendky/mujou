//! Segment subsampling: break long line segments into shorter ones.
//!
//! This is step 9 in the pipeline, applied after path joining.
//!
//! Long straight line segments in Cartesian (XY) space can map to
//! unexpected arcs when converted to polar (theta-rho) coordinates for
//! the THR export format.  Subsampling inserts evenly-spaced
//! intermediate points along segments that exceed a maximum length,
//! ensuring the polar conversion produces smooth curves.
//!
//! [Sandify](https://github.com/jeffeb3/sandify) performs the same
//! operation (with a 2 mm threshold in machine coordinates) before
//! its Cartesian-to-polar conversion.  Since mujou works in pixel
//! space and doesn't know the physical table size, the threshold is
//! expressed in pixels.

use crate::types::{Point, Polyline};

/// Subdivide long segments in a polyline so no segment exceeds
/// `max_length` pixels.
///
/// For each consecutive pair of points where the Euclidean distance
/// exceeds `max_length`, evenly-spaced intermediate points are
/// inserted along the segment.  Short segments (≤ `max_length`) are
/// kept as-is.
///
/// Returns the polyline unchanged (by clone) when `max_length` is
/// non-positive or the polyline has fewer than 2 points.
///
/// # Examples
///
/// ```
/// use mujou_pipeline::{Point, Polyline};
/// use mujou_pipeline::subsample::subsample;
///
/// let polyline = Polyline::new(vec![
///     Point::new(0.0, 0.0),
///     Point::new(10.0, 0.0),
/// ]);
/// let result = subsample(&polyline, 3.0);
/// // 10px segment / 3px max → 4 sub-segments → 5 points
/// assert_eq!(result.len(), 5);
/// ```
#[must_use]
#[allow(clippy::many_single_char_names)]
pub fn subsample(polyline: &Polyline, max_length: f64) -> Polyline {
    let points = polyline.points();
    if points.len() < 2 || max_length <= 0.0 {
        return polyline.clone();
    }

    let mut result = Vec::with_capacity(points.len());
    result.push(points[0]);

    for window in points.windows(2) {
        let a = window[0];
        let b = window[1];
        let dist = a.distance(b);

        if dist > max_length {
            // Number of sub-segments: ceil(dist / max_length).
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let n = (dist / max_length).ceil() as usize;
            #[allow(clippy::cast_precision_loss)]
            let n_f = n as f64;
            // Insert n-1 intermediate points before the endpoint.
            for i in 1..n {
                #[allow(clippy::cast_precision_loss)]
                let t = i as f64 / n_f;
                let x = (b.x - a.x).mul_add(t, a.x);
                let y = (b.y - a.y).mul_add(t, a.y);
                result.push(Point::new(x, y));
            }
        }
        result.push(b);
    }

    Polyline::new(result)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Helper: build a simple polyline from (x, y) pairs.
    fn poly(coords: &[(f64, f64)]) -> Polyline {
        Polyline::new(coords.iter().map(|&(x, y)| Point::new(x, y)).collect())
    }

    // --- No-op cases ---

    #[test]
    fn empty_polyline_returns_empty() {
        let p = poly(&[]);
        let result = subsample(&p, 5.0);
        assert!(result.is_empty());
    }

    #[test]
    fn single_point_returns_unchanged() {
        let p = poly(&[(1.0, 2.0)]);
        let result = subsample(&p, 5.0);
        assert_eq!(result.len(), 1);
        assert_eq!(result.points()[0], Point::new(1.0, 2.0));
    }

    #[test]
    fn short_segment_returns_unchanged() {
        let p = poly(&[(0.0, 0.0), (3.0, 0.0)]);
        let result = subsample(&p, 5.0);
        assert_eq!(result.len(), 2);
        assert_eq!(result.points()[0], Point::new(0.0, 0.0));
        assert_eq!(result.points()[1], Point::new(3.0, 0.0));
    }

    #[test]
    fn segment_exactly_max_length_not_split() {
        let p = poly(&[(0.0, 0.0), (5.0, 0.0)]);
        let result = subsample(&p, 5.0);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn zero_max_length_returns_unchanged() {
        let p = poly(&[(0.0, 0.0), (10.0, 0.0)]);
        let result = subsample(&p, 0.0);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn negative_max_length_returns_unchanged() {
        let p = poly(&[(0.0, 0.0), (10.0, 0.0)]);
        let result = subsample(&p, -1.0);
        assert_eq!(result.len(), 2);
    }

    // --- Splitting cases ---

    #[test]
    fn long_horizontal_segment_is_split() {
        // 10px segment with max_length 3px → ceil(10/3) = 4 sub-segments → 5 points.
        let p = poly(&[(0.0, 0.0), (10.0, 0.0)]);
        let result = subsample(&p, 3.0);
        assert_eq!(result.len(), 5);

        // First and last points preserved exactly.
        assert_eq!(result.points()[0], Point::new(0.0, 0.0));
        assert_eq!(result.points()[4], Point::new(10.0, 0.0));

        // Intermediate points evenly spaced at x = 2.5, 5.0, 7.5.
        let eps = 1e-10;
        assert!((result.points()[1].x - 2.5).abs() < eps);
        assert!((result.points()[2].x - 5.0).abs() < eps);
        assert!((result.points()[3].x - 7.5).abs() < eps);
        for pt in result.points() {
            assert!((pt.y - 0.0).abs() < eps);
        }
    }

    #[test]
    fn long_vertical_segment_is_split() {
        // 12px vertical segment with max_length 4px → 3 sub-segments → 4 points.
        let p = poly(&[(0.0, 0.0), (0.0, 12.0)]);
        let result = subsample(&p, 4.0);
        assert_eq!(result.len(), 4);

        let eps = 1e-10;
        assert!((result.points()[1].y - 4.0).abs() < eps);
        assert!((result.points()[2].y - 8.0).abs() < eps);
        assert!((result.points()[3].y - 12.0).abs() < eps);
    }

    #[test]
    fn diagonal_segment_is_split() {
        // 3-4-5 triangle: distance from (0,0) to (3,4) = 5.
        // max_length = 2 → ceil(5/2) = 3 sub-segments → 4 points.
        let p = poly(&[(0.0, 0.0), (3.0, 4.0)]);
        let result = subsample(&p, 2.0);
        assert_eq!(result.len(), 4);

        let eps = 1e-10;
        // t = 1/3: (1.0, 1.333...)
        assert!((result.points()[1].x - 1.0).abs() < eps);
        assert!((result.points()[1].y - 4.0 / 3.0).abs() < eps);
        // t = 2/3: (2.0, 2.666...)
        assert!((result.points()[2].x - 2.0).abs() < eps);
        assert!((result.points()[2].y - 8.0 / 3.0).abs() < eps);
    }

    // --- Multi-segment polylines ---

    #[test]
    fn mixed_short_and_long_segments() {
        // Segment 1: (0,0)→(2,0) = 2px (short, no split)
        // Segment 2: (2,0)→(12,0) = 10px (long, split into 4 → 5 points incl. endpoints)
        // Segment 3: (12,0)→(13,0) = 1px (short, no split)
        let p = poly(&[(0.0, 0.0), (2.0, 0.0), (12.0, 0.0), (13.0, 0.0)]);
        let result = subsample(&p, 3.0);

        // Original 4 points → 1 (first) + 1 (second, short seg) + 4 (3 intermediate + endpoint) + 1 (last short) = 7
        assert_eq!(result.len(), 7);

        // Endpoints preserved.
        assert_eq!(result.points()[0], Point::new(0.0, 0.0));
        assert_eq!(*result.points().last().unwrap(), Point::new(13.0, 0.0));
    }

    #[test]
    fn all_segments_short_returns_same_count() {
        let p = poly(&[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 0.0)]);
        let result = subsample(&p, 5.0);
        assert_eq!(result.len(), 4);
    }

    // --- Sub-segment length verification ---

    #[test]
    fn no_sub_segment_exceeds_max_length() {
        // A long polyline with varying segment lengths.
        let p = poly(&[
            (0.0, 0.0),
            (50.0, 0.0),
            (50.0, 30.0),
            (10.0, 10.0),
            (10.0, 100.0),
        ]);
        let max_len = 5.0;
        let result = subsample(&p, max_len);

        // Every consecutive pair should have distance ≤ max_len + epsilon.
        let pts = result.points();
        for i in 0..pts.len() - 1 {
            let d = pts[i].distance(pts[i + 1]);
            assert!(
                d <= max_len + 1e-9,
                "segment {i}→{} has length {d}, exceeds max {max_len}",
                i + 1,
            );
        }
    }

    #[test]
    fn sub_segments_are_approximately_equal_length() {
        // A single long segment: all sub-segments should have the same length.
        let p = poly(&[(0.0, 0.0), (100.0, 0.0)]);
        let result = subsample(&p, 7.0);

        let pts = result.points();
        let lengths: Vec<f64> = pts.windows(2).map(|w| w[0].distance(w[1])).collect();

        // All sub-segments should be equal (100 / ceil(100/7) = 100/15 ≈ 6.667).
        let expected = 100.0 / (100.0_f64 / 7.0).ceil();
        for (i, &len) in lengths.iter().enumerate() {
            assert!(
                (len - expected).abs() < 1e-10,
                "sub-segment {i} has length {len}, expected {expected}",
            );
        }
    }
}
