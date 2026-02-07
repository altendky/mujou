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
}
