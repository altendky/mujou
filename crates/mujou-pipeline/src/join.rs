//! Path joining: connect disconnected contours into a single continuous path.
//!
//! Sand tables cannot lift the ball -- every movement draws a visible line.
//! The output must be a single continuous path. This module defines the
//! [`PathJoiner`] trait for pluggable joining strategies and the
//! [`PathJoinerKind`] enum for runtime selection.

use crate::types::Polyline;

/// Selects which path joining strategy to use.
///
/// MVP ships with [`StraightLine`](Self::StraightLine) only.
/// Additional variants (retrace, edge-aware routing, spiral) can be added
/// without changing the `PipelineConfig` struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PathJoinerKind {
    /// Connect the end of each contour to the start of the next with a
    /// straight line segment.
    ///
    /// Simplest strategy. Produces visible straight scratches between
    /// features, but scratch length is minimized by prior path optimization.
    #[default]
    StraightLine,

    /// Retrace backward along the previous contour before jumping to the next.
    ///
    /// After finishing contour N, walk backward along its points to find the
    /// position that minimizes the Euclidean distance to the start of contour
    /// N+1. The retraced segment follows an already-drawn groove (invisible in
    /// sand), while the remaining straight-line jump is shorter than the
    /// original `StraightLine` connection.
    ///
    /// Produces a longer total path but fewer visible artifacts.
    Retrace,
}

/// Trait for path joining strategies.
///
/// Input: ordered disconnected contours (already optimized for minimal travel).
/// Output: a single continuous polyline.
pub trait PathJoiner {
    /// Join the given contours into a single continuous path.
    fn join(&self, contours: &[Polyline]) -> Polyline;
}

impl PathJoiner for PathJoinerKind {
    fn join(&self, contours: &[Polyline]) -> Polyline {
        match *self {
            Self::StraightLine => join_straight_line(contours),
            Self::Retrace => join_retrace(contours),
        }
    }
}

/// Connect contours end-to-start with straight line segments.
///
/// The connecting segments are implicit -- the last point of contour N
/// and the first point of contour N+1 form the straight-line jump.
fn join_straight_line(contours: &[Polyline]) -> Polyline {
    let total_points: usize = contours.iter().map(Polyline::len).sum();
    let mut points = Vec::with_capacity(total_points);

    for contour in contours {
        points.extend_from_slice(contour.points());
    }

    Polyline::new(points)
}

/// Retrace backward along each contour to minimize the visible jump to the next.
///
/// For each pair of consecutive contours (N, N+1):
/// 1. Emit all of contour N's points (forward).
/// 2. Walk backward through contour N's points, finding the index whose
///    distance to contour N+1's first point is minimal.
/// 3. Emit the reversed suffix from contour N's last point back to (and
///    including) that optimal backtrack point.
///
/// The retraced segment overlaps an already-drawn groove so it is invisible
/// in sand. The remaining implicit jump (from the backtrack point to contour
/// N+1's start) is shorter than the original straight-line connection.
fn join_retrace(contours: &[Polyline]) -> Polyline {
    if contours.is_empty() {
        return Polyline::new(Vec::new());
    }

    let mut points = Vec::new();
    // At minimum, we emit every contour's points (same as straight-line).
    let lower_bound: usize = contours.iter().map(Polyline::len).sum();
    points.reserve(lower_bound);

    for (i, contour) in contours.iter().enumerate() {
        // Emit the contour in its original (forward) direction.
        points.extend_from_slice(contour.points());

        // If there is a next contour, retrace backward to minimize the jump.
        if let Some(next) = contours.get(i + 1)
            && let Some(next_start) = next.first()
        {
            let src = contour.points();
            if src.len() >= 2 {
                // Find the point in contour N that is closest to the
                // start of contour N+1. We search all points (including
                // the last, which is the starting position of the
                // backtrack) and pick the index with the minimum distance.
                let best_idx = src
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| {
                        a.distance_squared(*next_start)
                            .total_cmp(&b.distance_squared(*next_start))
                    })
                    .map_or(src.len() - 1, |(idx, _)| idx);

                // Only retrace if the best point is not the last point
                // (i.e. backtracking actually helps).
                if best_idx < src.len() - 1 {
                    // Walk backward from the second-to-last point down to
                    // the best index (inclusive).
                    for j in (best_idx..src.len() - 1).rev() {
                        points.push(src[j]);
                    }
                }
            }
        }
    }

    Polyline::new(points)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Point;

    #[test]
    fn default_is_straight_line() {
        assert_eq!(PathJoinerKind::default(), PathJoinerKind::StraightLine);
    }

    #[test]
    fn join_empty_contours() {
        let result = PathJoinerKind::StraightLine.join(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn join_single_contour() {
        let contour = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(2.0, 0.0),
        ]);
        let result = PathJoinerKind::StraightLine.join(std::slice::from_ref(&contour));
        assert_eq!(result, contour);
    }

    #[test]
    fn join_two_contours_concatenates_points() {
        let c1 = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)]);
        let c2 = Polyline::new(vec![Point::new(5.0, 5.0), Point::new(6.0, 6.0)]);

        let result = PathJoinerKind::StraightLine.join(&[c1, c2]);

        assert_eq!(result.len(), 4);
        assert_eq!(result.points()[0], Point::new(0.0, 0.0));
        assert_eq!(result.points()[1], Point::new(1.0, 1.0));
        // The jump from (1,1) to (5,5) is the implicit straight-line connection
        assert_eq!(result.points()[2], Point::new(5.0, 5.0));
        assert_eq!(result.points()[3], Point::new(6.0, 6.0));
    }

    #[test]
    fn join_preserves_total_point_count() {
        let contours: Vec<Polyline> = (0..5)
            .map(|i| {
                let base = f64::from(i) * 10.0;
                Polyline::new(vec![
                    Point::new(base, 0.0),
                    Point::new(base + 1.0, 1.0),
                    Point::new(base + 2.0, 0.0),
                ])
            })
            .collect();

        let result = PathJoinerKind::StraightLine.join(&contours);
        assert_eq!(result.len(), 15); // 5 contours * 3 points each
    }

    // --- Retrace tests ---

    #[test]
    fn retrace_empty_contours() {
        let result = PathJoinerKind::Retrace.join(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn retrace_single_contour() {
        let contour = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(2.0, 0.0),
        ]);
        let result = PathJoinerKind::Retrace.join(std::slice::from_ref(&contour));
        // Single contour: no joining needed, output equals input.
        assert_eq!(result, contour);
    }

    #[test]
    fn retrace_two_contours_adds_backtrack_points() {
        // Contour 1: (0,0) -> (10,0) -> (20,0)
        // Contour 2 starts at (11,1), which is closest to (10,0) at index 1
        //
        // Expected output:
        //   Forward c1:  (0,0), (10,0), (20,0)
        //   Backtrack:   (10,0)  -- retrace from end back to index 1
        //   Forward c2:  (11,1), (12,1)
        let c1 = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(20.0, 0.0),
        ]);
        let c2 = Polyline::new(vec![Point::new(11.0, 1.0), Point::new(12.0, 1.0)]);

        let result = PathJoinerKind::Retrace.join(&[c1, c2]);
        let pts = result.points();

        // 3 (forward c1) + 1 (backtrack to index 1) + 2 (forward c2) = 6
        assert_eq!(pts.len(), 6);
        // Forward c1
        assert_eq!(pts[0], Point::new(0.0, 0.0));
        assert_eq!(pts[1], Point::new(10.0, 0.0));
        assert_eq!(pts[2], Point::new(20.0, 0.0));
        // Backtrack to the point closest to c2's start
        assert_eq!(pts[3], Point::new(10.0, 0.0));
        // Forward c2
        assert_eq!(pts[4], Point::new(11.0, 1.0));
        assert_eq!(pts[5], Point::new(12.0, 1.0));
    }

    #[test]
    fn retrace_picks_optimal_backtrack_point() {
        // Contour 1 is a zigzag: (0,0) -> (10,10) -> (20,0) -> (30,10)
        // Contour 2 starts at (21,1), closest to (20,0) at index 2
        //
        // Backtrack from index 3 (end) back to index 2 inclusive.
        let c1 = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(20.0, 0.0),
            Point::new(30.0, 10.0),
        ]);
        let c2 = Polyline::new(vec![Point::new(21.0, 1.0)]);

        let result = PathJoinerKind::Retrace.join(&[c1, c2]);
        let pts = result.points();

        // 4 (forward c1) + 1 (backtrack: index 2) + 1 (forward c2) = 6
        assert_eq!(pts.len(), 6);
        // The backtrack point should be (20,0)
        assert_eq!(pts[4], Point::new(20.0, 0.0));
        assert_eq!(pts[5], Point::new(21.0, 1.0));
    }

    #[test]
    fn retrace_no_backtrack_when_end_is_closest() {
        // If the last point of contour N is already the closest to N+1's
        // start, no backtrack points are emitted (same as StraightLine).
        let c1 = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(5.0, 0.0),
        ]);
        let c2 = Polyline::new(vec![Point::new(6.0, 0.0)]);

        let result = PathJoinerKind::Retrace.join(&[c1.clone(), c2.clone()]);
        let straight = PathJoinerKind::StraightLine.join(&[c1, c2]);

        // When end is closest, retrace degrades to straight-line behavior.
        assert_eq!(result, straight);
    }

    #[test]
    fn retrace_has_at_least_as_many_points_as_straight_line() {
        let contours: Vec<Polyline> = (0..5)
            .map(|i| {
                let base = f64::from(i) * 10.0;
                Polyline::new(vec![
                    Point::new(base, 0.0),
                    Point::new(base + 3.0, 5.0),
                    Point::new(base + 6.0, 0.0),
                ])
            })
            .collect();

        let retrace = PathJoinerKind::Retrace.join(&contours);
        let straight = PathJoinerKind::StraightLine.join(&contours);

        assert!(retrace.len() >= straight.len());
    }
}
