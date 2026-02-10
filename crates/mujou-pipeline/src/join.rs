//! Path joining: order and connect disconnected contours into a single
//! continuous path.
//!
//! Sand tables cannot lift the ball -- every movement draws a visible line.
//! The output must be a single continuous path. Each [`PathJoinerKind`]
//! variant receives **unordered** contours and is responsible for both
//! ordering and joining them. This allows strategies like [`Retrace`] to
//! integrate ordering decisions with backtracking capabilities.

use crate::optimize;
use crate::types::Polyline;

/// Selects which path joining strategy to use.
///
/// Each variant receives **unordered** contours and handles its own
/// ordering internally. Additional variants (edge-aware routing, spiral)
/// can be added without changing the `PipelineConfig` struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PathJoinerKind {
    /// Nearest-neighbor ordering followed by straight-line concatenation.
    ///
    /// Internally calls [`optimize::optimize_path_order()`] to reorder
    /// contours by nearest-neighbor, then connects the end of each contour
    /// to the start of the next with a straight line segment.
    ///
    /// Produces visible straight scratches between features, but scratch
    /// length is minimized by the internal path optimization.
    #[default]
    StraightLine,

    /// Full-history retrace with integrated contour ordering.
    ///
    /// Implements a retrace-aware greedy nearest-neighbor algorithm that
    /// considers the **entire drawn path history** when choosing the next
    /// contour. Any previously visited point is reachable at zero visible
    /// cost by retracing backward through already-drawn grooves.
    ///
    /// For each candidate contour (in both forward and reversed
    /// orientations), the algorithm finds the point in the full output
    /// path history closest to the candidate's entry point. The
    /// combination with the smallest distance wins. The algorithm then
    /// retraces backward through the drawn path to that history point
    /// and emits the chosen contour.
    ///
    /// Produces a longer total path but significantly fewer visible
    /// artifacts than `StraightLine`.
    Retrace,
}

/// Trait for path joining strategies.
///
/// Input: **unordered** disconnected contours (from simplification).
/// Output: a single continuous polyline.
///
/// Each implementation is responsible for ordering the contours as part
/// of joining them.
pub trait PathJoiner {
    /// Order and join the given contours into a single continuous path.
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

/// Nearest-neighbor ordering followed by straight-line concatenation.
///
/// Delegates ordering to [`optimize::optimize_path_order()`], then
/// concatenates contours end-to-start. The connecting segments are
/// implicit -- the last point of contour N and the first point of
/// contour N+1 form the straight-line jump.
fn join_straight_line(contours: &[Polyline]) -> Polyline {
    let ordered = optimize::optimize_path_order(contours);

    let total_points: usize = ordered.iter().map(Polyline::len).sum();
    let mut points = Vec::with_capacity(total_points);

    for contour in &ordered {
        points.extend_from_slice(contour.points());
    }

    Polyline::new(points)
}

/// Full-history retrace with integrated contour ordering.
///
/// Algorithm (retrace-aware greedy nearest-neighbor):
///
/// 1. Start with all contours in a candidate pool.
/// 2. Pick contour 0, emit its points into the output path.
/// 3. While candidates remain:
///    a. For each candidate, for each orientation (forward/reversed),
///    find the point in the ENTIRE output path history closest to
///    the candidate's entry point.
///    b. Pick the combination with the smallest distance.
///    c. Retrace backward through the drawn path to the closest
///    history point (invisible in sand).
///    d. Emit the chosen contour's points (reversed if needed).
/// 4. Return the output path.
///
/// Every point in the output path is connected to its neighbors by an
/// already-drawn segment. Retracing backward follows these segments,
/// which are invisible in sand. Any previously visited point is
/// reachable at zero visible cost.
fn join_retrace(contours: &[Polyline]) -> Polyline {
    // Filter out empty contours.
    let candidates: Vec<&Polyline> = contours.iter().filter(|c| !c.is_empty()).collect();

    if candidates.is_empty() {
        return Polyline::new(Vec::new());
    }

    let n = candidates.len();
    let mut used = vec![false; n];
    let mut output: Vec<crate::types::Point> = Vec::new();

    // Reserve a lower bound (all contour points).
    let lower_bound: usize = candidates.iter().map(|c| c.len()).sum();
    output.reserve(lower_bound);

    // Start with candidate 0 (forward).
    used[0] = true;
    output.extend_from_slice(candidates[0].points());

    for _ in 1..n {
        // For each remaining candidate, in both orientations, find the
        // closest point in the entire output history to the candidate's
        // entry point (first point in the chosen orientation).
        let mut best_candidate: Option<usize> = None;
        let mut best_reversed = false;
        let mut best_history_idx: usize = 0;
        let mut best_dist = f64::INFINITY;

        for (j, candidate) in candidates.iter().enumerate() {
            if used[j] {
                continue;
            }

            // Try both orientations. The entry point is the first point
            // of the contour in the chosen orientation.
            // Safety: candidates are pre-filtered to be non-empty, so
            // first()/last() always return Some.
            let c_first = candidate
                .first()
                .copied()
                .unwrap_or(crate::types::Point::new(0.0, 0.0));
            let c_last = candidate.last().copied().unwrap_or(c_first);

            for reversed in [false, true] {
                let entry = if reversed { c_last } else { c_first };

                // Find the closest point in the output history.
                for (h_idx, h_pt) in output.iter().enumerate() {
                    let dist = h_pt.distance_squared(entry);
                    if dist < best_dist {
                        best_dist = dist;
                        best_candidate = Some(j);
                        best_reversed = reversed;
                        best_history_idx = h_idx;
                    }
                }
            }
        }

        // The loop invariant guarantees at least one unvisited candidate,
        // so `best_candidate` is always `Some` here.
        let Some(chosen_idx) = best_candidate else {
            unreachable!("all candidates visited before loop completed");
        };
        used[chosen_idx] = true;

        // Retrace: walk backward from the current end of the output path
        // to the best history point. We emit points from output[end-1]
        // down to output[best_history_idx], exclusive of the current end
        // (which is already the last emitted point) but inclusive of the
        // history point.
        let current_end = output.len() - 1;
        if best_history_idx < current_end {
            // Walk backward: emit output[current_end - 1] down to
            // output[best_history_idx] (inclusive).
            for k in (best_history_idx..current_end).rev() {
                output.push(output[k]);
            }
        }

        // Emit the chosen contour's points.
        if best_reversed {
            for pt in candidates[chosen_idx].points().iter().rev() {
                output.push(*pt);
            }
        } else {
            output.extend_from_slice(candidates[chosen_idx].points());
        }
    }

    Polyline::new(output)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::types::Point;

    // --- StraightLine tests ---

    #[test]
    fn default_is_straight_line() {
        assert_eq!(PathJoinerKind::default(), PathJoinerKind::StraightLine);
    }

    #[test]
    fn straight_line_empty_contours() {
        let result = PathJoinerKind::StraightLine.join(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn straight_line_single_contour() {
        let contour = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(2.0, 0.0),
        ]);
        let result = PathJoinerKind::StraightLine.join(std::slice::from_ref(&contour));
        assert_eq!(result, contour);
    }

    #[test]
    fn straight_line_preserves_total_point_count() {
        // Contours already in nearest-neighbor order along the x-axis,
        // so optimize_path_order should preserve ordering.
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

    #[test]
    fn straight_line_reorders_for_shorter_travel() {
        // Deliberately bad ordering: c0 ends at (1,0), c1 is far away at
        // (50,0), c2 is nearby at (2,0). Internal optimize should visit
        // c2 before c1.
        let c0 = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(1.0, 0.0)]);
        let c1 = Polyline::new(vec![Point::new(50.0, 0.0), Point::new(51.0, 0.0)]);
        let c2 = Polyline::new(vec![Point::new(2.0, 0.0), Point::new(3.0, 0.0)]);

        let result = PathJoinerKind::StraightLine.join(&[c0, c1, c2]);
        let pts = result.points();

        // c0 is first (always), then c2 (nearby), then c1 (far).
        assert_eq!(pts[0], Point::new(0.0, 0.0));
        assert_eq!(pts[1], Point::new(1.0, 0.0));
        assert_eq!(pts[2], Point::new(2.0, 0.0));
        assert_eq!(pts[3], Point::new(3.0, 0.0));
        assert_eq!(pts[4], Point::new(50.0, 0.0));
        assert_eq!(pts[5], Point::new(51.0, 0.0));
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
    fn retrace_filters_empty_contours() {
        let c1 = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(1.0, 0.0)]);
        let empty = Polyline::new(vec![]);
        let c2 = Polyline::new(vec![Point::new(2.0, 0.0)]);

        let result = PathJoinerKind::Retrace.join(&[c1, empty, c2]);
        // Should contain points from c1 and c2, skipping the empty contour.
        assert!(result.len() >= 3);
    }

    #[test]
    fn retrace_backtracks_to_closest_history_point() {
        // c1: (0,0) -> (10,0) -> (20,0)
        // c2 starts at (11,1), closest history point is (10,0) at index 1
        //
        // Expected output:
        //   Forward c1:  (0,0), (10,0), (20,0)
        //   Backtrack:   (10,0)  -- retrace from index 2 back to index 1
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
    fn retrace_no_backtrack_when_end_is_closest() {
        // c1: (0,0) -> (1,0) -> (5,0), c2 starts at (6,0).
        // End of c1 (5,0) is already closest to c2, so no backtrack.
        let c1 = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(5.0, 0.0),
        ]);
        let c2 = Polyline::new(vec![Point::new(6.0, 0.0)]);

        let result = PathJoinerKind::Retrace.join(&[c1, c2]);
        // 3 (c1) + 0 (no backtrack) + 1 (c2) = 4
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn retrace_picks_optimal_backtrack_across_zigzag() {
        // c1 zigzags: (0,0) -> (10,10) -> (20,0) -> (30,10)
        // c2 starts at (21,1), closest to (20,0) at history index 2.
        let c1 = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(20.0, 0.0),
            Point::new(30.0, 10.0),
        ]);
        let c2 = Polyline::new(vec![Point::new(21.0, 1.0)]);

        let result = PathJoinerKind::Retrace.join(&[c1, c2]);
        let pts = result.points();

        // 4 (c1) + 1 (backtrack to index 2: (20,0)) + 1 (c2) = 6
        assert_eq!(pts.len(), 6);
        assert_eq!(pts[4], Point::new(20.0, 0.0));
        assert_eq!(pts[5], Point::new(21.0, 1.0));
    }

    #[test]
    fn retrace_full_history_across_earlier_contours() {
        // The key feature: backtracking reaches points from earlier contours.
        //
        // c1: (0,0) -> (10,0)
        // c2: (20,0) -> (30,0)   (chosen second because it's nearest to c1's end)
        // c3: (1,1)              (closest history point is (0,0) from c1!)
        //
        // Without full-history, the old algorithm could only backtrack along c2.
        // With full-history, it can retrace all the way back to c1's (0,0).
        let c1 = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0)]);
        let c2 = Polyline::new(vec![Point::new(20.0, 0.0), Point::new(30.0, 0.0)]);
        let c3 = Polyline::new(vec![Point::new(1.0, 1.0)]);

        let result = PathJoinerKind::Retrace.join(&[c1, c2, c3]);
        let pts = result.points();

        // The output should contain (0,0) from c1 as a backtrack target for c3.
        // After emitting c1 and c2, the algorithm backtracks through the entire
        // history to reach (0,0) (closest to c3's (1,1)).
        //
        // Expected sequence:
        //   c1 forward: (0,0), (10,0)
        //   no backtrack for c2 (end of c1 at (10,0) is closest to c2's start (20,0))
        //   c2 forward: (20,0), (30,0)
        //   backtrack for c3: (20,0), (10,0), (0,0) -- walk back through entire history
        //   c3 forward: (1,1)

        // Verify (0,0) appears in the backtrack sequence before c3's point
        let c3_point_idx = pts
            .iter()
            .rposition(|p| *p == Point::new(1.0, 1.0))
            .unwrap();

        // The point just before c3 should be (0,0) -- the closest history point.
        assert_eq!(
            pts[c3_point_idx - 1],
            Point::new(0.0, 0.0),
            "should have retraced back to (0,0) from c1 before emitting c3"
        );
    }

    #[test]
    fn retrace_considers_reversed_orientation() {
        // c1: (0,0) -> (10,0)
        // c2: (5,0) -> (15,0)  -- c2's end (15,0) is farther from history,
        //   but c2's start (5,0) is closer. However, the entry point that
        //   minimizes distance to a history point should be used.
        //   Forward entry (5,0) is closest to history (10,0) at dist 5.
        //   Reversed entry (15,0) is closest to history (10,0) at dist 5.
        //   Tie goes to forward (iterated first).
        //
        // Better test: c2 end is very close to a history point.
        // c1: (0,0) -> (10,0)
        // c2: (50,0) -> (11,0) -- reversed entry (11,0) is much closer to
        //   history (10,0) than forward entry (50,0).
        let c1 = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0)]);
        let c2 = Polyline::new(vec![Point::new(50.0, 0.0), Point::new(11.0, 0.0)]);

        let result = PathJoinerKind::Retrace.join(&[c1, c2]);
        let pts = result.points();

        // c2 should be emitted reversed: (11,0) then (50,0).
        // c1 forward: (0,0), (10,0)
        // no backtrack needed (10,0) is closest to reversed c2 entry (11,0)
        // c2 reversed: (11,0), (50,0)
        assert_eq!(pts.len(), 4);
        assert_eq!(pts[0], Point::new(0.0, 0.0));
        assert_eq!(pts[1], Point::new(10.0, 0.0));
        assert_eq!(pts[2], Point::new(11.0, 0.0));
        assert_eq!(pts[3], Point::new(50.0, 0.0));
    }

    #[test]
    fn retrace_integrated_ordering_picks_nearest_candidate() {
        // Contours given in bad order: c0 at origin, c1 far away, c2 nearby.
        // The integrated ordering should pick c2 before c1.
        let c0 = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(1.0, 0.0)]);
        let c1 = Polyline::new(vec![Point::new(100.0, 0.0), Point::new(101.0, 0.0)]);
        let c2 = Polyline::new(vec![Point::new(2.0, 0.0), Point::new(3.0, 0.0)]);

        let result = PathJoinerKind::Retrace.join(&[c0, c1, c2]);
        let pts = result.points();

        // c0 emitted first, then c2 (nearby), then c1 (far).
        // c0: (0,0), (1,0)
        // c2: (2,0), (3,0) -- no backtrack, end of c0 is closest
        // c1: (100,0), (101,0) -- no backtrack, end of c2 is closest candidate
        assert_eq!(pts[0], Point::new(0.0, 0.0));
        assert_eq!(pts[1], Point::new(1.0, 0.0));
        assert_eq!(pts[2], Point::new(2.0, 0.0));
        assert_eq!(pts[3], Point::new(3.0, 0.0));
        // c1 follows after c2
        assert!(pts.iter().any(|p| *p == Point::new(100.0, 0.0)));
        assert!(pts.iter().any(|p| *p == Point::new(101.0, 0.0)));
    }

    #[test]
    fn retrace_has_at_least_as_many_points_as_contour_total() {
        // Retrace may add backtrack points, so the output should have at
        // least as many points as the sum of all contour points.
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

        let total_contour_points: usize = contours.iter().map(Polyline::len).sum();
        let result = PathJoinerKind::Retrace.join(&contours);

        assert!(
            result.len() >= total_contour_points,
            "retrace output ({}) should be >= sum of contour points ({total_contour_points})",
            result.len(),
        );
    }

    /// Diagnostic test: measure algorithm behavior on realistic synthetic data.
    ///
    /// Run with `cargo test -p mujou-pipeline retrace_diagnostic -- --nocapture`
    /// to see timing and output growth stats.
    #[test]
    fn retrace_diagnostic_reports_stats() {
        use std::time::Instant;

        // Simulate ~200 contours with ~50 points each, scattered randomly-ish
        // across a 1000x1000 canvas.  Use a deterministic pseudo-random
        // pattern (no rand dependency).
        let n_contours = 200;
        let pts_per_contour = 50;

        let contours: Vec<Polyline> = (0..n_contours)
            .map(|i| {
                // Spread contour centers across the canvas using a simple hash.
                let cx = f64::from((i * 137 + 17) % 1000);
                let cy = f64::from((i * 251 + 43) % 1000);
                let points: Vec<Point> = (0..pts_per_contour)
                    .map(|j| {
                        let angle =
                            std::f64::consts::TAU * f64::from(j) / f64::from(pts_per_contour);
                        let r = f64::from(j % 5).mul_add(2.0, 10.0);
                        Point::new(r.mul_add(angle.cos(), cx), r.mul_add(angle.sin(), cy))
                    })
                    .collect();
                Polyline::new(points)
            })
            .collect();

        let total_input_points: usize = contours.iter().map(Polyline::len).sum();

        let start = Instant::now();
        let result = PathJoinerKind::Retrace.join(&contours);
        let elapsed = start.elapsed();

        let output_points = result.len();
        let backtrack_points = output_points - total_input_points;
        #[allow(clippy::cast_precision_loss)]
        let expansion_ratio = output_points as f64 / total_input_points as f64;

        eprintln!("--- retrace diagnostic ---");
        eprintln!("  contours:         {n_contours}");
        eprintln!("  pts/contour:      {pts_per_contour}");
        eprintln!("  total input pts:  {total_input_points}");
        eprintln!("  output pts:       {output_points}");
        eprintln!("  backtrack pts:    {backtrack_points}");
        eprintln!("  expansion ratio:  {expansion_ratio:.2}x");
        eprintln!("  elapsed:          {elapsed:?}");
        eprintln!("--------------------------");

        // Sanity: output should be at least input size.
        assert!(output_points >= total_input_points);
    }
}
