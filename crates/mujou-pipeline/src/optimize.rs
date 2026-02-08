//! Path optimization: reorder and orient contours to minimize travel distance.
//!
//! Uses a nearest-neighbor greedy heuristic on contour endpoints.
//! For each pair of contours, the minimum of four endpoint distances
//! (start-start, start-end, end-start, end-end) determines proximity.
//! Each contour may be reversed to minimize the gap from the previous
//! contour's endpoint.
//!
//! This is step 7 in the pipeline, between path simplification and path
//! joining.

use crate::types::{Point, Polyline};

/// Reorder and orient contours to minimize total travel distance.
///
/// Starting from the first contour, greedily visits the nearest unvisited
/// contour, reversing its direction if that shortens the gap from the
/// previous contour's endpoint.
///
/// Returns the reordered (and possibly reversed) contours. Empty contours
/// are filtered out. If all contours are empty, returns an empty vec.
#[must_use = "returns the optimized contour ordering"]
pub fn optimize_path_order(contours: &[Polyline]) -> Vec<Polyline> {
    // Filter out empty contours and collect into owned working set.
    let candidates: Vec<&Polyline> = contours.iter().filter(|c| !c.is_empty()).collect();

    if candidates.is_empty() {
        return Vec::new();
    }

    let n = candidates.len();
    let mut visited = vec![false; n];
    let mut result = Vec::with_capacity(n);

    // Start with the first contour (no reordering of the start).
    visited[0] = true;
    result.push(candidates[0].clone());

    for _ in 1..n {
        let current_end = result
            .last()
            .and_then(Polyline::last)
            .copied()
            .unwrap_or(Point::new(0.0, 0.0));

        let mut best: Option<(usize, bool)> = None;
        let mut best_dist = f64::INFINITY;

        for (j, candidate) in candidates.iter().enumerate() {
            if visited[j] {
                continue;
            }

            // unwrap_or is safe here because we filtered empty contours.
            let c_start = candidate.first().copied().unwrap_or(Point::new(0.0, 0.0));
            let c_end = candidate.last().copied().unwrap_or(Point::new(0.0, 0.0));

            let dist_forward = current_end.distance_squared(c_start);
            let dist_reverse = current_end.distance_squared(c_end);

            let (dist, reversed) = if dist_forward <= dist_reverse {
                (dist_forward, false)
            } else {
                (dist_reverse, true)
            };

            if dist < best_dist {
                best_dist = dist;
                best = Some((j, reversed));
            }
        }

        // The loop invariant guarantees at least one unvisited candidate,
        // so `best` is always `Some` here. Use `continue` to satisfy the
        // type system without panicking.
        let Some((best_idx, best_reversed)) = best else {
            continue;
        };

        visited[best_idx] = true;

        if best_reversed {
            let mut points = candidates[best_idx].clone().into_points();
            points.reverse();
            result.push(Polyline::new(points));
        } else {
            result.push(candidates[best_idx].clone());
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_empty() {
        let result = optimize_path_order(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn single_contour_returned_unchanged() {
        let contour = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(2.0, 0.0),
        ]);
        let result = optimize_path_order(std::slice::from_ref(&contour));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], contour);
    }

    #[test]
    fn empty_contours_filtered_out() {
        let contours = vec![
            Polyline::new(vec![]),
            Polyline::new(vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)]),
            Polyline::new(vec![]),
        ];
        let result = optimize_path_order(&contours);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn nearer_contour_visited_first() {
        // C0 ends at (0,0). C1 starts at (100, 100). C2 starts at (1, 1).
        // Greedy NN should visit C2 before C1.
        let c0 = Polyline::new(vec![Point::new(0.0, 0.0)]);
        let c1 = Polyline::new(vec![Point::new(100.0, 100.0), Point::new(101.0, 100.0)]);
        let c2 = Polyline::new(vec![Point::new(1.0, 1.0), Point::new(2.0, 1.0)]);

        let result = optimize_path_order(&[c0.clone(), c1.clone(), c2.clone()]);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], c0);
        assert_eq!(result[1], c2);
        assert_eq!(result[2], c1);
    }

    #[test]
    fn contour_reversed_to_minimize_gap() {
        // C0 ends at (10, 0). C1 goes from (100, 0) to (11, 0).
        // The end of C1 (11, 0) is closer to C0's end (10, 0) than
        // C1's start (100, 0), so C1 should be reversed.
        let c0 = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0)]);
        let c1 = Polyline::new(vec![Point::new(100.0, 0.0), Point::new(11.0, 0.0)]);

        let result = optimize_path_order(&[c0, c1]);
        assert_eq!(result.len(), 2);
        // C1 should be reversed: now starts at (11, 0), ends at (100, 0).
        assert_eq!(result[1].first(), Some(&Point::new(11.0, 0.0)));
        assert_eq!(result[1].last(), Some(&Point::new(100.0, 0.0)));
    }

    #[test]
    fn preserves_all_contours() {
        let contours: Vec<Polyline> = (0..10)
            .map(|i| {
                let x = f64::from(i) * 10.0;
                Polyline::new(vec![Point::new(x, 0.0), Point::new(x + 1.0, 0.0)])
            })
            .collect();

        let result = optimize_path_order(&contours);
        assert_eq!(result.len(), 10);
    }

    #[test]
    fn total_travel_reduced_vs_original_order() {
        // Deliberately bad ordering: contours are far apart in sequence
        // but close when reordered.
        let c0 = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(1.0, 0.0)]);
        let c1 = Polyline::new(vec![Point::new(50.0, 0.0), Point::new(51.0, 0.0)]);
        let c2 = Polyline::new(vec![Point::new(2.0, 0.0), Point::new(3.0, 0.0)]);
        let c3 = Polyline::new(vec![Point::new(52.0, 0.0), Point::new(53.0, 0.0)]);

        let original = [c0, c1, c2, c3];
        let optimized = optimize_path_order(&original);

        let travel_original = total_travel(&original);
        let travel_optimized = total_travel(&optimized);

        assert!(
            travel_optimized <= travel_original,
            "expected optimized travel ({travel_optimized}) <= original ({travel_original})",
        );
    }

    /// Helper: compute total travel distance between consecutive contour endpoints.
    fn total_travel(contours: &[Polyline]) -> f64 {
        contours
            .windows(2)
            .map(|pair| {
                let end = pair[0].last().copied().unwrap_or(Point::new(0.0, 0.0));
                let start = pair[1].first().copied().unwrap_or(Point::new(0.0, 0.0));
                end.distance(start)
            })
            .sum()
    }
}
