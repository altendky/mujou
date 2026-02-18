# Image Processing Pipeline

The pipeline is format-agnostic.
The internal representation is XY polylines (`Vec<Polyline>` where `Polyline = Vec<Point>` and `Point = (f64, f64)`).
Export serializers convert this to each output format.

All pipeline code lives in the `mujou-pipeline` crate (core layer, pure Rust, no I/O).

## Processing Steps

### 1. Decode Image

Accept common raster formats: PNG, JPEG, BMP, WebP.
Use the `image` crate to decode raw bytes into an `RgbaImage` pixel buffer.

### 2. Downsample

Resize the image so the longest axis matches `working_resolution`.
All subsequent pipeline stages operate at this reduced resolution.

**User parameters:**

- `working_resolution` (u32, default: 1000)
- `downsample_filter` (`DownsampleFilter`, default: `Triangle`)

### 3. Gaussian Blur

Smooth the RGBA image to reduce noise before edge detection.
Each R/G/B/A channel is blurred independently using `imageproc::filter::gaussian_blur_f32(channel, sigma)`.

Operating on the full RGBA image means the blur preview in the UI shows color (not grayscale), and downstream edge detection can extract already-blurred channels without redundant per-channel blurring. Mathematically, blurring each channel independently then extracting a derived channel (e.g. luminance) is equivalent to extracting the channel first then blurring, since Gaussian blur is a linear per-channel operation.

**User parameter:** `blur_sigma` (f32, default: 1.4)

### 4. Canny Edge Detection

Detect edges using Canny on one or more image channels, combining results via pixel-wise maximum.

By default, edge detection runs on the luminance (grayscale) channel only. The user can enable additional channels to capture edges that luminance alone misses -- for example, hue boundaries where color changes but brightness stays similar.

#### Edge channels

Canny runs independently on each enabled channel. The per-channel edge maps are combined via pixel-wise maximum, so edges detected in *any* enabled channel appear in the final edge map.

| Channel | Source | Default | Notes |
| --- | --- | --- | --- |
| Luminance | sRGB/Rec.709 grayscale | **on** | Standard luminance, works well for most images |
| Red | R from RGBA | off | Skin appears bright; useful for skin/lip boundaries |
| Green | G from RGBA | off | Most similar to luminance; captures overall detail |
| Blue | B from RGBA | off | Skin appears dark; tends to be noisier |
| Saturation | S from HSV | off | Highlights hue boundaries (lips, colored clothing) |

All channels are extracted from the already-blurred RGBA image (step 3), so no additional per-channel blurring is needed.

See [#96](https://github.com/altendky/mujou/issues/96) for planned future channels (Hue, Value, Lab).

#### Canny internals

Internally, Canny performs:

1. Sobel gradient computation (X and Y)
2. Non-maximum suppression
3. Hysteresis thresholding -- pixels above `high_threshold` are definite edges; pixels between `low_threshold` and `high_threshold` are edges only if connected to a definite edge

**User parameters:**

- `edge_channels` (`EdgeChannels`, default: luminance only)
- `canny_low` (f32, default: 15.0)
- `canny_high` (f32, default: 40.0)

Maximum sensible threshold is approximately 1140.39 (`sqrt(5) * 2 * 255`).

### 5. Contour Tracing

Extract polylines from the binary edge map.
This is a [pluggable algorithm strategy](principles.md#pluggable-algorithm-strategies) -- the user selects which tracing algorithm to use.

**User parameter:** `contour_tracer` (impl `ContourTracer`, default: `BorderFollowing`)

#### BorderFollowing (MVP)

Uses `imageproc::contours::find_contours(image)` which implements Suzuki-Abe border following.
Returns `Vec<Contour<u32>>` with border type information (outer vs hole).
Convert contour points to `Vec<Polyline>` in floating-point image coordinates.

On 1-pixel-wide Canny edges, Suzuki-Abe traces both sides of each edge pixel strip, producing doubled borders at integer coordinates.
RDP simplification (step 6) collapses this doubling in practice.
Image2Sand uses the same approach (OpenCV's `findContours`).

**Tradeoffs:** Zero custom code (library call + type conversion glue). Doubled borders on thin edges rely on RDP to clean up. All contours returned as closed loops even if the underlying edge is open.

#### MarchingSquares (future)

Marching squares isoline extraction at sub-pixel precision.
Traces the boundary between black and white pixels, producing a single centerline path rather than a doubled border.

**Tradeoffs:** ~80-120 lines custom code. Cleaner single-line geometry without relying on RDP to collapse doubling. More naturally handles open vs closed paths. Not provided by `imageproc`.

### 6. Path Simplification (Optional)

Reduce point count using Ramer-Douglas-Peucker (RDP) algorithm.
This is implemented from scratch (~30 lines) to avoid pulling in the `geo` crate dependency tree.

The algorithm recursively finds the point farthest from the line between the first and last points of a segment.
If that distance exceeds the tolerance, the segment is split and both halves are processed.
Otherwise, intermediate points are dropped.

**User parameter:** `simplify_tolerance` (f64, default: 2.0 pixels)

### 7. Mask (Optional)

Clip all polylines to a mask shape centered on the image.
Points outside the mask are removed.
Polylines that cross the mask boundary are split at the intersection.
Contours entirely outside the mask are discarded before joining, so the join step only connects surviving contours.

Two mask shapes are supported:

- **Circle** — for round sand tables (Sisyphus, Oasis Mini). `mask_scale` is a fraction of the image diagonal; at 1.0 the circle circumscribes the full image.
- **Rectangle** — axis-aligned rectangle. `mask_scale` controls the shorter dimension relative to the image's shorter dimension. `mask_aspect_ratio` extends the longer dimension. `mask_landscape` controls orientation.

The mask stage returns a `MaskResult` containing `Vec<ClippedPolyline>` with explicit per-endpoint clip metadata (`start_clipped`, `end_clipped`) identifying every point that was created by intersection with the mask boundary.

#### Border path

When clipping creates boundary endpoints, the joiner may connect them across open space near the edge, producing visually jarring artifacts. The `border_path` option adds a border polyline matching the mask shape (a circle sampled at ~3px arc-length spacing, or a closed 4-corner rectangle). This gives the joiner a path along the mask boundary so connections between boundary endpoints route along the edge rather than cutting across empty space.

Three modes:

| Mode | Behaviour |
| ---- | --------- |
| `Auto` (default) | Add the border polyline only when clipping actually intersects at least one polyline endpoint |
| `On` | Always add the border polyline when the mask is enabled |
| `Off` | Never add a border polyline |

The border shape is tied to the mask shape via the `MaskShape` enum — each shape variant implements both clipping and border generation, enforced by exhaustive `match` arms.

**User parameters:**

- `mask_mode` (`MaskMode`, default: `Circle`) — `Off`, `Circle`, or `Rectangle`
- `mask_scale` (f64, 0.0-1.5, default: 0.75) — scale factor for the mask shape
- `mask_aspect_ratio` (f64, 1.0-4.0, default: 1.0) — rectangle aspect ratio (only for Rectangle)
- `mask_landscape` (bool, default: true) — rectangle orientation (only for Rectangle)
- `border_path` (`BorderPathMode`, default: `Auto`)

### 8. Path Ordering + Joining

Sand tables cannot lift the ball -- every movement draws a visible line.
The output must be a **single continuous path**, not a set of disconnected contours.
This step receives contours from masking (if enabled) or simplification, and produces a single continuous `Polyline`. Each joining strategy handles its own ordering internally, which allows strategies like Retrace to integrate ordering decisions with backtracking capabilities.

This is a [pluggable algorithm strategy](principles.md#pluggable-algorithm-strategies) -- the user selects which joining method to use.

**User parameter:** `path_joiner` (impl `PathJoiner`, default: `Mst`)

#### Mst (default)

MST-based segment-to-segment join algorithm. Finds globally optimal connections between polyline components via a minimum spanning tree, minimizing total new connecting segment length (the only visible artifacts on sand).

Algorithm phases:

1. **MST via Kruskal:** Insert all polyline segments into an R\*-tree spatial index (`rstar`). Sample points along each polyline at adaptive spacing and query the R-tree for K nearest cross-component segments to generate candidate edges with exact segment-to-segment distance (`geo::Euclidean`). Sort candidates by distance and merge via `petgraph::UnionFind` (Kruskal's algorithm). When a connection point falls in the interior of a segment, that segment is split at the connection point.
2. **Fix parity:** Count odd-degree vertices. Pair odd vertices and duplicate the shortest path between each pair (Dijkstra). Duplicated edges represent retracing through already-drawn grooves (visually free). The pairing algorithm is controlled by `parity_strategy`: `Greedy` (default) pairs by nearest Euclidean distance; `Optimal` uses minimum-weight perfect matching via DP over bitmasks for small vertex counts (n <= 20) or a best-of-two heuristic for larger counts.
3. **Hierholzer:** Find an Eulerian path through the augmented graph (original edges + MST connecting edges + duplicated retrace edges).
4. **Emit:** Convert the vertex sequence to a `Polyline`.

**User parameter:** `parity_strategy` (enum `ParityStrategy`, default: `Greedy`)

**Tradeoffs:** Globally optimal connections (MST) instead of greedy ordering. Segment-to-segment distances find truly closest points between polylines (not just sampled vertices). Interior joins are supported. Produces significantly fewer visible artifacts and shorter new connecting segments than both `StraightLine` and `Retrace`.

#### StraightLine

Nearest-neighbor ordering followed by straight-line concatenation. Internally calls `optimize_path_order()` (greedy nearest-neighbor TSP on contour endpoints), then connects the end of each contour to the start of the next with a straight line segment.

**Tradeoffs:** Simplest strategy. Produces visible straight scratches between features. Scratch length is minimized by the internal path optimization but not eliminated.

#### Retrace

Full-history retrace with integrated contour ordering. Implements a retrace-aware greedy nearest-neighbor algorithm:

1. Start with all contours in a candidate pool.
2. Pick contour 0, emit its points into the output path.
3. While candidates remain:
   a. For each candidate, for each orientation (forward/reversed), find the point in the **entire output path history** closest to the candidate's entry point.
   b. Pick the combination with the smallest distance.
   c. Retrace backward through the drawn path to the closest history point (these segments follow already-drawn grooves -- invisible in sand).
   d. Emit the chosen contour's points (reversed if needed).

Any previously visited point is reachable at zero visible cost. This means the algorithm can exploit proximity to points visited many contours ago, which the separate optimize-then-join approach cannot.

**Performance:** Brute-force O(N^2 x M) where N = contour count, M = avg points per contour. For typical images (~200 contours, ~50 pts each) this is ~2x10^8 distance computations. Spatial indexing can be added later for complex images.

**Tradeoffs:** Longer total path length but significantly fewer visible artifacts. Integrated ordering eliminates the structural limitation where optimization ignores backtracking capability.

#### EdgeAwareRouting (future)

Route connecting segments along edges from the Canny output, so connections follow features in the image and blend in visually.

**Tradeoffs:** Connections look intentional. Requires pathfinding (A* or similar) on the edge map. Significantly more complex.

#### Spiral (future, polar tables only)

For .thr output on circular tables, connect via short spiral arcs in polar coordinate space.
Spirals are the natural visual language of polar sand tables.

**Tradeoffs:** Only applicable to polar output formats. Requires theta-rho space path planning.

### 9. Invert (Optional)

By default, edges (high contrast boundaries) are traced.
Inversion swaps the binary edge map so dark regions are traced instead of light-to-dark transitions.

**User parameter:** `invert` (bool, default: false)

## User-Tunable Parameters Summary

| Parameter | Type | Default | Description |
| --------- | ---- | ------- | ----------- |
| `blur_sigma` | f32 | 1.4 | Gaussian blur kernel sigma |
| `edge_channels` | `EdgeChannels` | luminance only | Which channels to use for edge detection (composable) |
| `canny_low` | f32 | 15.0 | Canny low threshold |
| `canny_high` | f32 | 40.0 | Canny high threshold |
| `canny_max` | f32 | 60.0 | Upper bound for Canny threshold sliders (UI only) |
| `contour_tracer` | `ContourTracer` | `BorderFollowing` | Contour tracing algorithm ([strategy](principles.md#pluggable-algorithm-strategies)) |
| `simplify_tolerance` | f64 | 2.0 | RDP simplification tolerance (pixels) |
| `path_joiner` | `PathJoiner` | `Mst` | Path joining method ([strategy](principles.md#pluggable-algorithm-strategies)) |
| `mask_mode` | `MaskMode` | `Circle` | Mask shape: `Off`, `Circle`, `Rectangle` |
| `mask_scale` | f64 | 0.75 | Scale factor for mask shape (0.0-1.5) |
| `mask_aspect_ratio` | f64 | 1.0 | Rectangle aspect ratio (1.0-4.0, Rectangle only) |
| `mask_landscape` | bool | true | Rectangle orientation (Rectangle only) |
| `border_path` | `BorderPathMode` | `Auto` | Add border polyline along mask edge (`Auto`/`On`/`Off`) |
| `invert` | bool | false | Invert edge map |

## Performance Considerations

### WASM Constraints

- Single-threaded execution (no `rayon`)
- No SIMD by default (though `wasm32-simd128` is available in modern browsers)
- Gaussian blur + Canny on a 2MP (1920x1080) image: estimated 100-500ms depending on kernel size and browser
- Contour tracing is O(n) in edge pixels, generally fast
- Memory: a 2MP RGBA image is ~8MB; grayscale ~2MB

### Mitigation Strategies

- Process on the main thread for MVP (with a loading indicator)
- Consider downsampling large images (>4MP) before processing
- Move to web workers if UI blocking proves unacceptable
- Enable `wasm32-simd128` target feature for SIMD acceleration:

```toml
# .cargo/config.toml
[target.wasm32-unknown-unknown]
rustflags = ["-C", "target-feature=+simd128"]
```

## Prior Art

- **[Image2Sand](https://orionwc.github.io/Image2Sand/)** -- JavaScript web tool using OpenCV.js for Canny + `findContours` + `approxPolyDP`.
  Uses nearest-neighbor TSP for path optimization.
  Outputs polar coordinates for CrunchLabs Sand Garden (different format than .thr).
- **[Sandify](https://sandify.org)** -- Algorithmic pattern generator (not image converter), exports .thr and .gcode.
- **[fly115/Image2Sand](https://github.com/fly115/Image2Sand)** -- Excel macro version, exports .gcode and .thr.
