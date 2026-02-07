# Principles

## Sans-IO Design

The project follows **full sans-IO design principles** to maximize testability.

### Core Principles

1. **Core crates have zero I/O dependencies** -- no `web-sys`, no `dioxus`, no async, no filesystem
2. **Core crates CAN have pure computation dependencies** -- `image`, `imageproc`, `serde`, `thiserror` are allowed
3. **Pure functions over side effects in core** -- image bytes in, polylines out; polylines in, format string out
4. **I/O crates handle all platform interaction** -- file uploads, downloads, DOM rendering, canvas
5. **100% testable without a browser** -- core logic tested with deterministic inputs, no DOM or WASM runtime needed

### Example Pattern

```rust
// Core crate (mujou-pipeline) - pure logic, no I/O
pub fn process(image_bytes: &[u8], config: &PipelineConfig) -> Result<Polyline, PipelineError> {
    let img = decode_image(image_bytes)?;
    let gray = to_grayscale(&img);
    let blurred = gaussian_blur(&gray, config.blur_sigma);
    let edges = canny(&blurred, config.canny_low, config.canny_high);
    let contours = config.contour_tracer.trace(&edges);
    let simplified = simplify_paths(&contours, config.simplify_tolerance);
    let optimized = optimize_path_order(&simplified);
    let joined = config.path_joiner.join(&optimized);
    Ok(joined)
}

// Core crate (mujou-export) - pure serialization, no I/O
pub fn to_thr(path: &Polyline, config: &ThrConfig) -> String {
    // Pure function: single continuous path -> theta-rho text
}

// IO crate (mujou-io) - browser interaction
fn trigger_download(content: &str, filename: &str, mime_type: &str) {
    // web-sys Blob + object URL + <a> click
}
```

### Layer Boundaries

| Layer | Crates | I/O Allowed? | Async Allowed? |
| ----- | ------ | ------------ | -------------- |
| Core | `mujou-pipeline`, `mujou-export` | No | No |
| Integration | `mujou-io` | Yes | Yes |
| Application | `mujou` | Yes | Yes |

## Pluggable Algorithm Strategies

When a pipeline step has multiple viable algorithms, design it as a **user-selectable strategy** rather than hardcoding a single approach.

### Rationale

Different images, output devices, and aesthetic preferences favor different algorithms.
Rather than picking one algorithm and hoping it works for all cases, expose the choice to the user and make it easy to add new strategies over time.

### Guidelines

1. **Define the step by its inputs and outputs, not its algorithm.** Each pipeline step has a trait that specifies the type signature (e.g., binary edge map in, polylines out). Any implementation that satisfies that trait is a valid strategy.
2. **Ship with one strategy, design for many.** MVP can launch with a single implementation per step. The architecture should make adding a second strategy a small, isolated change -- implement the trait on a new struct, wire it to the UI.
3. **Each strategy is a pure function.** Strategies live in the core layer with no I/O dependencies. This makes them independently testable with synthetic inputs.
4. **User selects via UI.** The UI exposes strategy choices as dropdowns or radio buttons. The `PipelineConfig` stores the user's selection and the pipeline dispatches to the corresponding trait implementation.
5. **Document tradeoffs per strategy.** Each strategy's doc comment or documentation entry should state what it's good at, what it's bad at, and when to prefer it.

### Example

```rust
/// Trait for contour tracing strategies.
/// Input: binary edge map. Output: disconnected polylines.
trait ContourTracer {
    fn trace(&self, edges: &GrayImage) -> Vec<Polyline>;
}

/// Suzuki-Abe border following via imageproc::contours::find_contours.
/// Fast, zero custom code. Doubles borders on 1px-wide edges;
/// relies on RDP simplification to collapse the doubling.
struct BorderFollowing;

impl ContourTracer for BorderFollowing {
    fn trace(&self, edges: &GrayImage) -> Vec<Polyline> {
        // ...
    }
}

/// Marching squares isoline extraction.
/// Produces single centerline paths at sub-pixel precision.
/// Better geometry, more custom code.
struct MarchingSquares;

impl ContourTracer for MarchingSquares {
    fn trace(&self, edges: &GrayImage) -> Vec<Polyline> {
        // ...
    }
}

/// Trait for path joining strategies.
/// Input: ordered disconnected contours. Output: single continuous path.
trait PathJoiner {
    fn join(&self, contours: &[Polyline]) -> Polyline;
}

/// Connect end of each contour to start of next with a straight line.
/// Simple, minimal code. Visible scratches between features.
struct StraightLineJoin;

impl PathJoiner for StraightLineJoin {
    fn join(&self, contours: &[Polyline]) -> Polyline {
        // ...
    }
}
```

### Current strategy points

These pipeline steps are designed as pluggable strategies:

| Step | Trait | MVP implementation | Future candidates |
| ---- | ----- | ------------------ | ----------------- |
| Contour tracing | `ContourTracer` | `BorderFollowing` (Suzuki-Abe via `imageproc`) | `MarchingSquares` |
| Path joining | `PathJoiner` | `StraightLineJoin` | `RetraceJoin`, `EdgeAwareJoin`, `SpiralJoin` (polar) |

As the project matures, other pipeline steps may benefit from the same pattern (e.g., edge detection algorithms, simplification algorithms, path optimization heuristics).

## Dependencies Policy

### Core Crates (sans-IO)

**Allowed:**

- `image` (pixel buffer types and decoding)
- `imageproc` (image processing algorithms, with `default-features = false`)
- `serde` (serialization)
- `thiserror` (error types)
- Pure computation crates

**Forbidden:**

- `dioxus` or any UI framework
- `web-sys`, `js-sys`, `wasm-bindgen`
- Any async runtime
- File system access
- Network access
- DOM interaction

### I/O Crates

**Allowed:**

- `dioxus`
- `web-sys`, `js-sys`, `wasm-bindgen`
- Browser APIs (file input, Blob, canvas)
- Platform-specific crates behind `#[cfg]` gates

## Testing Philosophy

**Target 100% coverage** with explicit exclusions for untestable code.
The sans-IO architecture makes this achievable for core crates -- all image processing and format serialization is pure functions testable with synthetic inputs.

Core crate tests require only `cargo test` -- no browser, no WASM runtime, no DOM.
