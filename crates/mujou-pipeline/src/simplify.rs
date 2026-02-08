//! Path simplification using the Ramer-Douglas-Peucker algorithm.
//!
//! Reduces point count in polylines by removing points that are within
//! a given tolerance of the line between their neighbors. This is
//! implemented from scratch (~30 lines) to avoid pulling in the `geo`
//! crate dependency tree.
//!
//! This is step 6 in the pipeline, between contour tracing and path
//! optimization.

use crate::types::{Point, Polyline};

/// Simplify a single polyline using the Ramer-Douglas-Peucker algorithm.
///
/// Points within `tolerance` pixels of the line between their endpoints
/// are removed. A tolerance of 0.0 preserves all points.
///
/// Returns the simplified polyline. Polylines with fewer than 3 points
/// are returned unchanged (nothing to simplify).
#[must_use = "returns the simplified polyline"]
pub fn simplify(polyline: &Polyline, tolerance: f64) -> Polyline {
    let points = polyline.points();
    if points.len() < 3 {
        return polyline.clone();
    }

    let mut kept = vec![false; points.len()];
    kept[0] = true;
    kept[points.len() - 1] = true;

    rdp_recurse(points, 0, points.len() - 1, tolerance, &mut kept);

    let simplified: Vec<Point> = points
        .iter()
        .zip(&kept)
        .filter(|&(_, k)| *k)
        .map(|(&p, _)| p)
        .collect();

    Polyline::new(simplified)
}

/// Simplify multiple polylines, applying RDP to each independently.
#[must_use = "returns the simplified polylines"]
pub fn simplify_paths(polylines: &[Polyline], tolerance: f64) -> Vec<Polyline> {
    polylines.iter().map(|pl| simplify(pl, tolerance)).collect()
}

/// Recursive step of the Ramer-Douglas-Peucker algorithm.
///
/// Finds the point between `start` and `end` that is farthest from the
/// line segment between them. If that distance exceeds `tolerance`, the
/// point is kept and both sub-segments are processed recursively.
fn rdp_recurse(points: &[Point], start: usize, end: usize, tolerance: f64, kept: &mut [bool]) {
    if end <= start + 1 {
        return;
    }

    let mut max_dist = 0.0;
    let mut max_idx = start;

    for i in (start + 1)..end {
        let d = perpendicular_distance(points[i], points[start], points[end]);
        if d > max_dist {
            max_dist = d;
            max_idx = i;
        }
    }

    if max_dist > tolerance {
        kept[max_idx] = true;
        rdp_recurse(points, start, max_idx, tolerance, kept);
        rdp_recurse(points, max_idx, end, tolerance, kept);
    }
}

/// Perpendicular distance from point `p` to the line defined by `a` and `b`.
///
/// Uses the formula: |cross(b-a, p-a)| / |b-a|.
/// When `a` and `b` coincide, returns the distance from `p` to `a`.
fn perpendicular_distance(p: Point, a: Point, b: Point) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let length_sq = dx.mul_add(dx, dy * dy);

    if length_sq == 0.0 {
        // a and b are the same point.
        return p.distance(a);
    }

    // |cross product| / |line length|
    let cross = dx.mul_add(a.y - p.y, -(dy * (a.x - p.x)));
    cross.abs() / length_sq.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_polyline_unchanged() {
        let pl = Polyline::new(vec![]);
        let result = simplify(&pl, 1.0);
        assert!(result.is_empty());
    }

    #[test]
    fn single_point_unchanged() {
        let pl = Polyline::new(vec![Point::new(1.0, 2.0)]);
        let result = simplify(&pl, 1.0);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn two_points_unchanged() {
        let pl = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0)]);
        let result = simplify(&pl, 1.0);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn zero_tolerance_preserves_all_points() {
        let pl = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.1),
            Point::new(2.0, 0.0),
            Point::new(3.0, 0.05),
            Point::new(4.0, 0.0),
        ]);
        let result = simplify(&pl, 0.0);
        assert_eq!(result.len(), 5);
    }

    #[test]
    fn collinear_points_collapse_to_endpoints() {
        // Points on a straight line: the middle ones should all be removed.
        let pl = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(2.0, 2.0),
            Point::new(3.0, 3.0),
            Point::new(4.0, 4.0),
        ]);
        let result = simplify(&pl, 0.1);
        assert_eq!(result.len(), 2);
        assert_eq!(result.points()[0], Point::new(0.0, 0.0));
        assert_eq!(result.points()[1], Point::new(4.0, 4.0));
    }

    #[test]
    fn zigzag_retains_peaks() {
        // Zigzag pattern with peaks at y=5 -- should be retained with
        // tolerance smaller than the peak height.
        let pl = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(2.0, 5.0),
            Point::new(4.0, 0.0),
            Point::new(6.0, 5.0),
            Point::new(8.0, 0.0),
        ]);
        let result = simplify(&pl, 1.0);
        // All peaks are > 1.0 from the baseline, so all points should be kept.
        assert_eq!(result.len(), 5);
    }

    #[test]
    fn large_tolerance_collapses_zigzag() {
        let pl = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(2.0, 5.0),
            Point::new(4.0, 0.0),
            Point::new(6.0, 5.0),
            Point::new(8.0, 0.0),
        ]);
        let result = simplify(&pl, 10.0);
        // With tolerance larger than the peak height, only endpoints remain.
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn simplify_paths_applies_to_each() {
        let polylines = vec![
            Polyline::new(vec![
                Point::new(0.0, 0.0),
                Point::new(1.0, 1.0),
                Point::new(2.0, 2.0),
            ]),
            Polyline::new(vec![
                Point::new(0.0, 0.0),
                Point::new(1.0, 5.0),
                Point::new(2.0, 0.0),
            ]),
        ];
        let results = simplify_paths(&polylines, 0.5);
        assert_eq!(results.len(), 2);
        // First polyline: collinear, should collapse.
        assert_eq!(results[0].len(), 2);
        // Second polyline: peak at 5.0, should be kept.
        assert_eq!(results[1].len(), 3);
    }

    #[test]
    fn perpendicular_distance_on_axis() {
        // Point (1, 3) is 3 units from the line y=0 (from (0,0) to (2,0)).
        let d = perpendicular_distance(
            Point::new(1.0, 3.0),
            Point::new(0.0, 0.0),
            Point::new(2.0, 0.0),
        );
        assert!((d - 3.0).abs() < 1e-10);
    }

    #[test]
    fn perpendicular_distance_diagonal_segment() {
        // Point (2, -1) is ~1.789 from line (0,0)->(4,2).
        // Correct: |4*(-1) - 2*(-2)| / sqrt(20) = 8 / sqrt(20)
        let d = perpendicular_distance(
            Point::new(2.0, -1.0),
            Point::new(0.0, 0.0),
            Point::new(4.0, 2.0),
        );
        let expected = 8.0 / 20.0_f64.sqrt();
        assert!((d - expected).abs() < 1e-10, "got {d}, expected {expected}",);
    }

    #[test]
    fn perpendicular_distance_coincident_endpoints() {
        // When a and b are the same point, distance should be point-to-point.
        let d = perpendicular_distance(
            Point::new(3.0, 4.0),
            Point::new(0.0, 0.0),
            Point::new(0.0, 0.0),
        );
        assert!((d - 5.0).abs() < 1e-10);
    }
}
