# Implementation

## Phase 0: Project Setup

- [ ] Install Dioxus CLI, verify `dx new` works
- [ ] Scaffold project with `dx new`, restructure into Cargo workspace
- [ ] Create workspace `Cargo.toml` with centralized deps and lint config
- [ ] Create crate structure: `mujou-pipeline`, `mujou-export`, `mujou-io`, `mujou`
- [ ] Create `rust-toolchain.toml`, `rustfmt.toml`, `.gitignore`
- [ ] Create `Dioxus.toml` pointing to the `mujou` binary crate
- [ ] Set up Tailwind CSS (`tailwind.css`, asset pipeline)
- [ ] Add `LICENSE-MIT`, `LICENSE-APACHE`
- [ ] Verify `dx serve --platform web` shows a basic page

## Phase 1: Core Pipeline (mujou-pipeline)

- [ ] Define shared types: `Point`, `Polyline`, `PipelineConfig`, `PipelineError`
- [ ] Define strategy traits: `ContourTracer`, `PathJoiner` (see [principles](principles.md#pluggable-algorithm-strategies))
- [ ] Implement `grayscale.rs` -- image bytes to `GrayImage`
- [ ] Implement `blur.rs` -- wrap `gaussian_blur_f32`
- [ ] Implement `edge.rs` -- wrap `canny`
- [ ] Implement `contour.rs` -- `ContourTracer` trait + `BorderFollowing` impl via `find_contours`
- [ ] Implement `simplify.rs` -- Ramer-Douglas-Peucker from scratch
- [ ] Implement `optimize.rs` -- nearest-neighbor contour ordering with direction reversal
- [ ] Implement `join.rs` -- `PathJoiner` trait + `StraightLineJoin` impl
- [ ] Implement `mask.rs` -- circular mask / crop
- [ ] Implement top-level `process()` function (returns single `Polyline`)
- [ ] Write unit tests for each module

## Phase 2: Export Formats (mujou-export)

- [ ] Implement `svg.rs` -- polylines to SVG string
- [ ] Implement `thr.rs` -- XY to theta-rho with continuous theta unwinding
- [ ] Implement `gcode.rs` -- polylines to G0/G1 commands
- [ ] Implement `dxf.rs` -- polylines to minimal DXF
- [ ] Implement `png.rs` -- rasterize polylines to PNG bytes
- [ ] Write unit tests for each serializer

## Phase 3: Minimal UI (mujou-io + mujou)

- [ ] Build `upload.rs` component -- file input + drag-drop zone
- [ ] Build `preview.rs` component -- render polylines as inline SVG
- [ ] Wire up: upload -> decode -> process -> display SVG preview
- [ ] Build `export.rs` component -- format buttons + Blob downloads
- [ ] Verify end-to-end: upload image -> see traced paths -> download .thr

## Phase 4: Parameter Controls

- [ ] Build `controls.rs` -- sliders for all pipeline parameters
- [ ] Wire sliders to `PipelineConfig` signal, re-run pipeline on change
- [ ] Add circular mask toggle with radius slider
- [ ] Add invert toggle
- [ ] Add preview mode toggle (original / edges / paths / paths only)
- [ ] Add loading indicator during processing

## Phase 5: Polish

- [ ] Responsive layout for mobile browsers
- [ ] Error handling -- invalid formats, oversized images, processing failures
- [ ] Set up pre-commit hooks
- [ ] Coverage setup and reporting
- [ ] Performance testing with large images, add downsampling if needed
- [ ] Deploy as static site (`dx build --platform web`)

## Phase 6: Future (Not MVP)

- [ ] Web worker offloading for heavy processing
- [ ] 2-opt path optimization improvement
- [ ] Desktop and mobile builds
- [ ] CI/CD pipeline (GitHub Actions)
- [ ] Configurable G-code headers and export options
- [ ] Spiral-in/spiral-out path generation for .thr
