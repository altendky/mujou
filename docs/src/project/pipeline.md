# Image Processing Pipeline

The pipeline is format-agnostic.
The internal representation is XY polylines (`Vec<Polyline>` where `Polyline = Vec<Point>` and `Point = (f64, f64)`).
Export serializers convert this to each output format.

All pipeline code lives in the `mujou-pipeline` crate (core layer, pure Rust, no I/O).

## Processing Steps

### 1. Decode Image

Accept common raster formats: PNG, JPEG, BMP, WebP.
Use the `image` crate to decode raw bytes into an `RgbaImage` pixel buffer.

### 2. Convert to Grayscale

Convert the RGBA image to a single-channel grayscale image.
Standard luminance formula: `0.299*R + 0.587*G + 0.114*B`.

### 3. Gaussian Blur

Smooth the grayscale image to reduce noise before edge detection.
Use `imageproc::filter::gaussian_blur_f32(image, sigma)`.

**User parameter:** `blur_sigma` (f32, default: 1.4)

### 4. Canny Edge Detection

Detect edges using `imageproc::edges::canny(image, low_threshold, high_threshold)`.
Returns a binary image (255 for edge pixels, 0 for non-edge).

Internally, Canny performs:

1. Sobel gradient computation (X and Y)
2. Non-maximum suppression
3. Hysteresis thresholding -- pixels above `high_threshold` are definite edges; pixels between `low_threshold` and `high_threshold` are edges only if connected to a definite edge

**User parameters:**

- `canny_low` (f32, default: 50.0)
- `canny_high` (f32, default: 150.0)

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

### 7. Path Optimization

Minimize travel distance between disconnected contours.
On a sand table, travel between contours draws visible lines, so minimizing this improves output quality.

**Strategy: Nearest-neighbor TSP on contours**

1. For each pair of contours, compute four distances: start-to-start, start-to-end, end-to-start, end-to-end.
   Use the minimum.
2. Starting from contour 0, greedily visit the nearest unvisited contour.
3. For each contour, choose the direction (forward or reversed) that minimizes the gap from the previous contour's endpoint.

This is a greedy heuristic, not optimal.
2-opt improvement is a potential future enhancement.

### 8. Path Joining

Sand tables cannot lift the ball -- every movement draws a visible line.
The output must be a **single continuous path**, not a set of disconnected contours.
This step flattens the ordered `Vec<Polyline>` from step 7 into one continuous `Polyline` by inserting connecting segments between each contour.

This is a [pluggable algorithm strategy](principles.md#pluggable-algorithm-strategies) -- the user selects which joining method to use.

**User parameter:** `path_joiner` (impl `PathJoiner`, default: `StraightLineJoin`)

#### StraightLine (MVP)

Connect the end of each contour to the start of the next with a straight line segment.

**Tradeoffs:** Simplest (~10 lines). Produces visible straight scratches between features. Scratch length is minimized by the path optimization in step 7 but not eliminated.

#### Retrace (future)

Retrace backward along the previous contour before jumping to the next.
The retraced segment follows an already-drawn groove, so it is invisible in sand.
The remaining straight-line jump starts from a point closer to the next contour.

**Tradeoffs:** Longer total path length but less visible artifact. Requires computing the optimal backtrack distance.

#### EdgeAwareRouting (future)

Route connecting segments along edges from the Canny output, so connections follow features in the image and blend in visually.

**Tradeoffs:** Connections look intentional. Requires pathfinding (A* or similar) on the edge map. Significantly more complex.

#### Spiral (future, polar tables only)

For .thr output on circular tables, connect via short spiral arcs in polar coordinate space.
Spirals are the natural visual language of polar sand tables.

**Tradeoffs:** Only applicable to polar output formats. Requires theta-rho space path planning.

### 9. Circular Mask (Optional)

For round sand tables (Sisyphus, Oasis Mini), clip all polylines to a circle centered on the image.
Points outside the circle are removed.
Polylines that cross the circle boundary are split at the intersection.

**User parameters:**

- `circular_mask` (bool, default: false)
- `mask_diameter` (f64, fraction of image width, default: 1.0)

### 10. Invert (Optional)

By default, edges (high contrast boundaries) are traced.
Inversion swaps the binary edge map so dark regions are traced instead of light-to-dark transitions.

**User parameter:** `invert` (bool, default: false)

## User-Tunable Parameters Summary

| Parameter | Type | Default | Description |
| --------- | ---- | ------- | ----------- |
| `blur_sigma` | f32 | 1.4 | Gaussian blur kernel sigma |
| `canny_low` | f32 | 50.0 | Canny low threshold |
| `canny_high` | f32 | 150.0 | Canny high threshold |
| `contour_tracer` | `ContourTracer` | `BorderFollowing` | Contour tracing algorithm ([strategy](principles.md#pluggable-algorithm-strategies)) |
| `simplify_tolerance` | f64 | 2.0 | RDP simplification tolerance (pixels) |
| `path_joiner` | `PathJoiner` | `StraightLineJoin` | Path joining method ([strategy](principles.md#pluggable-algorithm-strategies)) |
| `circular_mask` | bool | false | Clip output to circle |
| `mask_diameter` | f64 | 1.0 | Mask diameter as fraction of image width |
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
