//! MST-based path joining: connect disconnected contours via a minimum
//! spanning tree, then find an Eulerian path through the resulting graph.
//!
//! # Algorithm overview
//!
//! 1. **Phase 1 — MST via Kruskal:** Build an R\*-tree of all polyline
//!    segments, sample points along each polyline, and query the R-tree
//!    for K nearest cross-component segments to generate candidate edges.
//!    Sort candidates by distance and merge via `UnionFind` (Kruskal's
//!    algorithm) until a single connected component remains.
//!
//! 2. **Phase 2 — Fix parity:** An Eulerian path requires exactly 0 or 2
//!    odd-degree vertices. Greedily pair remaining odd-degree vertices and
//!    duplicate the shortest path between each pair (duplication represents
//!    free retracing through already-drawn grooves).
//!
//! 3. **Phase 3 — Hierholzer:** Find an Eulerian path (or circuit) that
//!    traverses every edge exactly once.
//!
//! 4. **Phase 4 — Emit:** Convert the vertex sequence into a `Polyline`.

use geo::line_measures::Distance;
use geo::{Closest, ClosestPoint, Euclidean, Line};
use petgraph::algo::dijkstra;
use petgraph::graph::{NodeIndex, UnGraph};
use petgraph::unionfind::UnionFind;
use petgraph::visit::EdgeRef;
use rstar::RTree;
use rstar::primitives::GeomWithData;

use crate::types::{Point, Polyline, polyline_bounding_box};

// ---------------------------------------------------------------------------
// Type conversions at the module boundary
// ---------------------------------------------------------------------------

/// Convert a pipeline `Point` to a `geo::Coord`.
const fn point_to_coord(p: Point) -> geo::Coord<f64> {
    geo::Coord { x: p.x, y: p.y }
}

/// Convert a `geo::Coord` back to a pipeline `Point`.
const fn coord_to_point(c: geo::Coord<f64>) -> Point {
    Point::new(c.x, c.y)
}

// ---------------------------------------------------------------------------
// R-tree entry: a geo::Line tagged with a segment identifier
// ---------------------------------------------------------------------------

/// Identifies a segment within the input polyline set.
///
/// `(polyline_index, segment_index)` — the segment from vertex `i` to
/// vertex `i+1` in polyline `polyline_index`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SegmentId {
    polyline_idx: usize,
    segment_idx: usize,
}

/// A `geo::Line` tagged with its [`SegmentId`], suitable for R\*-tree
/// insertion.
type IndexedSegment = GeomWithData<Line<f64>, SegmentId>;

// ---------------------------------------------------------------------------
// Phase 1: MST via Kruskal
// ---------------------------------------------------------------------------

/// An MST edge connecting two polylines at specific points.
#[derive(Debug, Clone, Copy)]
struct MstEdge {
    /// Polyline index of one endpoint.
    poly_a: usize,
    /// Polyline index of the other endpoint.
    poly_b: usize,
    /// Connection point on polyline A (may be interior to a segment).
    point_a: geo::Coord<f64>,
    /// Connection point on polyline B (may be interior to a segment).
    point_b: geo::Coord<f64>,
    /// Segment index within polyline A where the connection point lies.
    seg_a: usize,
    /// Segment index within polyline B where the connection point lies.
    seg_b: usize,
}

/// Generate sample points along a polyline's segments at adaptive density.
///
/// Longer segments get more samples; short segments contribute only their
/// endpoints. Returns `(geo::Point, polyline_index, segment_index)` tuples
/// suitable for R-tree nearest-neighbor queries.
fn sample_points_along_polyline(
    poly: &Polyline,
    poly_idx: usize,
    max_sample_spacing: f64,
) -> Vec<(geo::Point<f64>, usize, usize)> {
    let pts = poly.points();
    if pts.len() < 2 {
        return if pts.is_empty() {
            Vec::new()
        } else {
            vec![(geo::Point::new(pts[0].x, pts[0].y), poly_idx, 0)]
        };
    }

    let mut samples = Vec::new();
    for seg_idx in 0..pts.len() - 1 {
        let start = pts[seg_idx];
        let end = pts[seg_idx + 1];
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        let seg_len = dx.hypot(dy);

        // Always include the start vertex.
        samples.push((geo::Point::new(start.x, start.y), poly_idx, seg_idx));

        // Intermediate samples for long segments.
        if seg_len > max_sample_spacing {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let n_intervals = (seg_len / max_sample_spacing).ceil() as usize;
            for k in 1..n_intervals {
                #[allow(clippy::cast_precision_loss)]
                let frac = k as f64 / n_intervals as f64;
                samples.push((
                    geo::Point::new(frac.mul_add(dx, start.x), frac.mul_add(dy, start.y)),
                    poly_idx,
                    seg_idx,
                ));
            }
        }
    }
    // Always include the last vertex.
    if let Some(last) = pts.last() {
        let last_seg = pts.len() - 2;
        samples.push((geo::Point::new(last.x, last.y), poly_idx, last_seg));
    }

    samples
}

/// Find the closest point on a `geo::Line` to a query `geo::Point`,
/// returning the `geo::Coord` of that point.
fn closest_coord_on_line(line: &Line<f64>, query: &geo::Point<f64>) -> geo::Coord<f64> {
    match line.closest_point(query) {
        Closest::Intersection(p) | Closest::SinglePoint(p) => p.into(),
        Closest::Indeterminate => line.start,
    }
}

/// Hard cap on total R-tree nearest-neighbor iterations per sample
/// point to prevent degenerate O(N) scans when a polyline has many
/// self-segments.
const MAX_NN_ITERATIONS: usize = 200;

/// Number of pixels (at working resolution) between sample points used
/// for R-tree nearest-neighbor queries during MST candidate generation.
///
/// Lower values produce more samples and better MST quality at the cost
/// of more R-tree queries.  At the default `working_resolution` of 256
/// this yields ~51 samples across the longest axis, matching the
/// previous hard-coded divisor of 50.
const SAMPLE_SPACING_PIXELS: f64 = 5.0;

/// Build an MST connecting all polylines using Kruskal's algorithm with
/// R-tree-accelerated candidate edge generation.
///
/// For each sample point on each polyline, queries the K nearest segments
/// in the R-tree. Cross-component pairs become candidate edges. All
/// candidates are sorted by distance and processed via Kruskal's
/// union-find merge.
///
/// Returns a list of [`MstEdge`]s (one fewer than the number of polylines).
#[allow(clippy::too_many_lines)]
fn build_mst(polylines: &[&Polyline], k_nearest: usize, working_resolution: u32) -> Vec<MstEdge> {
    let n = polylines.len();
    if n <= 1 {
        return Vec::new();
    }

    // Build R-tree of all segments.
    let segments: Vec<IndexedSegment> = polylines
        .iter()
        .enumerate()
        .flat_map(|(pi, poly)| {
            let pts = poly.points();
            (0..pts.len().saturating_sub(1)).map(move |si| {
                let a = point_to_coord(pts[si]);
                let b = point_to_coord(pts[si + 1]);
                GeomWithData::new(
                    Line::new(a, b),
                    SegmentId {
                        polyline_idx: pi,
                        segment_idx: si,
                    },
                )
            })
        })
        .collect();

    let tree = RTree::bulk_load(segments);

    // Pre-compute sample points for each polyline (adaptive spacing).
    // Derive sample spacing from working_resolution so density scales
    // with the image resolution rather than being a fixed count.
    let (min_x, min_y, max_x, max_y) = polyline_bounding_box(polylines);
    let extent = (max_x - min_x).max(max_y - min_y).max(1.0);
    let pixel_size = extent / f64::from(working_resolution).max(1.0);
    let sample_spacing = pixel_size * SAMPLE_SPACING_PIXELS;

    let all_samples: Vec<Vec<(geo::Point<f64>, usize, usize)>> = polylines
        .iter()
        .enumerate()
        .map(|(pi, poly)| sample_points_along_polyline(poly, pi, sample_spacing))
        .collect();

    // Generate candidate edges via R-tree k-nearest queries.
    // For each sample point, find the K nearest segments and record
    // cross-polyline connections as candidate edges.
    let mut candidates: Vec<(f64, MstEdge)> = Vec::new();

    for samples in &all_samples {
        for &(query_pt, poly_idx, seg_idx) in samples {
            let my_pts = polylines[poly_idx].points();
            let my_seg_end = (seg_idx + 1).min(my_pts.len().saturating_sub(1));
            let my_line = Line::new(
                point_to_coord(my_pts[seg_idx]),
                point_to_coord(my_pts[my_seg_end]),
            );

            let mut cross_count = 0;
            let mut iter_count = 0;
            for candidate in tree.nearest_neighbor_iter(&query_pt) {
                iter_count += 1;
                if iter_count > MAX_NN_ITERATIONS {
                    break;
                }

                let cand_poly = candidate.data.polyline_idx;
                if cand_poly == poly_idx {
                    continue; // Same polyline, skip without counting.
                }

                let cand_line = *candidate.geom();
                let dist = Euclidean.distance(&my_line, &cand_line);

                // Find exact connection points.
                let point_on_cand = closest_coord_on_line(&cand_line, &query_pt);
                let point_on_mine =
                    closest_coord_on_line(&my_line, &geo::Point::from(point_on_cand));

                let exact_dist = Euclidean.distance(
                    &geo::Point::from(point_on_mine),
                    &geo::Point::from(point_on_cand),
                );

                let use_dist = dist.min(exact_dist);

                candidates.push((
                    use_dist,
                    MstEdge {
                        poly_a: poly_idx,
                        poly_b: cand_poly,
                        point_a: point_on_mine,
                        point_b: point_on_cand,
                        seg_a: seg_idx,
                        seg_b: candidate.data.segment_idx,
                    },
                ));

                cross_count += 1;
                if cross_count >= k_nearest {
                    break;
                }
            }
        }
    }

    // Sort candidates by distance (Kruskal's algorithm).
    candidates.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // Kruskal: process edges in sorted order, union-find to accept
    // only cross-component edges.
    let mut uf = UnionFind::<usize>::new(n);
    let mut edges = Vec::with_capacity(n - 1);

    for (_, edge) in candidates {
        let ra = uf.find_mut(edge.poly_a);
        let rb = uf.find_mut(edge.poly_b);
        if ra != rb {
            uf.union(ra, rb);
            edges.push(edge);
            if edges.len() == n - 1 {
                break; // MST complete.
            }
        }
    }

    // If R-tree candidates didn't cover all components, fall back to
    // brute-force: collect all cross-component endpoint pairs, sort by
    // distance, and continue Kruskal's algorithm.
    if edges.len() < n - 1 {
        // Collect disconnected component representatives.
        let disconnected: Vec<usize> = (0..n)
            .filter(|&i| {
                let root = uf.find_mut(i);
                // Include one representative per component that still
                // needs connecting. We check all polylines.
                root == i || (0..i).all(|j| uf.find_mut(j) != root)
            })
            .collect();

        // Gather all cross-component endpoint-pair candidates.
        let mut fallback_candidates: Vec<(f64, MstEdge)> = Vec::new();
        for (idx_i, &pi) in disconnected.iter().enumerate() {
            for &pj in disconnected.iter().skip(idx_i + 1) {
                if uf.find_mut(pi) == uf.find_mut(pj) {
                    continue;
                }
                let pts_a = polylines[pi].points();
                let pts_b = polylines[pj].points();
                if pts_a.is_empty() || pts_b.is_empty() {
                    continue;
                }
                let mut best_dist = f64::INFINITY;
                let mut best_a = point_to_coord(pts_a[0]);
                let mut best_b = point_to_coord(pts_b[0]);
                let mut best_seg_a = 0;
                let mut best_seg_b = 0;

                for &(ai, asi) in &[(0, 0), (pts_a.len() - 1, pts_a.len().saturating_sub(2))] {
                    for &(bi, bsi) in &[(0, 0), (pts_b.len() - 1, pts_b.len().saturating_sub(2))] {
                        let ca = point_to_coord(pts_a[ai]);
                        let cb = point_to_coord(pts_b[bi]);
                        let d = (ca.x - cb.x).hypot(ca.y - cb.y);
                        if d < best_dist {
                            best_dist = d;
                            best_a = ca;
                            best_b = cb;
                            best_seg_a = asi;
                            best_seg_b = bsi;
                        }
                    }
                }

                fallback_candidates.push((
                    best_dist,
                    MstEdge {
                        poly_a: pi,
                        poly_b: pj,
                        point_a: best_a,
                        point_b: best_b,
                        seg_a: best_seg_a,
                        seg_b: best_seg_b,
                    },
                ));
            }
        }

        // Sort by distance and apply Kruskal's to the fallback candidates.
        fallback_candidates
            .sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        for (_, edge) in fallback_candidates {
            let ra = uf.find_mut(edge.poly_a);
            let rb = uf.find_mut(edge.poly_b);
            if ra != rb {
                uf.union(ra, rb);
                edges.push(edge);
                if edges.len() == n - 1 {
                    break;
                }
            }
        }
    }

    edges
}

// ---------------------------------------------------------------------------
// Graph construction: polyline segments + MST connecting edges
// ---------------------------------------------------------------------------

/// A vertex in the Eulerian graph, identified by its `geo::Coord`.
///
/// We use a map from discretized coordinates to `NodeIndex` for dedup.
/// Floating-point coords are compared via bit-exact equality (same as
/// the existing `retrace_interior_entry_split_traversal` test).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CoordKey {
    x_bits: u64,
    y_bits: u64,
}

impl CoordKey {
    const fn from_coord(c: geo::Coord<f64>) -> Self {
        Self {
            x_bits: c.x.to_bits(),
            y_bits: c.y.to_bits(),
        }
    }
}

/// Build the Eulerian graph from polyline segments and MST connecting edges.
///
/// Returns `(graph, node_coords)` where `node_coords[node_index]` gives
/// the `geo::Coord` for each node.
#[allow(clippy::too_many_lines)]
fn build_graph(
    polylines: &[&Polyline],
    mst_edges: &[MstEdge],
) -> (UnGraph<(), f64>, Vec<geo::Coord<f64>>) {
    let mut graph = UnGraph::<(), f64>::new_undirected();
    let mut coord_to_node = std::collections::HashMap::<CoordKey, NodeIndex>::new();
    let mut node_coords: Vec<geo::Coord<f64>> = Vec::new();

    let get_or_insert = |coord: geo::Coord<f64>,
                         graph: &mut UnGraph<(), f64>,
                         coord_to_node: &mut std::collections::HashMap<CoordKey, NodeIndex>,
                         node_coords: &mut Vec<geo::Coord<f64>>|
     -> NodeIndex {
        let key = CoordKey::from_coord(coord);
        *coord_to_node.entry(key).or_insert_with(|| {
            let idx = graph.add_node(());
            node_coords.push(coord);
            idx
        })
    };

    // Collect all segment split points from MST edges.
    // For each (polyline_idx, segment_idx), record the split point(s).
    let mut splits: std::collections::HashMap<(usize, usize), Vec<geo::Coord<f64>>> =
        std::collections::HashMap::new();
    for edge in mst_edges {
        splits
            .entry((edge.poly_a, edge.seg_a))
            .or_default()
            .push(edge.point_a);
        splits
            .entry((edge.poly_b, edge.seg_b))
            .or_default()
            .push(edge.point_b);
    }

    // Add polyline segments, splitting at MST connection points.
    for (pi, poly) in polylines.iter().enumerate() {
        let pts = poly.points();
        if pts.is_empty() {
            continue;
        }

        for si in 0..pts.len().saturating_sub(1) {
            let seg_start = point_to_coord(pts[si]);
            let seg_end = point_to_coord(pts[si + 1]);

            if let Some(split_pts) = splits.get(&(pi, si)) {
                // Sort split points by distance from segment start.
                let mut ordered_splits: Vec<geo::Coord<f64>> = split_pts.clone();
                ordered_splits.sort_by(|a, b| {
                    let da = (a.x - seg_start.x).hypot(a.y - seg_start.y);
                    let db = (b.x - seg_start.x).hypot(b.y - seg_start.y);
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                });
                // Deduplicate split points that are the same as endpoints.
                ordered_splits.retain(|sp| {
                    let at_start = (sp.x - seg_start.x).hypot(sp.y - seg_start.y) < 1e-10;
                    let at_end = (sp.x - seg_end.x).hypot(sp.y - seg_end.y) < 1e-10;
                    !at_start && !at_end
                });
                // Deduplicate near-identical split points.
                ordered_splits.dedup_by(|a, b| (a.x - b.x).hypot(a.y - b.y) < 1e-10);

                // Build sub-segments: start -> split1 -> split2 -> ... -> end.
                let mut prev = seg_start;
                for sp in &ordered_splits {
                    let n_prev =
                        get_or_insert(prev, &mut graph, &mut coord_to_node, &mut node_coords);
                    let n_sp = get_or_insert(*sp, &mut graph, &mut coord_to_node, &mut node_coords);
                    let dist = (prev.x - sp.x).hypot(prev.y - sp.y);
                    if dist > 1e-12 {
                        graph.add_edge(n_prev, n_sp, dist);
                    }
                    prev = *sp;
                }
                let n_prev = get_or_insert(prev, &mut graph, &mut coord_to_node, &mut node_coords);
                let n_end =
                    get_or_insert(seg_end, &mut graph, &mut coord_to_node, &mut node_coords);
                let dist = (prev.x - seg_end.x).hypot(prev.y - seg_end.y);
                if dist > 1e-12 {
                    graph.add_edge(n_prev, n_end, dist);
                }
            } else {
                // No split: single edge for this segment.
                let na = get_or_insert(seg_start, &mut graph, &mut coord_to_node, &mut node_coords);
                let nb = get_or_insert(seg_end, &mut graph, &mut coord_to_node, &mut node_coords);
                let dist = (seg_start.x - seg_end.x).hypot(seg_start.y - seg_end.y);
                if dist > 1e-12 {
                    graph.add_edge(na, nb, dist);
                }
            }
        }

        // Handle single-point polylines.
        if pts.len() == 1 {
            let coord = point_to_coord(pts[0]);
            get_or_insert(coord, &mut graph, &mut coord_to_node, &mut node_coords);
        }
    }

    // Add MST connecting edges.
    for edge in mst_edges {
        let na = get_or_insert(
            edge.point_a,
            &mut graph,
            &mut coord_to_node,
            &mut node_coords,
        );
        let nb = get_or_insert(
            edge.point_b,
            &mut graph,
            &mut coord_to_node,
            &mut node_coords,
        );
        let dist = (edge.point_a.x - edge.point_b.x).hypot(edge.point_a.y - edge.point_b.y);
        if dist > 1e-12 {
            graph.add_edge(na, nb, dist);
        } else {
            // Zero-length MST edge: the points coincide. Still add an edge
            // so the graph stays connected.
            graph.add_edge(na, nb, 0.0);
        }
    }

    (graph, node_coords)
}

// ---------------------------------------------------------------------------
// Phase 2: Fix parity for Eulerian path
// ---------------------------------------------------------------------------

/// Identify vertices with odd degree.
fn odd_degree_vertices(graph: &UnGraph<(), f64>) -> Vec<NodeIndex> {
    graph
        .node_indices()
        .filter(|&n| graph.edges(n).count() % 2 != 0)
        .collect()
}

/// Fix parity by duplicating shortest paths between paired odd-degree
/// vertices.
///
/// Greedily pairs each odd-degree vertex with its nearest unmatched odd
/// peer and duplicates the shortest path between them. Duplicated edges
/// represent retracing (visually free).
///
/// Uses Euclidean distance as a fast heuristic for pairing (avoids
/// running Dijkstra for every pair), then Dijkstra only for path
/// reconstruction of the selected pairs.
/// # Errors
///
/// Returns an error if shortest-path reconstruction fails for any
/// odd-degree vertex pair (indicates a graph connectivity bug).
fn fix_parity(graph: &mut UnGraph<(), f64>, node_coords: &[geo::Coord<f64>]) -> Result<(), String> {
    let mut odd = odd_degree_vertices(graph);

    if odd.len() <= 2 {
        return Ok(()); // 0 or 2 odd-degree vertices: already Eulerian.
    }

    // Greedy matching using Euclidean distance as heuristic: pair each
    // odd vertex with the nearest unmatched odd peer.
    while odd.len() > 2 {
        let mut best_i = 0;
        let mut best_j = 1;
        let mut best_dist = f64::INFINITY;

        // Find the closest pair by Euclidean distance (O(n^2) but n is small).
        for (i, &ni) in odd.iter().enumerate() {
            let ci = node_coords[ni.index()];
            for (j, &nj) in odd.iter().enumerate().skip(i + 1) {
                let cj = node_coords[nj.index()];
                let d = (ci.x - cj.x).hypot(ci.y - cj.y);
                if d < best_dist {
                    best_dist = d;
                    best_i = i;
                    best_j = j;
                }
            }
        }

        // Reconstruct the shortest path between the chosen pair and
        // duplicate each edge along it.
        let start = odd[best_i];
        let end = odd[best_j];
        let path = shortest_path(graph, start, end)?;
        for window in path.windows(2) {
            let (a, b) = (window[0], window[1]);
            // Find the weight of the existing edge.
            let weight = graph
                .edges(a)
                .find(|e| e.target() == b)
                .map_or(0.0, |e| *e.weight());
            graph.add_edge(a, b, weight);
        }

        // Remove the matched pair (remove higher index first).
        if best_i > best_j {
            odd.swap_remove(best_i);
            odd.swap_remove(best_j);
        } else {
            odd.swap_remove(best_j);
            odd.swap_remove(best_i);
        }
    }
    Ok(())
}

/// Reconstruct the shortest path from `start` to `end` using Dijkstra.
///
/// Returns the node sequence `[start, ..., end]`.
///
/// # Errors
///
/// Returns an error if `end` is unreachable from `start` or if path
/// reconstruction fails (e.g. due to a disconnected graph).
fn shortest_path(
    graph: &UnGraph<(), f64>,
    start: NodeIndex,
    end: NodeIndex,
) -> Result<Vec<NodeIndex>, String> {
    // Run petgraph's Dijkstra for costs, then reconstruct path greedily.
    let costs = dijkstra(graph as &UnGraph<(), f64>, start, Some(end), |e| {
        *e.weight()
    });

    if !costs.contains_key(&end) {
        return Err(format!(
            "shortest_path: end node {end:?} is unreachable from start {start:?}"
        ));
    }

    // Greedy reconstruction: from end, step to the neighbor with
    // cost[neighbor] + edge_weight == cost[current].
    //
    // A visited set prevents infinite cycling on multigraphs where
    // `fix_parity` has added parallel edges.  Without this guard, two
    // nodes A–B with duplicate edges can satisfy the cost check in both
    // directions, causing the loop to oscillate A→B→A→… forever and
    // eventually trigger a capacity overflow panic.
    let mut visited = std::collections::HashSet::new();
    let mut path = vec![end];
    visited.insert(end);
    let mut current = end;
    while current != start {
        let current_cost = costs.get(&current).copied().unwrap_or(f64::INFINITY);
        let mut next = None;
        for edge in graph.edges(current) {
            let neighbor = edge.target();
            if visited.contains(&neighbor) {
                continue;
            }
            let neighbor_cost = costs.get(&neighbor).copied().unwrap_or(f64::INFINITY);
            let edge_weight = *edge.weight();
            // Check if this neighbor is on the shortest path.
            if (neighbor_cost + edge_weight - current_cost).abs() < 1e-10 {
                next = Some(neighbor);
                break;
            }
        }
        if let Some(n) = next {
            path.push(n);
            visited.insert(n);
            current = n;
        } else {
            return Err(format!(
                "shortest_path: reconstruction stalled at node {current:?} \
                 (start={start:?}, end={end:?}, path len so far={})",
                path.len()
            ));
        }
    }
    path.reverse();

    if path.first() != Some(&start) || path.last() != Some(&end) || path.len() < 2 {
        return Err(format!(
            "shortest_path: reconstruction produced invalid path \
             (start={start:?}, end={end:?}, path len={})",
            path.len()
        ));
    }

    Ok(path)
}

// ---------------------------------------------------------------------------
// Phase 3: Hierholzer's algorithm for Eulerian path
// ---------------------------------------------------------------------------

/// Find an Eulerian path (or circuit) through the graph using
/// Hierholzer's algorithm.
///
/// Assumes the graph has been fixed to have 0 or 2 odd-degree vertices.
/// Returns the node sequence of the path.
fn hierholzer(graph: &UnGraph<(), f64>) -> Vec<NodeIndex> {
    if graph.node_count() == 0 {
        return Vec::new();
    }

    // Choose start vertex: prefer an odd-degree vertex if one exists.
    let start = graph
        .node_indices()
        .find(|&n| graph.edges(n).count() % 2 != 0)
        .or_else(|| graph.node_indices().find(|&n| graph.edges(n).count() > 0))
        .unwrap_or_else(|| {
            graph
                .node_indices()
                .next()
                .unwrap_or_else(|| NodeIndex::new(0))
        });

    let mut stack = vec![start];
    let mut path = Vec::new();

    // Track which edges have been used (by edge index).
    let mut used_edges = vec![false; graph.edge_count()];

    while let Some(&current) = stack.last() {
        // Find an unused edge from current.
        let next_edge = graph.edges(current).find_map(|e| {
            let eidx = e.id().index();
            if used_edges[eidx] {
                None
            } else {
                Some((e.id(), e.target()))
            }
        });

        if let Some((edge_id, target)) = next_edge {
            used_edges[edge_id.index()] = true;
            stack.push(target);
        } else {
            path.push(stack.pop().unwrap_or(start));
        }
    }

    path.reverse();
    path
}

// ---------------------------------------------------------------------------
// Phase 4: Emit as Polyline
// ---------------------------------------------------------------------------

/// Convert a sequence of node indices into a `Polyline` using the
/// coordinate map.
fn emit_polyline(path: &[NodeIndex], node_coords: &[geo::Coord<f64>]) -> Polyline {
    let points: Vec<Point> = path
        .iter()
        .map(|&n| coord_to_point(node_coords[n.index()]))
        .collect();
    Polyline::new(points)
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Join disconnected contours into a single continuous path using an
/// MST-based algorithm.
///
/// The algorithm:
/// 1. Finds globally optimal connections between polyline components via
///    a minimum spanning tree (minimizing total new connecting segment
///    length).
/// 2. Computes segment-to-segment distances using an R\*-tree spatial
///    index for truly closest points between polylines.
/// 3. Fixes vertex parity for Eulerian path existence by duplicating
///    shortest paths (retracing is visually free on sand tables).
/// 4. Finds an Eulerian path through the augmented graph.
///
/// # Panics
///
/// Panics if shortest-path reconstruction fails during the parity-fix
/// phase.  This indicates a bug in MST construction (the graph should
/// be fully connected after phase 1).
#[must_use]
#[allow(clippy::expect_used)] // structural invariant: MST guarantees connectivity
pub fn join_mst(contours: &[Polyline], k_nearest: usize, working_resolution: u32) -> Polyline {
    // Filter out empty contours.
    let polylines: Vec<&Polyline> = contours.iter().filter(|c| !c.is_empty()).collect();

    if polylines.is_empty() {
        return Polyline::new(Vec::new());
    }

    if polylines.len() == 1 {
        return polylines[0].clone();
    }

    // Phase 1: Build MST.
    let mst_edges = build_mst(&polylines, k_nearest.max(1), working_resolution);

    // Phase 2+3: Build graph, fix parity, find Eulerian path.
    let (mut graph, node_coords) = build_graph(&polylines, &mst_edges);
    fix_parity(&mut graph, &node_coords)
        .expect("fix_parity: shortest-path reconstruction failed on MST-connected graph");
    let path = hierholzer(&graph);

    // Phase 4: Emit.
    if path.is_empty() {
        // Fallback: concatenate all contour points.
        let all_points: Vec<Point> = polylines
            .iter()
            .flat_map(|p| p.points().iter().copied())
            .collect();
        return Polyline::new(all_points);
    }

    emit_polyline(&path, &node_coords)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::types::PipelineConfig;

    const TEST_K: usize = PipelineConfig::DEFAULT_MST_NEIGHBOURS;
    const TEST_RESOLUTION: u32 = PipelineConfig::DEFAULT_WORKING_RESOLUTION;
    use crate::types::Point;

    #[test]
    fn mst_join_empty() {
        let result = join_mst(&[], TEST_K, TEST_RESOLUTION);
        assert!(result.is_empty());
    }

    #[test]
    fn mst_join_single_contour() {
        let contour = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(2.0, 0.0),
        ]);
        let result = join_mst(std::slice::from_ref(&contour), TEST_K, TEST_RESOLUTION);
        assert_eq!(result, contour);
    }

    #[test]
    fn mst_join_single_point_contour() {
        let contour = Polyline::new(vec![Point::new(5.0, 5.0)]);
        let result = join_mst(std::slice::from_ref(&contour), TEST_K, TEST_RESOLUTION);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn mst_join_two_contours_produces_single_path() {
        let c1 = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(1.0, 0.0)]);
        let c2 = Polyline::new(vec![Point::new(3.0, 0.0), Point::new(4.0, 0.0)]);
        let result = join_mst(&[c1, c2], TEST_K, TEST_RESOLUTION);

        // Must be non-empty and cover all original points.
        assert!(
            result.len() >= 4,
            "expected >= 4 points, got {}",
            result.len()
        );
    }

    #[test]
    fn mst_join_filters_empty_contours() {
        let c1 = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(1.0, 0.0)]);
        let empty = Polyline::new(Vec::new());
        let c2 = Polyline::new(vec![Point::new(3.0, 0.0), Point::new(4.0, 0.0)]);
        let result = join_mst(&[c1, empty, c2], TEST_K, TEST_RESOLUTION);
        assert!(result.len() >= 4);
    }

    #[test]
    fn mst_join_all_input_segments_covered() {
        // Verify every original point appears in the output.
        let c1 = Polyline::new(vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
        ]);
        let c2 = Polyline::new(vec![Point::new(20.0, 0.0), Point::new(30.0, 0.0)]);
        let c3 = Polyline::new(vec![
            Point::new(50.0, 50.0),
            Point::new(60.0, 50.0),
            Point::new(60.0, 60.0),
        ]);

        let result = join_mst(
            &[c1.clone(), c2.clone(), c3.clone()],
            TEST_K,
            TEST_RESOLUTION,
        );
        let output_set: std::collections::HashSet<(u64, u64)> = result
            .points()
            .iter()
            .map(|p| (p.x.to_bits(), p.y.to_bits()))
            .collect();

        for contour in &[&c1, &c2, &c3] {
            for p in contour.points() {
                assert!(
                    output_set.contains(&(p.x.to_bits(), p.y.to_bits())),
                    "point ({}, {}) from input not found in output",
                    p.x,
                    p.y,
                );
            }
        }
    }

    #[test]
    fn mst_join_path_is_continuous() {
        // The output should be a single polyline (implicitly continuous).
        // There's no structural gap — it's a Vec<Point>.
        let c1 = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(5.0, 0.0)]);
        let c2 = Polyline::new(vec![Point::new(10.0, 0.0), Point::new(15.0, 0.0)]);
        let c3 = Polyline::new(vec![Point::new(20.0, 0.0), Point::new(25.0, 0.0)]);

        let result = join_mst(&[c1, c2, c3], TEST_K, TEST_RESOLUTION);
        assert!(!result.is_empty());
        // All points should be finite.
        for p in result.points() {
            assert!(p.x.is_finite() && p.y.is_finite());
        }
    }

    #[test]
    fn mst_join_collinear_contours() {
        // Three collinear contours — MST should connect them optimally.
        let c1 = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(1.0, 0.0)]);
        let c2 = Polyline::new(vec![Point::new(3.0, 0.0), Point::new(4.0, 0.0)]);
        let c3 = Polyline::new(vec![Point::new(6.0, 0.0), Point::new(7.0, 0.0)]);

        let result = join_mst(&[c1, c2, c3], TEST_K, TEST_RESOLUTION);
        // Output should contain all 6 original points plus MST connections
        // and any retrace edges.
        assert!(
            result.len() >= 6,
            "expected >= 6 points, got {}",
            result.len(),
        );
    }

    #[test]
    fn mst_join_many_contours() {
        // Stress test with many small contours.
        let contours: Vec<Polyline> = (0..20)
            .map(|i| {
                #[allow(clippy::cast_precision_loss)]
                let base = f64::from(i) * 10.0;
                Polyline::new(vec![
                    Point::new(base, 0.0),
                    Point::new(base + 3.0, 5.0),
                    Point::new(base + 6.0, 0.0),
                ])
            })
            .collect();

        let total_points: usize = contours.iter().map(Polyline::len).sum();
        let result = join_mst(&contours, TEST_K, TEST_RESOLUTION);

        assert!(
            result.len() >= total_points,
            "output ({}) should be >= total input points ({total_points})",
            result.len(),
        );
    }

    // --- Internal unit tests ---

    #[test]
    fn type_conversion_roundtrip() {
        let p = Point::new(3.5, 2.5);
        let c = point_to_coord(p);
        let p2 = coord_to_point(c);
        assert_eq!(p, p2);
    }

    #[test]
    fn closest_coord_on_line_endpoint() {
        let line = Line::new(
            geo::Coord { x: 0.0, y: 0.0 },
            geo::Coord { x: 10.0, y: 0.0 },
        );
        let query = geo::Point::new(-5.0, 0.0);
        let result = closest_coord_on_line(&line, &query);
        assert!((result.x - 0.0).abs() < 1e-10);
        assert!((result.y - 0.0).abs() < 1e-10);
    }

    #[test]
    fn closest_coord_on_line_interior() {
        let line = Line::new(
            geo::Coord { x: 0.0, y: 0.0 },
            geo::Coord { x: 10.0, y: 0.0 },
        );
        let query = geo::Point::new(5.0, 3.0);
        let result = closest_coord_on_line(&line, &query);
        assert!((result.x - 5.0).abs() < 1e-10);
        assert!((result.y - 0.0).abs() < 1e-10);
    }

    #[test]
    fn hierholzer_simple_circuit() {
        // Triangle graph: 3 vertices, 3 edges. All degree 2 → Euler circuit.
        let g = UnGraph::<(), f64>::from_edges([(0u32, 1, 1.0), (1, 2, 1.0), (2, 0, 1.0)]);

        let path = hierholzer(&g);
        // Should visit all 3 edges → 4 nodes in path (start == end for circuit).
        assert_eq!(
            path.len(),
            4,
            "Euler circuit on triangle should have 4 nodes"
        );
        assert_eq!(
            path[0], path[3],
            "circuit should start and end at same node"
        );
    }

    #[test]
    fn hierholzer_simple_path() {
        // Path graph: A--B--C. Degree of A and C is 1 (odd), B is 2.
        // Euler path from A to C (or C to A).
        let g = UnGraph::<(), f64>::from_edges([(0u32, 1, 1.0), (1, 2, 1.0)]);

        let path = hierholzer(&g);
        assert_eq!(path.len(), 3, "Euler path on A-B-C should visit 3 nodes");
    }

    #[test]
    fn odd_degree_detection() {
        // Path A-B-C: A and C have degree 1 (odd), B has degree 2 (even).
        let mut g = UnGraph::<(), f64>::new_undirected();
        let a = g.add_node(());
        let b = g.add_node(());
        let c = g.add_node(());
        g.add_edge(a, b, 1.0);
        g.add_edge(b, c, 1.0);

        let odd = odd_degree_vertices(&g);
        assert_eq!(odd.len(), 2);
    }

    #[test]
    fn parity_fixing_reduces_odd_vertices() {
        // Star graph: center connected to 4 leaves.
        // Center has degree 4 (even), each leaf has degree 1 (odd) → 4 odd vertices.
        let mut g = UnGraph::<(), f64>::new_undirected();
        let center = g.add_node(());
        let l1 = g.add_node(());
        let l2 = g.add_node(());
        let l3 = g.add_node(());
        let l4 = g.add_node(());
        g.add_edge(center, l1, 1.0);
        g.add_edge(center, l2, 2.0);
        g.add_edge(center, l3, 3.0);
        g.add_edge(center, l4, 4.0);

        let node_coords = vec![
            geo::Coord { x: 0.0, y: 0.0 },  // center
            geo::Coord { x: 1.0, y: 0.0 },  // l1
            geo::Coord { x: -1.0, y: 0.0 }, // l2
            geo::Coord { x: 0.0, y: 1.0 },  // l3
            geo::Coord { x: 0.0, y: -1.0 }, // l4
        ];

        fix_parity(&mut g, &node_coords).unwrap();
        let odd = odd_degree_vertices(&g);
        assert!(
            odd.len() <= 2,
            "after parity fix, should have <= 2 odd vertices, got {}",
            odd.len(),
        );
    }

    /// The MST connection between two polylines should split a segment
    /// when the optimal point is in the segment interior (not at an
    /// endpoint).
    #[test]
    fn interior_segment_split() {
        // Polyline A: long horizontal segment from (0,0) to (100,0).
        let a = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(100.0, 0.0)]);
        // Polyline B: short vertical segment above the midpoint of A.
        let b = Polyline::new(vec![Point::new(50.0, 5.0), Point::new(50.0, 10.0)]);

        let result = join_mst(&[a, b], TEST_K, TEST_RESOLUTION);
        let pts = result.points();
        assert!(
            pts.len() >= 4,
            "expected at least 4 points (A split + B), got {}",
            pts.len(),
        );

        // The output should contain a point near (50, 0) — the split point
        // on A's segment — that is NOT one of A's original endpoints.
        let has_interior_split = pts.iter().any(|p| {
            let near_x50 = (p.x - 50.0).abs() < 1.0;
            let near_y0 = p.y.abs() < 1.0;
            let not_endpoint = (p.x - 0.0).abs() > 1.0 && (p.x - 100.0).abs() > 1.0;
            near_x50 && near_y0 && not_endpoint
        });
        assert!(
            has_interior_split,
            "expected a split point near (50, 0) in the output; got: {:?}",
            pts.iter().map(|p| (p.x, p.y)).collect::<Vec<_>>(),
        );

        // Original endpoints should still be present.
        let has_origin = pts.iter().any(|p| p.x.abs() < 1e-6 && p.y.abs() < 1e-6);
        let has_end = pts
            .iter()
            .any(|p| (p.x - 100.0).abs() < 1e-6 && p.y.abs() < 1e-6);
        assert!(has_origin, "original endpoint (0,0) missing from output");
        assert!(has_end, "original endpoint (100,0) missing from output");
    }

    /// When R-tree K-nearest candidates fail to connect all components,
    /// the brute-force fallback should still produce a valid path.
    #[test]
    fn brute_force_fallback_produces_valid_path() {
        // Two nearby polylines and one very distant one.  With k=1 and
        // sparse sampling, the R-tree candidates are unlikely to bridge
        // the huge gap, forcing the brute-force fallback.
        let close_a = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0)]);
        let close_b = Polyline::new(vec![Point::new(0.0, 5.0), Point::new(10.0, 5.0)]);
        let far = Polyline::new(vec![
            Point::new(10000.0, 10000.0),
            Point::new(10010.0, 10000.0),
        ]);

        let result = join_mst(
            &[close_a.clone(), close_b.clone(), far.clone()],
            1,
            TEST_RESOLUTION,
        );
        let pts = result.points();

        // All input points should be covered.
        let total_input_pts = close_a.len() + close_b.len() + far.len();
        assert!(
            pts.len() >= total_input_pts,
            "output should cover all input points: got {} but expected >= {}",
            pts.len(),
            total_input_pts,
        );

        // Path should be continuous (already tested elsewhere, but
        // verify finiteness as a smoke check).
        for p in pts {
            assert!(
                p.x.is_finite() && p.y.is_finite(),
                "non-finite point: {p:?}"
            );
        }

        // All three original polylines' endpoints should appear.
        let check_endpoint = |x: f64, y: f64| {
            pts.iter()
                .any(|p| (p.x - x).abs() < 1.0 && (p.y - y).abs() < 1.0)
        };
        assert!(check_endpoint(0.0, 0.0), "close_a start missing");
        assert!(check_endpoint(10.0, 0.0), "close_a end missing");
        assert!(check_endpoint(0.0, 5.0), "close_b start missing");
        assert!(check_endpoint(10.0, 5.0), "close_b end missing");
        assert!(check_endpoint(10000.0, 10000.0), "far start missing");
        assert!(check_endpoint(10010.0, 10000.0), "far end missing");
    }
}
