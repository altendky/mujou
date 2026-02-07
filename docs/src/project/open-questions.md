# Open Questions

## Pending

- [ ] WebP decoding in WASM -- Does the `image` crate's WebP decoder work in `wasm32-unknown-unknown`? May need to limit input formats to PNG/JPEG/BMP if not.
- [ ] Maximum image size -- What's a reasonable size limit before auto-downsampling? 4MP? 8MP? Needs performance testing in WASM.
- [x] Contour tracing suitability -- Decided: design as a [pluggable algorithm strategy](principles.md#pluggable-algorithm-strategies) via the `ContourTracer` trait. MVP ships with `BorderFollowing` (Suzuki-Abe via `imageproc`). On 1px-wide Canny edges this produces doubled borders that RDP collapses in practice (same approach as Image2Sand). `MarchingSquares` is a deferred alternative for cleaner single-line geometry. See [Pipeline](pipeline.md#5-contour-tracing).
- [ ] Spiral in/out for .thr -- Should we generate spiral-in/out paths for sand tables that need the ball to start/end at center/edge, or is that the table firmware's responsibility? Image2Sand does not generate spirals.
- [ ] Point interpolation for .thr -- Image2Sand interpolates additional points along segments for smoother polar coordinate conversion. Do we need this, or is the point density from contour tracing sufficient?
- [ ] Deployment target -- GitHub Pages, Cloudflare Pages, or Netlify? All support static sites. GitHub Pages is simplest (same repo), Cloudflare is fastest.
- [ ] Pre-commit scope -- Match onshape-mcp's full hook suite from day one, or start with a minimal set?
- [ ] CI setup -- GitHub Actions workflows, when to set up? After MVP UI is working, or earlier?
- [x] Project naming -- Decided: **mujou** (無常, impermanence), domain **mujou.art**. See [Naming](naming.md) for full exploration and reasoning.
- [ ] WASM binary size -- How large will the binary be with `image` + `imageproc`? May need `wasm-opt -Oz` and `lto = true` in release profile. Need to measure.
- [ ] `imageproc` Rust version requirement -- `imageproc` 0.26 may require Rust 1.87+ (edition 2024). Verify and pin in `rust-toolchain.toml`.

## Deferred

Items to address after MVP:

### Performance

- [ ] Web worker offloading -- Move pipeline processing off the main thread to prevent UI blocking
- [ ] SIMD acceleration -- Enable `wasm32-simd128` target feature for faster image processing
- [ ] Image downsampling -- Auto-downsample images above a size threshold before processing

### Features

- [ ] `MarchingSquares` contour tracer -- New `ContourTracer` impl using marching squares isoline extraction. Produces single centerline paths at sub-pixel precision instead of doubled borders. Cleaner geometry without relying on RDP to collapse border doubling, more natural handling of open vs closed paths. ~80-120 lines custom code. `imageproc` does not provide this.
- [ ] Additional `PathJoiner` implementations -- `RetraceJoin` (backtrack along previous contour to shorten jumps), `EdgeAwareJoin` (route connections along Canny edges via A*), `SpiralJoin` (polar spiral arcs for .thr output).
- [ ] 2-opt path optimization -- Improve on nearest-neighbor TSP with local search
- [ ] Spiral-in/out generation -- Add entry/exit spirals to .thr output
- [ ] Additional G-code options -- Configurable headers, homing commands, coordinate offsets
- [ ] Desktop build -- Dioxus desktop target for native app
- [ ] Mobile build -- Dioxus Android/iOS targets

### Infrastructure

- [ ] GitHub Actions CI -- Linting, testing, WASM build, deployment
- [ ] Release workflow -- Automated static site deployment on tag
- [ ] Coverage reporting -- Codecov integration
