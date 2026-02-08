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

Static site deployment via GitHub Pages.
`dx bundle --release` produces HTML + JS + WASM files.
Zero backend, zero server-side processing.

### Hosting

| Component | Choice | Notes |
| --------- | ------ | ----- |
| Static site host | GitHub Pages | Free tier, same repo, no additional vendor |
| Domain | `mujou.art` | Registration and DNS hosting at registrar |
| URL structure | `/` landing page, `/app/` WASM app | Path-based routing, single repo |
| HTTPS | Automatic | GitHub Pages provisions TLS for custom domains |
| Blob storage | Deferred | Evaluate independently when needed (B2, R2, Tigris) |

### Dioxus Configuration

The WASM app is served under `/app/`, not at the domain root.
The `base_path` is set via the `--base-path app` CLI flag at deploy time, not in `Dioxus.toml`, so that local development with `dx serve` continues to work at the root path.

### GitHub Pages Repository Configuration

1. In the repository **Settings > Pages**, set **Source** to **GitHub Actions**
2. Under **Custom domain**, enter `mujou.art` and click **Save** (the custom domain is managed in repo settings, not via a `CNAME` file, when using GitHub Actions as the source)
3. Create a GitHub Actions workflow that:
   - Builds the WASM app with `dx bundle --release`
   - Assembles the deploy directory with the landing page at root and app output under `app/`
   - Copies `app/index.html` to `app/404.html` for client-side routing
   - Deploys using `actions/upload-pages-artifact` and `actions/deploy-pages`

### Custom Domain DNS Configuration

Configure DNS at the domain registrar to point at GitHub Pages:

**For the apex domain (`mujou.art`)**, add `A` and `AAAA` records pointing to GitHub Pages' IP addresses:

| Type | Host | Value |
| ---- | ---- | ----- |
| A | @ | 185.199.108.153 |
| A | @ | 185.199.109.153 |
| A | @ | 185.199.110.153 |
| A | @ | 185.199.111.153 |
| AAAA | @ | 2606:50c0:8000::153 |
| AAAA | @ | 2606:50c0:8001::153 |
| AAAA | @ | 2606:50c0:8002::153 |
| AAAA | @ | 2606:50c0:8003::153 |

**For `www.mujou.art`**, add a `CNAME` record to enable the redirect to the apex domain:

| Type | Host | Value |
| ---- | ---- | ----- |
| CNAME | www | `<username>.github.io` |

GitHub Pages automatically redirects `www.mujou.art` to `mujou.art` when the apex domain is configured as the custom domain.
The `CNAME` record routes `www` requests to GitHub's servers so they can issue the redirect.

After DNS propagates, enable **Enforce HTTPS** in the repository's Pages settings.

These IP addresses are monitored by a [scheduled workflow](../../.github/workflows/check-pages-ips.yml) that opens a PR if GitHub changes them.
The canonical values are stored in [`.github/pages-ips.json`](../../.github/pages-ips.json).

### Source and Deploy Structure

Landing page source is in `site/` (checked into the repo).
The `{{REPO_URL}}` placeholder in `site/index.html` is substituted by the deploy workflow using GitHub context variables.

The [deploy workflow](../../.github/workflows/deploy-github-pages.yml) assembles this structure before uploading:

```text
deploy/                         # assembled by CI (not checked in)
├── index.html                  # from site/, with {{REPO_URL}} substituted
├── app/
│   ├── index.html              # Dioxus app entry (from dx bundle)
│   ├── 404.html                # copy of index.html for client-side routing
│   └── assets/
│       ├── mujou_bg-*.wasm     # content-hashed WASM binary
│       └── mujou-*.js          # content-hashed JS loader
```
