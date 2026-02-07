# Requirements

## Platform Targets

| Platform | Priority | Status |
| -------- | -------- | ------ |
| Web (WASM) | Primary | Target for MVP |
| Desktop | Future | Same codebase via Dioxus |
| Android | Future | Experimental in Dioxus |
| iOS | Future | Better support in Dioxus than Android |

The web target is the MVP.
Desktop and mobile targets share the same Rust codebase via Dioxus multi-platform support, but are not in scope for the initial implementation.

## Technology Choices

| Component | Choice | Version | Rationale |
| --------- | ------ | ------- | --------- |
| Language | Rust | Edition 2024 | Safety, performance, WASM compilation |
| UI framework | Dioxus | 0.7+ | Single codebase for web/desktop/mobile, React-like RSX, Tailwind CSS, largest Rust GUI community |
| Styling | Tailwind CSS | 4 | Utility-first CSS, works with Dioxus RSX `class:` attributes |
| Image processing | `imageproc` | 0.26 | Canny edge detection, Gaussian blur, contour tracing; WASM-compatible with `default-features = false` |
| Image decoding | `image` | 0.25 | PNG, JPEG, BMP, WebP decoding; pure Rust, WASM-compatible |
| WASM browser APIs | `web-sys` | 0.3 | Type-safe bindings for Blob, URL, file downloads |
| Build tool | `dx` (Dioxus CLI) | latest | Project scaffolding, dev server, WASM builds |

### Why Dioxus Over Alternatives

- **vs egui**: egui is simpler and faster to prototype, but produces apps that look like developer tools, not consumer-facing web apps. Dioxus supports Tailwind CSS for polished styling. If Dioxus proves too painful for the web-only MVP, falling back to egui + eframe is reasonable.
- **vs Leptos/Yew**: These are web-only frameworks. Dioxus provides a path to desktop and mobile from the same codebase.
- **vs JavaScript/TypeScript**: Rust compiles to WASM for client-side execution with native-like performance. The image processing pipeline benefits from Rust's speed. No need for a separate backend.

### imageproc WASM Compatibility

`imageproc` compiles to `wasm32-unknown-unknown` with `default-features = false`.
This disables:

- **`rayon`** -- parallel processing via OS threads, which do not exist in WASM
- **`fft`** -- Fast Fourier Transform support, which pulls in `rustdct` with potential WASM issues

The core algorithms (Canny edge detection, Gaussian blur, contour finding) are pure Rust and work in single-threaded WASM.
The `imageproc` maintainers actively test against WASM (`wasm-bindgen-test` is in dev-dependencies).

### getrandom WASM Requirement

`imageproc` depends on `rand` which depends on `getrandom`.
For WASM targets, `getrandom` requires the `wasm-bindgen` feature to source randomness from `crypto.getRandomValues()`:

```toml
getrandom = { version = "0.3", features = ["wasm-bindgen"] }
```

## Supported Input Formats

- PNG
- JPEG
- BMP
- WebP (pending WASM compatibility verification)

## Supported Output Formats

See [Output Formats](formats.md) for detailed specifications.

- Theta-Rho (.thr) -- Sisyphus / Oasis Mini / DIY polar sand tables
- G-code (.gcode) -- XY/Cartesian sand tables (ZenXY, GRBL/Marlin)
- SVG (.svg) -- Universal vector format, also accepted by Oasis Mini app
- DXF (.dxf) -- CAD interchange
- PNG preview -- Rasterized path render for sharing/thumbnailing

## Deployment

Static site deployment.
`dx build --platform web` produces HTML + JS + WASM files.
Hostable on GitHub Pages, Netlify, Cloudflare Pages, or any static file server.
Zero backend, zero server-side processing.
