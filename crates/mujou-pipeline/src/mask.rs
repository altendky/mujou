//! Mask clipping: clip polylines to a shape boundary.
//!
//! For round sand tables (Sisyphus, Oasis Mini), all output paths must
//! fit within a circular boundary. Points outside the circle are removed,
//! and line segments that cross the boundary are split at the intersection
//! point on the circle.
//!
//! This is step 7 in the pipeline (optional), applied before path joining.

use std::f64::consts::PI;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::types::{Point, Polyline};

// ──────────────────────────── Public types ────────────────────────────

/// Resolved mask geometry used for both clipping and border generation.
///
/// Adding a new shape variant requires implementing both clipping (in
/// [`apply_mask`]) and border generation (in [`MaskShape::border_polyline`]),
/// enforced by exhaustive `match` arms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MaskShape {
    /// Circular mask centred on a point with a given radius.
    Circle {
        /// Centre of the circle in image coordinates.
        center: Point,
        /// Radius of the circle in pixels.
        radius: f64,
    },
}

impl MaskShape {
    /// Generate a border polyline matching this mask shape.
    ///
    /// The border follows the mask boundary and is intended to be included
    /// in the polyline set passed to the joiner, so that connections between
    /// boundary endpoints route along the edge rather than crossing open
    /// space.
    #[must_use]
    pub fn border_polyline(&self) -> Polyline {
        match self {
            Self::Circle { center, radius } => generate_circle_border(*center, *radius),
        }
    }
}

/// A polyline produced by mask clipping, with explicit metadata about
/// which endpoints were created by intersection with the mask boundary.
///
/// Clip-derived points can only appear at the first or last position of
/// an output polyline (never in the interior), so two booleans fully
/// identify every point that resulted from clipping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClippedPolyline {
    /// The clipped polyline geometry.
    pub polyline: Polyline,
    /// Whether the first point was created by intersection with the mask
    /// boundary.
    pub start_clipped: bool,
    /// Whether the last point was created by intersection with the mask
    /// boundary.
    pub end_clipped: bool,
}

/// Complete output of the mask stage, including clip metadata and
/// optional border.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaskResult {
    /// Polylines clipped to the mask shape, with per-endpoint clip
    /// metadata.
    pub clipped: Vec<ClippedPolyline>,
    /// Optional border polyline matching the mask shape.
    ///
    /// Present when [`BorderPathMode`] resolves to enabled (either `On`,
    /// or `Auto` with at least one clipped endpoint).
    pub border: Option<Polyline>,
}

impl MaskResult {
    /// Iterator over all polylines for rendering or joining.
    ///
    /// Yields the clipped polylines followed by the border polyline (if
    /// present).
    pub fn all_polylines(&self) -> impl Iterator<Item = &Polyline> {
        self.clipped
            .iter()
            .map(|c| &c.polyline)
            .chain(self.border.iter())
    }

    /// Whether any polyline endpoint was created by intersection with
    /// the mask boundary.
    #[must_use]
    pub fn any_clipped(&self) -> bool {
        self.clipped
            .iter()
            .any(|c| c.start_clipped || c.end_clipped)
    }
}

/// Controls whether a border polyline matching the mask shape is added
/// to the output.
///
/// The border polyline lets the joiner route connections along the mask
/// boundary rather than across open space, reducing visible artifacts
/// near the edge.
///
/// Only takes effect when a mask is enabled (`circular_mask = true`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BorderPathMode {
    /// Never add a border path.
    Off,
    /// Add a border path only when the mask clips at least one polyline
    /// endpoint.
    #[default]
    Auto,
    /// Always add a border path when a mask is enabled.
    On,
}

impl fmt::Display for BorderPathMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Off => f.write_str("Off"),
            Self::Auto => f.write_str("Auto"),
            Self::On => f.write_str("On"),
        }
    }
}

// ──────────────────────────── Public API ──────────────────────────────

/// Clip multiple polylines to a mask shape.
///
/// Each input polyline is independently clipped. A single input polyline
/// may produce multiple output polylines if it exits and re-enters the
/// mask boundary.
///
/// The returned [`ClippedPolyline`] values carry explicit metadata about
/// which endpoints were created by intersection with the boundary.
#[must_use = "returns the clipped polylines with clip metadata"]
pub fn apply_mask(polylines: &[Polyline], shape: &MaskShape) -> Vec<ClippedPolyline> {
    match shape {
        MaskShape::Circle { center, radius } => polylines
            .iter()
            .flat_map(|pl| clip_polyline_to_circle(pl, *center, *radius))
            .collect(),
    }
}

// ──────────────────── Circle clipping (internal) ─────────────────────

/// Arc-length spacing between consecutive points on a border polyline
/// (pixels).
const BORDER_POINT_SPACING: f64 = 3.0;

/// Minimum number of points on a border circle, even for very small
/// radii.
const MIN_BORDER_POINTS: usize = 8;

/// Clip a single polyline to a circle, splitting at boundary crossings.
///
/// Segments that cross the circle boundary are split at the intersection
/// point. The result may be multiple polylines if the original path
/// exits and re-enters the circle.
///
/// Points exactly on the boundary (within floating-point tolerance) are
/// considered inside.
fn clip_polyline_to_circle(
    polyline: &Polyline,
    center: Point,
    radius: f64,
) -> Vec<ClippedPolyline> {
    let points = polyline.points();
    if points.is_empty() {
        return Vec::new();
    }

    let radius_sq = radius * radius;
    let mut result = Vec::new();
    let mut current_segment: Vec<Point> = Vec::new();
    let mut current_start_clipped = false;

    for i in 0..points.len() {
        let p = points[i];
        let p_inside = is_inside(p, center, radius_sq);

        if i == 0 {
            if p_inside {
                current_segment.push(p);
                current_start_clipped = false;
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
                // Explicit `if let` preferred over `is_some_and`/`map_or`
                // because the closure mutates `current_segment`.
                #[allow(clippy::option_if_let_else)]
                let end_clipped =
                    if let Some(ix) = line_circle_intersection(prev, p, center, radius) {
                        current_segment.push(ix);
                        true
                    } else {
                        false
                    };
                if current_segment.len() >= 2 {
                    result.push(ClippedPolyline {
                        polyline: Polyline::new(std::mem::take(&mut current_segment)),
                        start_clipped: current_start_clipped,
                        end_clipped,
                    });
                } else {
                    current_segment.clear();
                }
            }
            (false, true) => {
                // Entering the circle: start new segment from intersection.
                if let Some(ix) = line_circle_intersection(prev, p, center, radius) {
                    current_start_clipped = true;
                    current_segment.push(ix);
                } else {
                    current_start_clipped = false;
                }
                current_segment.push(p);
            }
            (false, false) => {
                // Both outside: check if the segment passes through the
                // circle.
                let intersections = line_circle_both_intersections(prev, p, center, radius);
                if let Some((ix1, ix2)) = intersections {
                    // Flush any residual segment (defensive — should be
                    // empty here).
                    if current_segment.len() >= 2 {
                        result.push(ClippedPolyline {
                            polyline: Polyline::new(std::mem::take(&mut current_segment)),
                            start_clipped: current_start_clipped,
                            end_clipped: false,
                        });
                    } else {
                        current_segment.clear();
                    }
                    // The pass-through segment: both endpoints are
                    // boundary intersections.
                    result.push(ClippedPolyline {
                        polyline: Polyline::new(vec![ix1, ix2]),
                        start_clipped: true,
                        end_clipped: true,
                    });
                }
            }
        }
    }

    // Flush any remaining segment — the polyline ended while inside the
    // circle, so the final endpoint is NOT a boundary intersection.
    if current_segment.len() >= 2 {
        result.push(ClippedPolyline {
            polyline: Polyline::new(current_segment),
            start_clipped: current_start_clipped,
            end_clipped: false,
        });
    }

    result
}

/// Generate a closed circle border polyline.
///
/// Points are evenly spaced by arc length at approximately
/// [`BORDER_POINT_SPACING`] pixels. The polyline is closed (last point
/// equals the first).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn generate_circle_border(center: Point, radius: f64) -> Polyline {
    let circumference = 2.0 * PI * radius;
    let n = (circumference / BORDER_POINT_SPACING)
        .ceil()
        .max(MIN_BORDER_POINTS as f64) as usize;
    let mut points = Vec::with_capacity(n + 1);
    for i in 0..n {
        let angle = 2.0 * PI * (i as f64) / (n as f64);
        points.push(Point::new(
            radius.mul_add(angle.cos(), center.x),
            radius.mul_add(angle.sin(), center.y),
        ));
    }
    // Close the loop: last point equals the first.
    points.push(points[0]);
    Polyline::new(points)
}

// ──────────────────── Geometry helpers (internal) ─────────────────────

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
/// Returns `None` if the line misses the circle (negative discriminant)
/// or if the segment is degenerate (zero length, i.e. `a == b`).
fn solve_line_circle(a: Point, b: Point, center: Point, radius: f64) -> Option<(f64, f64)> {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let fx = a.x - center.x;
    let fy = a.y - center.y;

    let a_coeff = dx.mul_add(dx, dy * dy);
    if a_coeff == 0.0 {
        // Degenerate segment (a == b): no meaningful intersection.
        return None;
    }
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
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const CENTER: Point = Point::new(0.0, 0.0);
    const RADIUS: f64 = 10.0;

    // ── clip_polyline_to_circle ──────────────────────────────────────

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
        assert_eq!(result[0].polyline.len(), 3);
        assert_eq!(result[0].polyline, pl);
        assert!(!result[0].start_clipped);
        assert!(!result[0].end_clipped);
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
        // Path starts inside (0,0) and goes to (20,0), exiting at ~(10,0).
        let pl = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(20.0, 0.0)]);
        let result = clip_polyline_to_circle(&pl, CENTER, RADIUS);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].polyline.len(), 2);
        // First point unchanged.
        assert_eq!(result[0].polyline.points()[0], Point::new(0.0, 0.0));
        // Second point should be on the circle boundary at ~(10, 0).
        let clipped = result[0].polyline.points()[1];
        assert!(
            (clipped.distance(CENTER) - RADIUS).abs() < 1e-6,
            "clipped point should be on circle boundary, distance = {}",
            clipped.distance(CENTER),
        );
        // Clip metadata: start is original, end is clipped.
        assert!(!result[0].start_clipped);
        assert!(result[0].end_clipped);
    }

    #[test]
    fn segment_entering_circle_is_clipped() {
        // Path starts outside (-20, 0) and goes to (0, 0), entering at ~(-10, 0).
        let pl = Polyline::new(vec![Point::new(-20.0, 0.0), Point::new(0.0, 0.0)]);
        let result = clip_polyline_to_circle(&pl, CENTER, RADIUS);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].polyline.len(), 2);
        // First point should be on circle boundary at ~(-10, 0).
        let entry = result[0].polyline.points()[0];
        assert!(
            (entry.distance(CENTER) - RADIUS).abs() < 1e-6,
            "entry point should be on circle boundary, distance = {}",
            entry.distance(CENTER),
        );
        // Last point is origin.
        assert_eq!(result[0].polyline.points()[1], Point::new(0.0, 0.0));
        // Clip metadata: start is clipped, end is original.
        assert!(result[0].start_clipped);
        assert!(!result[0].end_clipped);
    }

    #[test]
    fn path_crossing_through_produces_clipped_segment() {
        // Path goes from (-20, 0) to (20, 0), crossing through the circle.
        // Should produce a single segment from ~(-10, 0) to ~(10, 0).
        let pl = Polyline::new(vec![Point::new(-20.0, 0.0), Point::new(20.0, 0.0)]);
        let result = clip_polyline_to_circle(&pl, CENTER, RADIUS);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].polyline.len(), 2);

        for p in result[0].polyline.points() {
            assert!(
                (p.distance(CENTER) - RADIUS).abs() < 1e-6,
                "intersection point should be on circle, distance = {}",
                p.distance(CENTER),
            );
        }
        // Both endpoints are boundary intersections.
        assert!(result[0].start_clipped);
        assert!(result[0].end_clipped);
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
        // First segment: starts inside, exits at boundary.
        assert!(!result[0].start_clipped);
        assert!(result[0].end_clipped);
        // Second segment: enters at boundary, ends inside.
        assert!(result[1].start_clipped);
        assert!(!result[1].end_clipped);
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
        assert_eq!(result[0].polyline.len(), 2);
        assert!(!result[0].start_clipped);
        assert!(!result[0].end_clipped);
    }

    #[test]
    fn single_point_polyline_returns_empty() {
        // A single point can't form a segment (need >= 2 points).
        let pl = Polyline::new(vec![Point::new(0.0, 0.0)]);
        let result = clip_polyline_to_circle(&pl, CENTER, RADIUS);
        assert!(result.is_empty());
    }

    // ── apply_mask ───────────────────────────────────────────────────

    #[test]
    fn apply_mask_processes_multiple_polylines() {
        let polylines = vec![
            Polyline::new(vec![Point::new(0.0, 0.0), Point::new(3.0, 0.0)]),
            Polyline::new(vec![Point::new(20.0, 20.0), Point::new(30.0, 20.0)]),
        ];
        let shape = MaskShape::Circle {
            center: CENTER,
            radius: RADIUS,
        };
        let result = apply_mask(&polylines, &shape);
        // First polyline is inside, second is outside.
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].polyline.len(), 2);
        assert!(!result[0].start_clipped);
        assert!(!result[0].end_clipped);
    }

    // ── MaskResult ───────────────────────────────────────────────────

    #[test]
    fn mask_result_any_clipped_true_when_clipping_occurred() {
        let mr = MaskResult {
            clipped: vec![ClippedPolyline {
                polyline: Polyline::new(vec![Point::new(0.0, 0.0), Point::new(5.0, 0.0)]),
                start_clipped: true,
                end_clipped: false,
            }],
            border: None,
        };
        assert!(mr.any_clipped());
    }

    #[test]
    fn mask_result_any_clipped_false_when_no_clipping() {
        let mr = MaskResult {
            clipped: vec![ClippedPolyline {
                polyline: Polyline::new(vec![Point::new(0.0, 0.0), Point::new(5.0, 0.0)]),
                start_clipped: false,
                end_clipped: false,
            }],
            border: None,
        };
        assert!(!mr.any_clipped());
    }

    #[test]
    fn mask_result_all_polylines_includes_border() {
        let clipped_pl = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(5.0, 0.0)]);
        let border_pl = Polyline::new(vec![Point::new(10.0, 0.0), Point::new(0.0, 10.0)]);
        let mr = MaskResult {
            clipped: vec![ClippedPolyline {
                polyline: clipped_pl.clone(),
                start_clipped: false,
                end_clipped: false,
            }],
            border: Some(border_pl.clone()),
        };
        let all: Vec<&Polyline> = mr.all_polylines().collect();
        assert_eq!(all.len(), 2);
        assert_eq!(*all[0], clipped_pl);
        assert_eq!(*all[1], border_pl);
    }

    #[test]
    fn mask_result_all_polylines_without_border() {
        let clipped_pl = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(5.0, 0.0)]);
        let mr = MaskResult {
            clipped: vec![ClippedPolyline {
                polyline: clipped_pl.clone(),
                start_clipped: false,
                end_clipped: false,
            }],
            border: None,
        };
        let all: Vec<&Polyline> = mr.all_polylines().collect();
        assert_eq!(all.len(), 1);
        assert_eq!(*all[0], clipped_pl);
    }

    // ── generate_circle_border ───────────────────────────────────────

    #[test]
    fn circle_border_points_are_on_circle() {
        let border = generate_circle_border(CENTER, RADIUS);
        assert!(
            border.len() > MIN_BORDER_POINTS,
            "border should have more than {} points (including closing point), got {}",
            MIN_BORDER_POINTS,
            border.len(),
        );
        for p in border.points() {
            assert!(
                (p.distance(CENTER) - RADIUS).abs() < 1e-10,
                "border point should be on circle, distance = {}",
                p.distance(CENTER),
            );
        }
    }

    #[test]
    fn circle_border_is_closed() {
        let border = generate_circle_border(CENTER, RADIUS);
        let first = border.first().expect("border should not be empty");
        let last = border.last().expect("border should not be empty");
        assert_eq!(first, last, "border should be closed (first == last)");
    }

    #[test]
    fn circle_border_point_count_scales_with_radius() {
        let small = generate_circle_border(CENTER, 5.0);
        let large = generate_circle_border(CENTER, 500.0);
        assert!(
            large.len() > small.len(),
            "larger circle should have more points: {} vs {}",
            large.len(),
            small.len(),
        );
    }

    #[test]
    fn mask_shape_border_polyline_dispatches_to_circle() {
        let shape = MaskShape::Circle {
            center: CENTER,
            radius: RADIUS,
        };
        let border = shape.border_polyline();
        // Should be a valid closed circle.
        let first = border.first().expect("border should not be empty");
        let last = border.last().expect("border should not be empty");
        assert_eq!(first, last);
        for p in border.points() {
            assert!(
                (p.distance(CENTER) - RADIUS).abs() < 1e-10,
                "border point should be on circle",
            );
        }
    }

    // ── BorderPathMode ───────────────────────────────────────────────

    #[test]
    fn border_path_mode_default_is_auto() {
        assert_eq!(BorderPathMode::default(), BorderPathMode::Auto);
    }

    #[test]
    fn border_path_mode_display() {
        assert_eq!(BorderPathMode::Off.to_string(), "Off");
        assert_eq!(BorderPathMode::Auto.to_string(), "Auto");
        assert_eq!(BorderPathMode::On.to_string(), "On");
    }
}
