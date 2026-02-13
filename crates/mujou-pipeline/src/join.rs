//! Path joining: order and connect disconnected contours into a single
//! continuous path.
//!
//! Sand tables cannot lift the ball -- every movement draws a visible line.
//! The output must be a single continuous path. Each [`PathJoinerKind`]
//! variant receives **unordered** contours and is responsible for both
//! ordering and joining them. This allows strategies like [`Retrace`] to
//! integrate ordering decisions with backtracking capabilities.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::mst_join;
use crate::optimize;
use crate::types::{PipelineConfig, Point, Polyline, polyline_bounding_box};

/// Selects which path joining strategy to use.
///
/// Each variant receives **unordered** contours and handles its own
/// ordering internally. Additional variants (edge-aware routing, spiral)
/// can be added without changing the `PipelineConfig` struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PathJoinerKind {
    /// Nearest-neighbor ordering followed by straight-line concatenation.
    ///
    /// Internally calls [`optimize::optimize_path_order()`] to reorder
    /// contours by nearest-neighbor, then connects the end of each contour
    /// to the start of the next with a straight line segment.
    ///
    /// Produces visible straight scratches between features, but scratch
    /// length is minimized by the internal path optimization.
    StraightLine,

    /// Full-history retrace with integrated contour ordering.
    ///
    /// Implements a retrace-aware greedy nearest-neighbor algorithm that
    /// considers the **entire drawn path history** when choosing the next
    /// contour. A spatial grid index provides O(1)-amortized nearest
    /// lookups, and candidate contours are sampled at arc-length spacing
    /// to find the best entry point (including interior points, not just
    /// endpoints).
    ///
    /// When the best entry is at an interior point, the contour is
    /// traversed via a split: forward to one end, retrace back to the
    /// entry, then backward to the other end — covering the full contour.
    ///
    /// Any previously visited point is reachable at zero visible cost
    /// by retracing backward through already-drawn grooves. Produces a
    /// longer total path but significantly fewer visible artifacts than
    /// `StraightLine`.
    Retrace,

    /// MST-based segment-to-segment join algorithm.
    ///
    /// Builds a minimum spanning tree over polyline components using
    /// segment-to-segment distances (via an R\*-tree spatial index),
    /// finding globally optimal connections that minimize total new line
    /// length. Interior joins are supported — connecting at any point
    /// along a polyline, not just endpoints.
    ///
    /// After MST construction, vertex parity is fixed by duplicating
    /// shortest paths (retracing is visually free), then Hierholzer's
    /// algorithm finds an Eulerian path through the augmented graph.
    ///
    /// Produces significantly fewer visible artifacts and shorter new
    /// connecting segments than both `StraightLine` and `Retrace`.
    #[default]
    Mst,
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
    fn join(&self, contours: &[Polyline], config: &PipelineConfig) -> Polyline;
}

impl fmt::Display for PathJoinerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StraightLine => f.write_str("StraightLine"),
            Self::Retrace => f.write_str("Retrace"),
            Self::Mst => f.write_str("Mst"),
        }
    }
}

impl PathJoiner for PathJoinerKind {
    fn join(&self, contours: &[Polyline], config: &PipelineConfig) -> Polyline {
        match *self {
            Self::StraightLine => join_straight_line(contours),
            Self::Retrace => join_retrace(contours),
            Self::Mst => mst_join::join_mst(contours, config.mst_neighbours),
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

// ---------------------------------------------------------------------------
// Spatial grid for O(1)-amortized nearest-neighbor queries on output history
// ---------------------------------------------------------------------------

/// Flat-array 2D spatial grid for fast nearest-neighbor lookups.
///
/// Points are bucketed into square cells on a fixed grid derived from the
/// input bounding box. Nearest-neighbor queries search expanding rings of
/// cells, terminating early once no closer point is possible.
///
/// Uses a flat `Vec` indexed by `(row * cols + col)` for cache-friendly
/// access — much faster than `HashMap` in hot loops.
struct SpatialGrid {
    cell_size: f64,
    inv_cell_size: f64,
    /// Grid origin (minimum x, minimum y) in world coordinates.
    origin_x: f64,
    origin_y: f64,
    /// Grid dimensions in cells.
    cols: usize,
    rows: usize,
    /// Flat array of cells. Each cell stores (`output_index`, point) pairs.
    cells: Vec<Vec<(usize, Point)>>,
    count: usize,
}

impl SpatialGrid {
    /// Create a grid covering the given bounding box with the given cell
    /// size. Adds a 1-cell margin on each side so points near the edge
    /// don't need special boundary handling.
    fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64, cell_size: f64) -> Self {
        let margin = cell_size;
        let ox = min_x - margin;
        let oy = min_y - margin;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let cols = ((max_x - ox + margin) / cell_size).ceil() as usize + 1;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let rows = ((max_y - oy + margin) / cell_size).ceil() as usize + 1;

        Self {
            cell_size,
            inv_cell_size: cell_size.recip(),
            origin_x: ox,
            origin_y: oy,
            cols,
            rows,
            cells: vec![Vec::new(); cols * rows],
            count: 0,
        }
    }

    /// Map a point to (col, row) grid indices, clamped to valid range.
    fn cell_of(&self, p: Point) -> (usize, usize) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let col = ((p.x - self.origin_x) * self.inv_cell_size)
            .floor()
            .max(0.0) as usize;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let row = ((p.y - self.origin_y) * self.inv_cell_size)
            .floor()
            .max(0.0) as usize;
        (col.min(self.cols - 1), row.min(self.rows - 1))
    }

    fn insert(&mut self, idx: usize, p: Point) {
        let (col, row) = self.cell_of(p);
        self.cells[row * self.cols + col].push((idx, p));
        self.count += 1;
    }

    /// Find the output index of the point nearest to `query`.
    ///
    /// Returns `(output_index, distance_squared)`, or `None` if the grid
    /// is empty. Among equidistant points, prefers the highest output
    /// index (most recent), which minimises retrace length.
    fn nearest(&self, query: Point) -> Option<(usize, f64)> {
        if self.count == 0 {
            return None;
        }

        let (qcol, qrow) = self.cell_of(query);
        let mut best_idx: Option<usize> = None;
        let mut best_dist = f64::INFINITY;

        // Maximum ring needed to cover the entire grid from the query cell.
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let max_ring = (self.cols - 1).max(self.rows - 1) as i32;

        for ring in 0..=max_ring {
            // Early termination: if the minimum possible distance from
            // the query to any point in this ring exceeds the best found
            // so far, no further ring can improve the result.
            // Rings 0-1 always searched: the query can lie anywhere inside
            // its cell, so the (ring-1)*cell_size lower bound is only
            // meaningful from ring 2 onward.
            if ring >= 2 {
                let min_ring_dist = f64::from(ring - 1) * self.cell_size;
                if min_ring_dist * min_ring_dist > best_dist {
                    break;
                }
            }

            #[allow(clippy::cast_sign_loss)]
            let r = ring as usize;
            let col_lo = qcol.saturating_sub(r);
            let col_hi = (qcol + r).min(self.cols - 1);
            let row_lo = qrow.saturating_sub(r);
            let row_hi = (qrow + r).min(self.rows - 1);

            for row in row_lo..=row_hi {
                for col in col_lo..=col_hi {
                    // Only visit cells on the border of this ring.
                    if ring > 0 && col > col_lo && col < col_hi && row > row_lo && row < row_hi {
                        continue;
                    }
                    for &(idx, pt) in &self.cells[row * self.cols + col] {
                        let d = query.distance_squared(pt);
                        #[allow(clippy::float_cmp)]
                        // exact equality for tie-breaking is intentional
                        if d < best_dist || (d == best_dist && Some(idx) > best_idx) {
                            best_dist = d;
                            best_idx = Some(idx);
                        }
                    }
                }
            }
        }

        best_idx.map(|idx| (idx, best_dist))
    }
}

// ---------------------------------------------------------------------------
// Arc-length sampling of candidate contours
// ---------------------------------------------------------------------------

/// Return vertex indices sampled at approximately `resolution` arc-length
/// spacing along the contour. Always includes the first and last vertex.
fn arc_length_sample_indices(contour: &Polyline, resolution: f64) -> Vec<usize> {
    let pts = contour.points();
    if pts.is_empty() {
        return Vec::new();
    }
    let last = pts.len() - 1;
    if last == 0 {
        return vec![0];
    }

    let mut indices = vec![0_usize];
    let mut accumulated = 0.0;

    for i in 1..pts.len() {
        accumulated += pts[i - 1].distance(pts[i]);
        if accumulated >= resolution {
            indices.push(i);
            accumulated %= resolution;
        }
    }

    // Always include the last vertex.
    if indices.last() != Some(&last) {
        indices.push(last);
    }

    indices
}

// ---------------------------------------------------------------------------
// Emit helpers: push points to output and index them in the spatial grid
// ---------------------------------------------------------------------------

/// Push every point in `pts` onto `output` and insert each into `grid`.
fn emit_and_index(output: &mut Vec<Point>, grid: &mut SpatialGrid, pts: &[Point]) {
    for &pt in pts {
        output.push(pt);
        grid.insert(output.len() - 1, pt);
    }
}

/// Push every point in `pts` (reversed) onto `output` and insert each into
/// `grid`.
fn emit_reversed_and_index(output: &mut Vec<Point>, grid: &mut SpatialGrid, pts: &[Point]) {
    for &pt in pts.iter().rev() {
        output.push(pt);
        grid.insert(output.len() - 1, pt);
    }
}

// ---------------------------------------------------------------------------
// Lazy cache update for nearby candidates
// ---------------------------------------------------------------------------

/// After emitting a contour (plus its retrace prefix), update cached
/// distances for candidate samples that are geometrically close to the
/// newly emitted points.
///
/// For each emitted point we find its grid cell, then scan the ±1
/// neighbourhood in `cell_to_samples`. Each candidate sample found there
/// is re-queried against the current grid; if the new distance is better
/// than the cached value the cache entry is replaced.
///
/// This is a heuristic: candidates whose samples are more than 1 cell
/// away from the emitted points may retain stale (higher) cached
/// distances. This affects only ordering quality, not correctness — a
/// stale entry delays but never skips a candidate.
fn update_nearby_caches(
    emitted_pts: &[Point],
    grid: &SpatialGrid,
    cell_to_samples: &[Vec<(usize, Point, usize)>],
    used: &[bool],
    cache: &mut [(f64, usize, usize)],
    visited_cells: &mut [bool],
    cells_to_scan: &mut Vec<usize>,
) {
    // Reset reusable buffers.
    visited_cells.fill(false);
    cells_to_scan.clear();

    for &pt in emitted_pts {
        let (col, row) = grid.cell_of(pt);
        // Scan ±1 neighbourhood around this cell.
        let col_lo = col.saturating_sub(1);
        let col_hi = (col + 1).min(grid.cols - 1);
        let row_lo = row.saturating_sub(1);
        let row_hi = (row + 1).min(grid.rows - 1);

        for r in row_lo..=row_hi {
            for c in col_lo..=col_hi {
                let idx = r * grid.cols + c;
                if !visited_cells[idx] {
                    visited_cells[idx] = true;
                    cells_to_scan.push(idx);
                }
            }
        }
    }

    // For each candidate sample in the scanned cells, re-query against
    // the grid and update the cache if better.
    for &cell_idx in cells_to_scan.iter() {
        for &(cand_idx, sample_pt, vert_idx) in &cell_to_samples[cell_idx] {
            if used[cand_idx] {
                continue;
            }
            if let Some((hist_idx, dist)) = grid.nearest(sample_pt)
                && dist < cache[cand_idx].0
            {
                cache[cand_idx] = (dist, vert_idx, hist_idx);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Retrace joiner
// ---------------------------------------------------------------------------

/// Target number of grid cells across the longest canvas dimension.
/// Balances lookup speed against memory overhead for typical workloads
/// (~200 contours x 50 pts).
const GRID_CELLS_PER_AXIS: f64 = 50.0;

/// Full-history retrace with integrated contour ordering.
///
/// Algorithm (retrace-aware greedy nearest-neighbor):
///
/// 1. Build a spatial grid over the output history for O(1) nearest
///    lookups.
/// 2. Pick contour 0, emit its points into the output path and index
///    them in the grid.
/// 3. While candidates remain:
///    a. For each candidate, sample entry points at `cell_size`
///    arc-length spacing (including endpoints). Query each sample
///    against the grid to find the closest history point.
///    b. Pick the (candidate, `entry_index`, `history_index`) triple with
///    the smallest distance.
///    c. Retrace backward through the output to the history point
///    (invisible in sand).
///    d. Emit the chosen contour starting from the entry index,
///    covering the full contour via a split traversal when the
///    entry is interior.
/// 4. Return the output path.
#[allow(clippy::too_many_lines)]
fn join_retrace(contours: &[Polyline]) -> Polyline {
    // Filter out empty contours.
    let candidates: Vec<&Polyline> = contours.iter().filter(|c| !c.is_empty()).collect();

    if candidates.is_empty() {
        return Polyline::new(Vec::new());
    }

    // Derive cell size from bounding box.
    debug_assert!(
        candidates.iter().any(|c| !c.is_empty()),
        "join_retrace requires at least one non-empty contour"
    );
    let (min_x, min_y, max_x, max_y) = polyline_bounding_box(&candidates);
    let canvas_extent = (max_x - min_x).max(max_y - min_y).max(1.0);
    let cell_size = canvas_extent / GRID_CELLS_PER_AXIS;

    let n = candidates.len();
    let mut used = vec![false; n];
    let mut output: Vec<Point> = Vec::new();
    let mut grid = SpatialGrid::new(min_x, min_y, max_x, max_y, cell_size);

    // Reserve a lower bound (all contour points).
    let lower_bound: usize = candidates.iter().map(|c| c.len()).sum();
    output.reserve(lower_bound);

    // Pre-compute arc-length sample points: (point, contour_vertex_index).
    let samples: Vec<Vec<(Point, usize)>> = candidates
        .iter()
        .map(|c| {
            let indices = arc_length_sample_indices(c, cell_size);
            let pts = c.points();
            indices.iter().map(|&i| (pts[i], i)).collect()
        })
        .collect();

    // Map grid cells → candidate sample info for lazy cache updates.
    let mut cell_to_samples: Vec<Vec<(usize, Point, usize)>> =
        vec![Vec::new(); grid.cols * grid.rows];
    for (j, sample_pts) in samples.iter().enumerate() {
        for &(pt, vert_idx) in sample_pts {
            let (col, row) = grid.cell_of(pt);
            cell_to_samples[row * grid.cols + col].push((j, pt, vert_idx));
        }
    }

    // Per-candidate best-known (dist, entry_vertex_idx, history_idx).
    let mut cache: Vec<(f64, usize, usize)> = vec![(f64::INFINITY, 0, 0); n];

    // Start with candidate 0.
    used[0] = true;
    emit_and_index(&mut output, &mut grid, candidates[0].points());

    // Seed caches: query ALL candidates against initial grid.
    for (j, sample_pts) in samples.iter().enumerate() {
        if used[j] {
            continue;
        }
        for &(entry, entry_idx) in sample_pts {
            if let Some((hist_idx, dist)) = grid.nearest(entry)
                && dist < cache[j].0
            {
                cache[j] = (dist, entry_idx, hist_idx);
            }
        }
    }

    // Reusable buffers for update_nearby_caches (hoisted to avoid
    // per-iteration heap allocation).
    let mut visited_cells: Vec<bool> = vec![false; grid.cols * grid.rows];
    let mut cells_to_scan: Vec<usize> = Vec::new();

    for _ in 1..n {
        // Pick the candidate with the best cached distance.
        let mut best_candidate: Option<usize> = None;
        let mut best_dist = f64::INFINITY;

        for j in 0..n {
            if !used[j] && cache[j].0 < best_dist {
                best_dist = cache[j].0;
                best_candidate = Some(j);
            }
        }

        let Some(chosen_idx) = best_candidate else {
            break; // all candidates consumed (shouldn't happen in this loop)
        };

        // Re-query the chosen candidate's entry point for a fresh
        // history_idx (the grid may have grown since the cache was set).
        let best_entry_idx = cache[chosen_idx].1;
        let entry_pt = candidates[chosen_idx].points()[best_entry_idx];
        // Safety: the grid is non-empty — candidate 0 was already emitted.
        let Some((best_history_idx, _)) = grid.nearest(entry_pt) else {
            unreachable!("grid is non-empty after initial emission")
        };
        used[chosen_idx] = true;

        // Retrace from current output end to the best history point.
        // Every retraced point follows an already-drawn groove
        // (invisible in sand).
        //
        // Safety: `k` is always < `current_end` (the length before this
        // loop started).  Point is Copy, so `output[k]` captures a value
        // before any reallocation caused by push.
        let current_end = output.len() - 1;
        if best_history_idx < current_end {
            for k in (best_history_idx..current_end).rev() {
                let pt = output[k];
                output.push(pt);
                grid.insert(output.len() - 1, pt);
            }
        }

        // Emit the chosen contour from the entry index.
        let pts = candidates[chosen_idx].points();
        let e = best_entry_idx;
        let last = pts.len() - 1;

        if e == 0 {
            emit_and_index(&mut output, &mut grid, pts);
        } else if e == last {
            emit_reversed_and_index(&mut output, &mut grid, pts);
        } else {
            // Split traversal: forward → retrace → backward.
            emit_and_index(&mut output, &mut grid, &pts[e..]);
            for i in (e..last).rev() {
                output.push(pts[i]);
                grid.insert(output.len() - 1, pts[i]);
            }
            for i in (0..e).rev() {
                output.push(pts[i]);
                grid.insert(output.len() - 1, pts[i]);
            }
        }

        // Update caches for candidates near the newly emitted contour.
        // Only the contour's own points are passed because the retrace
        // prefix consists of coordinate-duplicates of existing history
        // points — they cannot improve any candidate's cached distance.
        update_nearby_caches(
            candidates[chosen_idx].points(),
            &grid,
            &cell_to_samples,
            &used,
            &mut cache,
            &mut visited_cells,
            &mut cells_to_scan,
        );
    }

    Polyline::new(output)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::types::Point;

    fn default_config() -> PipelineConfig {
        PipelineConfig::default()
    }

    #[test]
    fn default_is_mst() {
        assert_eq!(PathJoinerKind::default(), PathJoinerKind::Mst);
    }

    #[test]
    fn join_empty_contours() {
        let result = PathJoinerKind::StraightLine.join(&[], &default_config());
        assert!(result.is_empty());
    }

    #[test]
    fn join_single_contour() {
        let contour = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(2.0, 0.0),
        ]);
        let result =
            PathJoinerKind::StraightLine.join(std::slice::from_ref(&contour), &default_config());
        assert_eq!(result, contour);
    }

    #[test]
    fn retrace_empty_contours() {
        let result = PathJoinerKind::Retrace.join(&[], &default_config());
        assert!(result.is_empty());
    }

    #[test]
    fn retrace_single_contour() {
        let contour = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(2.0, 0.0),
        ]);
        let result =
            PathJoinerKind::Retrace.join(std::slice::from_ref(&contour), &default_config());
        // Single contour: no joining needed, output equals input.
        assert_eq!(result, contour);
    }

    #[test]
    fn retrace_considers_reversed_orientation() {
        // c1: (0,0) -> (10,0)
        // c2: (50,0) -> (11,0) -- reversed entry (11,0) is much closer to
        //   history (10,0) than forward entry (50,0).
        let c1 = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0)]);
        let c2 = Polyline::new(vec![Point::new(50.0, 0.0), Point::new(11.0, 0.0)]);

        let result = PathJoinerKind::Retrace.join(&[c1, c2], &default_config());
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

        let result = PathJoinerKind::Retrace.join(&[c0, c1, c2], &default_config());
        let pts = result.points();

        // c0 emitted first, then c2 (nearby), then c1 (far).
        // c0: (0,0), (1,0)
        // c2: (2,0), (3,0) -- no backtrack, end of c0 is closest
        // c1: (100,0), (101,0) -- no backtrack, end of c2 is closest candidate
        assert_eq!(pts[0], Point::new(0.0, 0.0));
        assert_eq!(pts[1], Point::new(1.0, 0.0));
        assert_eq!(pts[2], Point::new(2.0, 0.0));
        assert_eq!(pts[3], Point::new(3.0, 0.0));
        // c1 follows after c2 (no backtrack needed; c2 end (3,0) is closest to c1 start)
        assert_eq!(pts[4], Point::new(100.0, 0.0));
        assert_eq!(pts[5], Point::new(101.0, 0.0));
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
        let result = PathJoinerKind::Retrace.join(&contours, &default_config());

        assert!(
            result.len() >= total_contour_points,
            "retrace output ({}) should be >= sum of contour points ({total_contour_points})",
            result.len(),
        );
    }

    #[test]
    fn retrace_interior_entry_split_traversal() {
        // c0: short contour near origin to seed history.
        let c0 = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(1.0, 0.0)]);

        // c1: long contour along y=10 from (-50, 10) to (50, 10) with
        // 101 evenly spaced vertices (spacing = 1.0). The bounding box
        // spans x: [-50, 50], y: [0, 10] → canvas_extent = 100,
        // cell_size = 100/50 = 2.0. arc_length_sample_indices produces
        // samples every 2 vertices, including interior points.
        //
        // The sample at index 50 = (0, 10) is closest to history point
        // (0, 0) at distance 10.0 — an interior vertex — triggering the
        // split traversal branch.
        let c1 = Polyline::new(
            (0..101)
                .map(|i| Point::new(f64::from(i) - 50.0, 10.0))
                .collect(),
        );

        let result = PathJoinerKind::Retrace.join(&[c0.clone(), c1.clone()], &default_config());
        let output_pts = result.points();

        // All of c1's points must appear in the output.
        let c1_set: std::collections::HashSet<_> = c1
            .points()
            .iter()
            .map(|p| (p.x.to_bits(), p.y.to_bits()))
            .collect();
        let output_set: std::collections::HashSet<_> = output_pts
            .iter()
            .map(|p| (p.x.to_bits(), p.y.to_bits()))
            .collect();
        assert!(
            c1_set.is_subset(&output_set),
            "all of c1's points should appear in the output"
        );

        // Split traversal retraces within c1 (forward to one end,
        // back to entry, backward to other end), producing more output
        // points than c0.len() + c1.len().
        assert!(
            result.len() > c0.len() + c1.len(),
            "split traversal should produce extra retrace points: got {} but expected > {}",
            result.len(),
            c0.len() + c1.len(),
        );
    }
}
