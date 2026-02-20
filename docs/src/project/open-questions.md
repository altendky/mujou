# Open Questions

## Pending

- [ ] WebP decoding in WASM -- Does the `image` crate's WebP decoder work in `wasm32-unknown-unknown`? May need to limit input formats to PNG/JPEG/BMP if not.
- [x] Maximum image size / working resolution -- Decided: downsample to ~256px on the long axis early in the pipeline. Based on reference target device analysis (34" table, ~5mm track width, ~170 resolvable lines). See [Decisions](decisions.md#reference-target-device).
- [x] Contour tracing suitability -- Decided: design as a [pluggable algorithm strategy](principles.md#pluggable-algorithm-strategies) via the `ContourTracer` trait. MVP ships with `BorderFollowing` (Suzuki-Abe via `imageproc`). On 1px-wide Canny edges this produces doubled borders that RDP collapses in practice (same approach as Image2Sand). `MarchingSquares` is a deferred alternative for cleaner single-line geometry. See [Pipeline](pipeline.md#5-contour-tracing).
- [x] Coordinate system and normalized space -- Decided: center origin, +Y up, mask edge = 1.0 (unit circle for circle mask). Mask always required. `mask_scale` replaced by `zoom` (photographer's convention, default 1.25, range 0.4–3.0). Normalization happens between contour tracing and simplification. All vector-stage parameters are in normalized units. See [Decisions](decisions.md#coordinate-system-and-normalized-space) and [Principles: Coordinate System](principles.md#coordinate-system).
- [ ] Spiral in/out for .thr -- Should we generate spiral-in/out paths for sand tables that need the ball to start/end at center/edge, or is that the table firmware's responsibility? Image2Sand does not generate spirals.
- [x] Point interpolation for .thr -- Addressed by the subsample pipeline step, which breaks long segments into shorter sub-segments before export. Subsample spacing is now in normalized units. See [Pipeline](pipeline.md).
- [x] Deployment target -- Decided: GitHub Pages. Simplest option (same repo, no additional vendor), free tier sufficient, avoids platform lock-in. App served at `/app/` path with landing page at root. See [Decisions](decisions.md#deployment-target).
- [ ] Pre-commit scope -- Match onshape-mcp's full hook suite from day one, or start with a minimal set?
- [ ] CI setup -- GitHub Actions workflows, when to set up? After MVP UI is working, or earlier?
- [x] Project naming -- Decided: **mujou** (無常, impermanence), domain **mujou.art**. See [Naming](naming.md) for full exploration and reasoning.
- [ ] WASM binary size -- How large will the binary be with `image` + `imageproc`? May need `wasm-opt -Oz` and `lto = true` in release profile. Need to measure.
- [ ] `imageproc` Rust version requirement -- `imageproc` 0.26 may require Rust 1.87+ (edition 2024). Verify and pin in `rust-toolchain.toml`.
- [ ] Normalized `simplify_tolerance` default -- Need to determine the right default for `simplify_tolerance` in normalized units. The old default was 2.0px at ~256px working resolution, which is roughly 2.0/128 ≈ 0.016 normalized (half the shorter dimension). Needs validation with real images at different working resolutions.
- [ ] Normalized `subsample_max_length` default -- Same concern as `simplify_tolerance`. Old default was 2.0px. Need to validate the normalized equivalent.
- [ ] Physical size awareness in pipeline -- Currently physical dimensions (mm) are only known at the export boundary. Future features (device-specific optimization, physical track width awareness) may require physical size awareness earlier in the pipeline. Deferred until a concrete feature needs it.

## Deferred

Items to address after MVP:

### Performance

- [x] Web worker offloading -- Pipeline processing runs in a dedicated web worker with cancel support and elapsed time indicator (see #47)
- [ ] Coarse-then-fine processing -- Run a low-res pass to identify edge regions, then mask the fine-res pass to only process those regions (~1% of pixels are edges). Avoids full-image high-res cost while preserving positional precision for smooth curves on large tables. Evaluate if 256px MVP produces visible staircase artifacts. See [Decisions](decisions.md#reference-target-device).
- [ ] SIMD acceleration -- Enable `wasm32-simd128` target feature for faster image processing
- [x] Image downsampling -- Decided: always downsample to ~256px working resolution after decode. See [Decisions](decisions.md#reference-target-device)

### Validation

- [ ] `PipelineConfig` validated constructor -- Add `try_new()` (or a builder) that enforces invariants (`blur_sigma > 0`, `canny_low <= canny_high`, `0.4 <= zoom <= 3.0`, `1.0 <= mask_aspect_ratio <= 4.0`, `simplify_tolerance >= 0.0`, `subsample_max_length > 0.0`), make fields private, add getters, and return `PipelineError::InvalidConfig` on failure. See [PR #2 discussion](https://github.com/altendky/mujou/pull/2#discussion_r2778003093).

### Architecture

- [ ] Shared types crate extraction -- `mujou-export` currently depends on `mujou-pipeline` to access shared types (`Point`, `Polyline`, `Dimensions`). Consider extracting these into a `mujou-types` crate to avoid coupling the export layer to the pipeline layer. Evaluate if the coupling causes problems as more export formats are added.

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
- [ ] Auto-deploy on merge to main -- Currently deploy is manual (`workflow_dispatch`). Consider triggering on push to `main` once the workflow is proven reliable. If enabled, consider whether deploy should be gated on CI passing (via `workflow_run` trigger or a combined workflow) to prevent deploying broken builds.
- [ ] PR preview deploys -- GitHub Pages does not support deploy previews from PRs natively. Options include external services (surge.sh, Cloudflare Pages for previews only), downloadable build artifacts for manual review, or no previews (rely on local `dx serve`). Revisit if reviewing UI changes from PRs becomes painful.
