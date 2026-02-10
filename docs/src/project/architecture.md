# Architecture

## Layer Design

```text
┌───────────────────────────────────────────────────────┐
│                   Application Layer                    │
│  ┌─────────────────────────────────────────────────┐  │
│  │                    mujou                          │  │
│  │       (Dioxus web app - wires everything)        │  │
│  └─────────────────────────────────────────────────┘  │
├───────────────────────────────────────────────────────┤
│                   Integration Layer                    │
│  ┌─────────────────────────────────────────────────┐  │
│  │                  mujou-io                         │  │
│  │  (web-sys file I/O, Blob downloads,              │  │
│  │   Dioxus component library)                      │  │
│  └─────────────────────────────────────────────────┘  │
├───────────────────────────────────────────────────────┤
│                      Core Layer                        │
│  ┌────────────────────┐  ┌─────────────────────────┐ │
│  │  mujou-pipeline     │  │    mujou-export          │ │
│  │  (image processing: │  │  (format serializers:    │ │
│  │   blur, canny,      │  │   .thr, .gcode, .svg,   │ │
│  │   contours, RDP,    │  │   .dxf, .png)            │ │
│  │   optimization)     │  │                          │ │
│  │  NO I/O             │  │  NO I/O                  │ │
│  └────────────────────┘  └─────────────────────────┘ │
└───────────────────────────────────────────────────────┘
```

## Workspace Layout

```text
mujou/
├── Cargo.toml                    # Workspace root
├── crates/
│   ├── mujou-pipeline/           # Pure image processing (sans-IO)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── grayscale.rs
│   │       ├── blur.rs
│   │       ├── edge.rs           # Canny edge detection
│   │       ├── contour.rs        # Contour tracing
│   │       ├── optimize.rs       # Path optimization
│   │       ├── simplify.rs       # Ramer-Douglas-Peucker
│   │       ├── mask.rs           # Circular mask / crop
│   │       └── types.rs          # Point, Polyline, PipelineConfig
│   ├── mujou-export/             # Pure format serializers (sans-IO)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── thr.rs            # Theta-Rho format
│   │       ├── gcode.rs          # G-code format
│   │       ├── svg.rs            # SVG format
│   │       ├── dxf.rs            # DXF format
│   │       └── png.rs            # Rasterized preview
│   ├── mujou-io/                 # Browser I/O + Dioxus components
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── download.rs       # Blob URL file downloads
│   │       └── components/
│   │           ├── mod.rs
│   │           ├── upload.rs     # File upload / drag-drop
│   │           ├── preview.rs    # SVG preview of paths
│   │           ├── controls.rs   # Parameter sliders
│   │           └── export.rs     # Export format buttons
│   └── mujou/                    # Binary entry point
│       ├── Cargo.toml
│       └── src/
│           └── main.rs
├── docs/                         # mdBook documentation
│   ├── book.toml
│   └── src/
│       ├── SUMMARY.md
│       └── project/
│           └── *.md
├── assets/                       # Static assets (example images)
├── Dioxus.toml                   # Dioxus CLI config
├── rust-toolchain.toml
├── rustfmt.toml
├── .pre-commit-config.yaml
├── typos.toml
├── deny.toml
├── AGENTS.md
├── README.md
├── LICENSE-MIT
└── LICENSE-APACHE
```

## Crate Descriptions

| Crate | Layer | Purpose |
| ----- | ----- | ------- |
| `mujou` | Application | Dioxus web app entry point, wires everything together |
| `mujou-io` | Integration | Browser I/O (file upload, downloads, DOM), Dioxus components |
| `mujou-pipeline` | Core | Pure image processing: grayscale, blur, Canny, contour tracing, RDP, path optimization (no I/O) |
| `mujou-export` | Core | Pure format serializers: THR, G-code, SVG, DXF, PNG (no I/O) |

## Data Flow

```text
Image bytes (from file upload)
  │
  ▼
┌──────────────────────────────────────────────────────┐
│  mujou-pipeline (core, pure)                          │
│                                                       │
│  decode → grayscale → blur → canny                    │
│    → contours (ContourTracer: border following | ...)  │
│    → simplify (RDP) → optimize path order             │
│    → join into single path (PathJoiner: straight | ...)│
│    → optional circular mask                           │
│                                                       │
│  Output: Polyline (single continuous path)            │
└──────────────┬───────────────────────────────────────┘
               │
               ▼
┌──────────────────────────────────────────────────────┐
│  mujou-export (core, pure)                            │
│                                                       │
│  Polyline → .thr text                                 │
│  Polyline → .gcode text                               │
│  Polyline → .svg text                                 │
│  Polyline → .dxf text                                 │
│  Polyline → PNG bytes                                 │
└──────────────┬───────────────────────────────────────┘
               │
               ▼
┌──────────────────────────────────────────────────────┐
│  mujou-io (integration)                               │
│                                                       │
│  Render SVG preview in DOM                            │
│  Trigger Blob download for export files               │
└──────────────────────────────────────────────────────┘
```

## Key Design Constraints

### WASM Target

The primary build target is `wasm32-unknown-unknown`.
This constrains dependency choices:

- No OS threads (`rayon` disabled in `imageproc`)
- No filesystem access in core crates
- No native FFI
- Browser APIs accessed through `web-sys` only in the IO layer

### Client-Side Only

All processing runs in the user's browser.
No images are uploaded to any server.
The deployed artifact is static files (HTML + JS + WASM) hostable on GitHub Pages, Netlify, or Cloudflare Pages with zero backend.
