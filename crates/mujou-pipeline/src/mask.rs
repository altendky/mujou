//! Circular mask: clip polylines to a circle.
//!
//! For round sand tables (Sisyphus, Oasis Mini), all output paths must
//! fit within a circular boundary. Points outside the circle are removed,
//! and line segments that cross the boundary are split at the intersection
//! point on the circle.
//!
//! This is step 9 in the pipeline (optional), applied after path joining.

use crate::types::{Point, Polyline};

/// Clip a single polyline to a circle, splitting at boundary crossings.
///
/// Segments that cross the circle boundary are split at the intersection
/// point. The result may be multiple polylines if the original path
/// exits and re-enters the circle.
///
/// Points exactly on the boundary (within floating-point tolerance) are
/// considered inside.
#[must_use = "returns the clipped polyline segments"]
pub fn clip_polyline_to_circle(polyline: &Polyline, center: Point, radius: f64) -> Vec<Polyline> {
    let points = polyline.points();
    if points.is_empty() {
        return Vec::new();
    }

    let radius_sq = radius * radius;
    let mut result = Vec::new();
    let mut current_segment: Vec<Point> = Vec::new();

    for i in 0..points.len() {
        let p = points[i];
        let p_inside = is_inside(p, center, radius_sq);

        if i == 0 {
            if p_inside {
                current_segment.push(p);
            }
            continue;
        }

        let prev = points[i - 1];
        let prev_inside = is_inside(prev, center, radius_sq);

        match (prev_inside, p_inside) {
            (true, true) => {
                // Both inside: just add the point.
                current_segment.push(p);
            }
            (true, false) => {
                // Exiting the circle: add intersection, finish segment.
                if let Some(ix) = line_circle_intersection(prev, p, center, radius) {
                    current_segment.push(ix);
                }
                if current_segment.len() >= 2 {
                    result.push(Polyline::new(std::mem::take(&mut current_segment)));
                } else {
                    current_segment.clear();
                }
            }
            (false, true) => {
                // Entering the circle: start new segment from intersection.
                if let Some(ix) = line_circle_intersection(prev, p, center, radius) {
                    current_segment.push(ix);
                }
                current_segment.push(p);
            }
            (false, false) => {
                // Both outside: check if the segment passes through the circle.
                let intersections = line_circle_both_intersections(prev, p, center, radius);
                if let Some((ix1, ix2)) = intersections {
                    // The segment crosses through the circle.
                    if current_segment.len() >= 2 {
                        result.push(Polyline::new(std::mem::take(&mut current_segment)));
                    } else {
                        current_segment.clear();
                    }
                    result.push(Polyline::new(vec![ix1, ix2]));
                }
            }
        }
    }

    // Flush any remaining segment.
    if current_segment.len() >= 2 {
        result.push(Polyline::new(current_segment));
    }

    result
}

/// Clip multiple polylines to a circle.
///
/// Each input polyline is independently clipped. A single input polyline
/// may produce multiple output polylines if it exits and re-enters the
/// circle.
#[must_use = "returns the clipped polylines"]
pub fn apply_circular_mask(polylines: &[Polyline], center: Point, radius: f64) -> Vec<Polyline> {
    polylines
        .iter()
        .flat_map(|pl| clip_polyline_to_circle(pl, center, radius))
        .collect()
}

/// Check if a point is inside or on the circle.
fn is_inside(p: Point, center: Point, radius_sq: f64) -> bool {
    p.distance_squared(center) <= radius_sq
}

/// Find the intersection point of a line segment (from `a` inside to `b`
/// outside, or vice versa) with a circle. Returns the intersection
/// closest to `a`.
///
/// Uses the parametric form: `P(t) = a + t*(b - a)` for `t` in `[0, 1]`.
/// Substitutes into `|P(t) - center|^2 = radius^2` to get a quadratic in `t`.
fn line_circle_intersection(a: Point, b: Point, center: Point, radius: f64) -> Option<Point> {
    let solutions = solve_line_circle(a, b, center, radius)?;

    // Return the first valid intersection in [0, 1] closest to a.
    let t = if (0.0..=1.0).contains(&solutions.0) {
        solutions.0
    } else if (0.0..=1.0).contains(&solutions.1) {
        solutions.1
    } else {
        return None;
    };

    Some(lerp(a, b, t))
}

/// Find both intersection points where a line segment passes through a circle.
///
/// Returns `Some((entry, exit))` if the segment crosses through the circle
/// (both intersection parameters are in `[0, 1]` and are distinct).
/// The entry point is the one closer to `a`.
fn line_circle_both_intersections(
    a: Point,
    b: Point,
    center: Point,
    radius: f64,
) -> Option<(Point, Point)> {
    let (t1, t2) = solve_line_circle(a, b, center, radius)?;

    if (0.0..=1.0).contains(&t1) && (0.0..=1.0).contains(&t2) && (t1 - t2).abs() > 1e-12 {
        let p1 = lerp(a, b, t1);
        let p2 = lerp(a, b, t2);
        Some((p1, p2))
    } else {
        None
    }
}

/// Solve the quadratic for line-circle intersection parameters.
///
/// Returns `Some((t_small, t_large))` where `t_small <= t_large`.
/// Returns `None` if the line misses the circle (negative discriminant).
fn solve_line_circle(a: Point, b: Point, center: Point, radius: f64) -> Option<(f64, f64)> {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let fx = a.x - center.x;
    let fy = a.y - center.y;

    let a_coeff = dx.mul_add(dx, dy * dy);
    let b_coeff = 2.0 * dx.mul_add(fx, dy * fy);
    let c_coeff = radius.mul_add(-radius, fx.mul_add(fx, fy * fy));

    let discriminant = b_coeff.mul_add(b_coeff, -4.0 * a_coeff * c_coeff);

    if discriminant < 0.0 {
        return None;
    }

    let sqrt_disc = discriminant.sqrt();
    let t1 = (-b_coeff - sqrt_disc) / (2.0 * a_coeff);
    let t2 = (-b_coeff + sqrt_disc) / (2.0 * a_coeff);

    Some((t1, t2))
}

/// Linear interpolation between two points.
fn lerp(a: Point, b: Point, t: f64) -> Point {
    Point::new(t.mul_add(b.x - a.x, a.x), t.mul_add(b.y - a.y, a.y))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CENTER: Point = Point::new(0.0, 0.0);
    const RADIUS: f64 = 10.0;

    #[test]
    fn empty_polyline_returns_empty() {
        let pl = Polyline::new(vec![]);
        let result = clip_polyline_to_circle(&pl, CENTER, RADIUS);
        assert!(result.is_empty());
    }

    #[test]
    fn entirely_inside_unchanged() {
        let pl = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(3.0, 0.0),
            Point::new(3.0, 3.0),
        ]);
        let result = clip_polyline_to_circle(&pl, CENTER, RADIUS);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 3);
        assert_eq!(result[0], pl);
    }

    #[test]
    fn entirely_outside_returns_empty() {
        let pl = Polyline::new(vec![
            Point::new(20.0, 20.0),
            Point::new(30.0, 20.0),
            Point::new(30.0, 30.0),
        ]);
        let result = clip_polyline_to_circle(&pl, CENTER, RADIUS);
        assert!(result.is_empty());
    }

    #[test]
    fn segment_exiting_circle_is_clipped() {
        // Path starts inside (0,0) and goes to (20,0), exiting circle at ~(10,0).
        let pl = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(20.0, 0.0)]);
        let result = clip_polyline_to_circle(&pl, CENTER, RADIUS);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 2);
        // First point unchanged.
        assert_eq!(result[0].points()[0], Point::new(0.0, 0.0));
        // Second point should be on the circle boundary at ~(10, 0).
        let clipped = result[0].points()[1];
        assert!(
            (clipped.distance(CENTER) - RADIUS).abs() < 1e-6,
            "clipped point should be on circle boundary, distance = {}",
            clipped.distance(CENTER),
        );
    }

    #[test]
    fn segment_entering_circle_is_clipped() {
        // Path starts outside (-20, 0) and goes to (0, 0), entering at ~(-10, 0).
        let pl = Polyline::new(vec![Point::new(-20.0, 0.0), Point::new(0.0, 0.0)]);
        let result = clip_polyline_to_circle(&pl, CENTER, RADIUS);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 2);
        // First point should be on circle boundary at ~(-10, 0).
        let entry = result[0].points()[0];
        assert!(
            (entry.distance(CENTER) - RADIUS).abs() < 1e-6,
            "entry point should be on circle boundary, distance = {}",
            entry.distance(CENTER),
        );
        // Last point is origin.
        assert_eq!(result[0].points()[1], Point::new(0.0, 0.0));
    }

    #[test]
    fn path_crossing_through_produces_clipped_segment() {
        // Path goes from (-20, 0) to (20, 0), crossing through the circle.
        // Should produce a single segment from ~(-10, 0) to ~(10, 0).
        let pl = Polyline::new(vec![Point::new(-20.0, 0.0), Point::new(20.0, 0.0)]);
        let result = clip_polyline_to_circle(&pl, CENTER, RADIUS);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 2);

        for p in result[0].points() {
            assert!(
                (p.distance(CENTER) - RADIUS).abs() < 1e-6,
                "intersection point should be on circle, distance = {}",
                p.distance(CENTER),
            );
        }
    }

    #[test]
    fn exit_and_reenter_produces_two_segments() {
        // Path: inside -> outside -> inside.
        // (0,0) -> (0,20) -> (5,0)
        // Should produce two clipped segments.
        let pl = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(0.0, 20.0),
            Point::new(5.0, 0.0),
        ]);
        let result = clip_polyline_to_circle(&pl, CENTER, RADIUS);
        assert_eq!(
            result.len(),
            2,
            "expected 2 segments for exit-and-reenter path, got {}",
            result.len()
        );
    }

    #[test]
    fn apply_circular_mask_processes_multiple_polylines() {
        let polylines = vec![
            Polyline::new(vec![Point::new(0.0, 0.0), Point::new(3.0, 0.0)]),
            Polyline::new(vec![Point::new(20.0, 20.0), Point::new(30.0, 20.0)]),
        ];
        let result = apply_circular_mask(&polylines, CENTER, RADIUS);
        // First polyline is inside, second is outside.
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 2);
    }

    #[test]
    fn point_on_boundary_is_inside() {
        // A point exactly on the circle should be considered inside.
        let pl = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0), // exactly on boundary
        ]);
        let result = clip_polyline_to_circle(&pl, CENTER, RADIUS);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 2);
    }

    #[test]
    fn single_point_polyline_returns_empty() {
        // A single point can't form a segment (need >= 2 points).
        let pl = Polyline::new(vec![Point::new(0.0, 0.0)]);
        let result = clip_polyline_to_circle(&pl, CENTER, RADIUS);
        assert!(result.is_empty());
    }
}
