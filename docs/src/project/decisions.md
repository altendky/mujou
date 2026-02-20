# Decisions

Resolved design decisions and their rationale.

## UI Framework

**Decision:** Dioxus 0.7+ with web as primary target.

**Rationale:** Single Rust codebase for web (WASM), desktop, and mobile.
React-like RSX syntax with Tailwind CSS styling.
Web target compiles to static files (HTML + JS + WASM), hostable on GitHub Pages with zero backend.
Largest Rust GUI community, YC-backed with full-time team.
See [Requirements](requirements.md#why-dioxus-over-alternatives).

**Alternative considered:** egui -- simpler, faster to prototype, but produces developer-tool aesthetics, not consumer-facing web apps.
If Dioxus proves too painful, falling back to egui + eframe is reasonable.

## Image Processing Library

**Decision:** `imageproc` 0.26 with `default-features = false`.

**Rationale:** Provides Canny edge detection, Gaussian blur, and contour tracing out of the box.
WASM-compatible when default features (rayon, fft) are disabled.
Maintainers actively test against WASM.
See [Requirements](requirements.md#imageproc-wasm-compatibility).

## Path Simplification

**Decision:** Hand-written Ramer-Douglas-Peucker (~30 lines).

**Rationale:** RDP is a trivial algorithm.
Importing the `geo` crate for a single algorithm pulls in a large dependency tree.
Self-implementation avoids unnecessary dependencies and keeps the WASM binary small.

## File Upload

**Decision:** Dioxus built-in file events (`onchange`/`ondrop`).

**Rationale:** Cross-platform, zero extra dependencies.
Dioxus wraps the HTML `<input type="file">` and drag-drop events with a unified API that works on both web and desktop.
No need for the `rfd` crate.

## Preview Rendering

**Decision:** Inline SVG elements in the Dioxus DOM.

**Rationale:** Simplest approach -- no HTML canvas, no JavaScript interop, no `web-sys` for rendering.
Dioxus supports SVG elements natively in RSX.
Vector rendering is crisp at any zoom level.
For very complex paths, the display version uses higher RDP tolerance to keep the DOM lightweight.

## File Downloads (WASM)

**Decision:** `web-sys` Blob + object URL + temporary `<a>` click.

**Rationale:** Type-safe Rust bindings.
Handles both string and binary data natively.
No JavaScript string escaping issues (unlike `document::eval`).

## Threading Strategy

**Decision:** Main thread for MVP, web workers deferred.

**Rationale:** Simpler setup.
Loading indicator shown during processing.
Move to web workers if UI blocking proves unacceptable with real-world images.

## Path Optimization

**Decision:** Nearest-neighbor TSP on contours with direction reversal.

**Rationale:** Simple, effective for sand table output quality.
Image2Sand uses the same approach.
2-opt improvement deferred as a future enhancement.

## Deployment Target

**Decision:** GitHub Pages with custom domain (`mujou.art`), app at `/app/` path.

**Rationale:** Simplest option that avoids adding a vendor.
The code is already on GitHub, so Pages is a setting on the same repo rather than a new account and billing relationship.
Free tier (100GB bandwidth/mo) is sufficient for a niche tool.
Build pipeline uses GitHub Actions, which is needed for CI anyway.
Static sites are inherently portable -- if GitHub Pages becomes insufficient, migrating to any other static host requires only changing the deploy target.

**Alternatives considered:**

- **Cloudflare Pages** -- faster CDN, generous free tier, pairs with R2 for blob storage. Rejected to avoid Cloudflare platform lock-in; each additional Cloudflare service (Workers, KV, R2, D1) increases coupling. The performance difference is negligible for a niche tool.
- **Netlify** -- best deploy DX (preview deploys, form handling, split testing). Rejected because its differentiating features (forms, serverless functions) are unused by mujou, and it adds a vendor for no capability gain over GitHub Pages. Bandwidth overages are billed at $55/100GB.
- **Subdomains** (`app.mujou.art`) -- preferred over path-based routing but requires either two repos (one per GitHub Pages site) or a different hosting provider. Path-based (`/app/`) is acceptable and keeps everything in one repo. Revisit if hosting provider changes.

**Configuration:** See [Requirements](requirements.md#deployment).

## Reference Target Device

**Decision:** Default pipeline resolution and parameters target a ~34" (850mm) diameter sand table with a ~5mm effective track width.

**Rationale:**

### Market survey (Feb 2026)

The kinetic sand table market spans desktop toys to furniture-scale pieces.
Key products by sand area diameter:

| Brand | Model | Sand Diameter | Price | Notes |
| ----- | ----- | ------------- | ----- | ----- |
| Oasis Mini | Desktop | 9" / 23cm | $129-149 | Best seller, 50k+ units shipped |
| SANDSARA mini | Desktop | ~8" / 20cm | $169-180 | |
| Sisyphus Mini ES | Desktop | 9.9" / 25cm | $690 | |
| SANDSARA Dark Walnut | Desktop | ~14" / 36cm | $750 | |
| HoMedics Drift 16" | Desktop | 16" / 41cm | $319 | |
| Sisyphus Metal Side | Side table | 16" / 41cm | $1,780 | |
| Oasis Side Table | Side table | 20" / 50cm | $399-499 | Pre-order, ships 2026 |
| HoMedics Drift 21" | Desktop | 21" / 53cm | $500 | |
| Sisyphus Metal Coffee | Coffee table | 27.25" / 69cm | $2,640 | |
| Oasis Coffee Table | Coffee table | 34" / 85cm | $799-999 | Pre-order, ships 2026 |

The **Oasis Coffee Table (34" / 850mm)** is the largest mainstream table.
It is also under $1,000, making it the largest table likely to see significant volume.

### Effective track width (~5mm)

All these tables use a steel ball (~12mm diameter) dragged magnetically through sand.
The ball is a sphere, so the groove it carves is narrower than the ball diameter -- only the contact chord at the depth the ball sinks matters:

- 0.5mm sink depth: track width ≈ 4.8mm
- 1.0mm sink depth: track width ≈ 6.6mm
- 2.0mm sink depth: track width ≈ 9.0mm

**We use 5mm as the working estimate for effective track width.**

### Resolvable detail vs. positional precision

The 5mm track width constrains two different things:

1. **Minimum line spacing** -- two parallel lines must be ≥5mm apart to read as distinct features. For a 34" (850mm) table this gives ~170 independent lines across the diameter.

2. **Positional precision** -- a single line's *position* can be controlled much finer than 5mm. A gently curving or slightly angled line benefits from sub-track-width resolution, the same way anti-aliased text benefits from sub-pixel positioning. Coarse quantization would produce visible staircase artifacts on gentle curves.

This means the useful *processing* resolution is higher than 170px -- we need enough resolution for smooth contour positioning, even though the output can't resolve features closer than ~5mm.

### Resolvable lines per table

At 5mm track width:

| Table | Diameter | Independent lines across |
| ----- | -------- | ----------------------- |
| Oasis Mini | 9" / 230mm | ~46 |
| Oasis Side Table | 20" / 500mm | ~100 |
| Sisyphus Metal Coffee | 27.25" / 690mm | ~138 |
| Oasis Coffee Table | 34" / 850mm | ~170 |

### Pipeline resolution strategy

**MVP approach:** Downsample the input image early (after decode) to a working resolution of ~256px on the long axis. Run the full pipeline (grayscale, blur, Canny, contour tracing, simplification, masking, joining) at this resolution. This is ~1.5x oversampling relative to the ~170 resolvable lines on the largest common table, which provides some headroom for smooth contour positioning without processing pixels that can never produce visible detail.

At 256x256 (65k pixels) vs 1024x1024 (1M pixels), the expensive stages (blur, Canny) should run ~16x faster.

Positional precision on gentle curves will be limited to the ~3.3mm grid spacing (850mm / 256px). This may produce visible staircase artifacts on the largest tables. Acceptable for MVP; evaluate with real output.

**Deferred: coarse-then-fine with region masking.** A coarse pass at low resolution identifies where edges exist, producing a binary mask of "interesting" regions. A second fine-resolution pass runs only in unmasked regions, skipping the ~99% of the image that is featureless. This avoids full-image high-res cost while preserving sub-pixel positional precision where edges actually occur. Simpler than a tiling approach (no stitching across tile boundaries). See [Open Questions](open-questions.md#performance).

## Coordinate System and Normalized Space

**Decision:** All pipeline stages after contour tracing operate in a center-origin,
+Y-up normalized coordinate space where the mask edge = 1.0 from center.
A mask is always required. The `mask_scale` parameter is replaced by `zoom`
(photographer's convention, default 1.25, range 0.4–3.0).

**Rationale:**

### Problems with the previous design

The previous pipeline used working-resolution pixel coordinates throughout,
with the mask as an optional clipping step. This caused several issues:

1. **Wasted viewport.** The preview SVG viewBox covered the full image
   dimensions. When the mask was smaller than the image (the common case),
   the actual output content occupied a fraction of the preview area.

2. **Resolution-coupled parameters.** Simplification tolerance, subsample
   spacing, and border point spacing were in pixel units. Changing
   `working_resolution` silently changed the visual effect of these
   parameters.

3. **Inconsistent coordinate semantics.** The internal representation
   (pixel coords), preview (pixel-based viewBox), and export formats
   (THR: normalized polar; SVG: sometimes mm, sometimes pixels; G-code: mm)
   each used different coordinate systems with ad-hoc inline transforms.

4. **Ambiguous info strings.** Diagnostic labels mixed pixel values with
   scale fractions. The `mask_scale` parameter (a fraction of the image
   diagonal) was labeled "d=" in diagnostics, easily confused with
   "diameter."

5. **Scale semantics.** `mask_scale` controlled the mask size relative to
   the image. Increasing the value made the mask larger, showing more
   content but also expanding the output geometry. The mental model was
   "resize the mask within a fixed image" rather than "zoom a fixed
   output frame into the image."

### Design

See [Principles: Coordinate System](principles.md#coordinate-system) for the
full specification.

Key choices:

- **Center origin, +Y up, radius = 1.** Aligns with the unit circle, makes THR
  polar conversion trivial (`rho = distance`, no scaling), and matches
  math/G-code conventions. The Y-flip from pixel space happens once.

- **Mask always required.** Every sand table and plotter has a physical boundary.
  The mask IS the output frame — it defines the normalized space. Removing the
  "no mask" case simplifies the pipeline (one coordinate system after
  normalization) and eliminates an edge case that produced unbounded output.

- **Zoom replaces mask_scale.** The output frame (mask shape) is fixed at
  radius = 1. The `zoom` parameter controls how the source image is projected
  into that frame using the photographer's convention: zoom > 1 magnifies
  (crops more), zoom < 1 shrinks (crops less). This inverts the old mental
  model and eliminates the viewport waste problem — the preview always fills
  the screen.

- **Normalization between contour tracing and simplification.** Pixel-buffer
  stages (decode through Canny) necessarily operate in pixel space.
  Contour tracing produces pixel coordinates from the edge map. All subsequent
  vector stages (simplify, mask, join, subsample) operate in normalized space.
  This is the natural boundary: raster processing ends, vector processing begins.

- **Resolution-independent parameters.** Simplification tolerance, subsample
  spacing, and border point spacing are in normalized units. The same parameter
  values produce visually consistent results regardless of `working_resolution`.

### Alternatives considered

- **Normalize at export only.** Keeps the pipeline in pixel space and transforms
  at the end. Rejected because it perpetuates resolution-coupled parameters and
  duplicates transform logic across exporters.

- **Shorter dimension = 1 (diameter-based).** The mask circle would have
  radius = 0.5 and the viewBox would be `-0.5 -0.5 1 1`. Rejected because
  in a center-origin system, radius = 1 is more natural: it aligns with the
  unit circle, makes THR rho = Euclidean distance, and avoids 0.5 factors
  throughout.

- **Keep mask optional.** Would require a separate "no mask" normalization
  (image shorter dim = 2, center origin). Rejected because every real output
  device has a physical boundary, and the "no mask" case added complexity for
  a use case that doesn't exist in practice.

## Project Architecture

**Decision:** Sans-IO with three-layer Cargo workspace.
See [Principles](principles.md) and [Architecture](architecture.md).

**Rationale:** Matches patterns established in the onshape-mcp project.
Core crates are testable without a browser or WASM runtime.
Clear separation between pure logic and platform I/O.
