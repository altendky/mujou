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

## Project Architecture

**Decision:** Sans-IO with three-layer Cargo workspace.
See [Principles](principles.md) and [Architecture](architecture.md).

**Rationale:** Matches patterns established in the onshape-mcp project.
Core crates are testable without a browser or WASM runtime.
Clear separation between pure logic and platform I/O.
