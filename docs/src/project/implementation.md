# Implementation

## Phase 0: Project Setup

- [x] Install Dioxus CLI, verify `dx new` works
- [x] Scaffold project with `dx new`, restructure into Cargo workspace
- [x] Create workspace `Cargo.toml` with centralized deps and lint config
- [x] Create crate structure: `mujou-pipeline`, `mujou-export`, `mujou-io`, `mujou-app`
- [x] Create `rust-toolchain.toml`, `rustfmt.toml`, `.gitignore`
- [x] Create `Dioxus.toml` pointing to the `mujou-app` binary crate
- [x] Set up Tailwind CSS (`tailwind.css`, asset pipeline)
- [x] Add `LICENSE-MIT`, `LICENSE-APACHE`
- [x] Verify `dx serve --platform web` shows a basic page

## Phase 1: Core Pipeline (mujou-pipeline)

- [x] Define shared types: `Point`, `Polyline`, `PipelineConfig`, `PipelineError`
- [x] Define strategy traits: `ContourTracer`, `PathJoiner` (see [principles](principles.md#pluggable-algorithm-strategies))
- [x] Implement `grayscale.rs` -- image bytes to `GrayImage`
- [x] Implement `blur.rs` -- wrap `gaussian_blur_f32`
- [x] Implement `edge.rs` -- wrap `canny`
- [x] Implement `contour.rs` -- `ContourTracer` trait + `BorderFollowing` impl via `find_contours`
- [x] Implement `simplify.rs` -- Ramer-Douglas-Peucker from scratch
- [x] Implement `optimize.rs` -- nearest-neighbor contour ordering with direction reversal
- [x] Implement `join.rs` -- `PathJoiner` trait + `StraightLineJoin` impl
- [x] Implement `mask.rs` -- circular mask / crop
- [x] Implement top-level `process()` function (returns single `Polyline`)
- [x] Write unit tests for each module

## Phase 2: Export Formats (mujou-export)

- [x] Implement `svg.rs` -- polylines to SVG string
- [x] Implement `thr.rs` -- XY to theta-rho with continuous theta unwinding
- [ ] Implement `gcode.rs` -- polylines to G0/G1 commands
- [ ] Implement `dxf.rs` -- polylines to minimal DXF
- [ ] Implement `png.rs` -- rasterize polylines to PNG bytes
- [x] Write unit tests for each serializer

## Phase 3: Minimal UI (mujou-io + mujou-app)

- [x] Build `upload.rs` component -- file input + drag-drop zone
- [x] Build `preview.rs` component -- render polylines as inline SVG
- [x] Wire up: upload -> decode -> process -> display SVG preview
- [x] Build `export.rs` component -- format buttons + Blob downloads
- [x] Verify end-to-end: upload image -> see traced paths -> download .thr

## Phase 4: Parameter Controls

- [x] Build `controls.rs` -- sliders for all pipeline parameters
- [x] Wire sliders to `PipelineConfig` signal, re-run pipeline on change
- [x] Add circular mask toggle with radius slider
- [x] Add invert toggle
- [x] Add preview mode toggle (original / edges / paths / paths only)
- [x] Add loading indicator during processing

## Phase 5: Polish

- [x] Responsive layout for mobile browsers
- [x] Error handling -- invalid formats, oversized images, processing failures
- [x] Set up pre-commit hooks
- [x] Coverage setup and reporting
- [x] Performance testing with large images, add downsampling if needed
- [x] Deploy as static site (`dx build --platform web`)

## Phase 6: Future (Not MVP)

- [x] Web worker offloading for heavy processing
- [ ] 2-opt path optimization improvement
- [ ] Desktop and mobile builds
- [x] CI/CD pipeline (GitHub Actions)
- [ ] Configurable G-code headers and export options
- [ ] Spiral-in/spiral-out path generation for .thr
