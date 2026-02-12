# UI Design

Simple, tool-focused interface.
Built with Dioxus 0.7 RSX and Tailwind CSS.
All processing runs client-side in WASM.

## Components

### File Upload

Compact "Upload Image" button in the header with a full-page drag-and-drop overlay.

- **Header button**: styled `<label>` wrapping a hidden `<input type="file">` — accepts PNG, JPEG, BMP, WebP
- **Drag overlay**: a fixed-position sentinel layer (`position: fixed; inset: 0`) is always in the DOM but invisible and non-interactive. When a file is dragged over the browser window the overlay becomes visible with a semi-transparent backdrop, dashed border, and "Drop image here" prompt. Uses a `dragenter`/`dragleave` counter to handle child-element event bubbling.
- Uses Dioxus built-in `onchange`/`ondrop` file events (cross-platform, no extra dependencies)
- Reads file bytes via `file.read_bytes().await`
- File validation errors display inline next to the upload button

### Canvas Preview

Traced paths rendered as inline SVG elements directly in Dioxus RSX.
No HTML `<canvas>` or JavaScript interop needed.

- SVG `viewBox` matches image dimensions
- Each polyline becomes a `<path>` element
- Toggle between preview modes: original, edges, paths overlaid, paths only
- Paths may use a higher RDP tolerance for display to keep the DOM lightweight

### Parameter Panel

Sliders wired to `PipelineConfig` via Dioxus signals.
Pipeline re-runs when parameters change.

| Control | Input Type | Range | Default |
| ------- | ---------- | ----- | ------- |
| Blur radius | Slider | 0.0 - 10.0 | 1.4 |
| Canny low threshold | Slider | 1 - canny max | 30 |
| Canny high threshold | Slider | canny low - canny max | 80 |
| Canny max | Slider | canny high - ~1140 | 120 |
| Contour tracing | Select | Border following / Marching squares | Border following |
| Simplify tolerance | Slider | 0.0 - 20.0 | 2.0 |
| Path joining | Select | Straight line / Retrace / ... | Retrace |
| Circular mask | Toggle | on/off | on |
| Mask diameter | Slider | 0.1 - 1.0 | 1.0 |
| Invert | Toggle | on/off | off |
| Preview mode | Select | original/edges/paths/paths only | paths |

Strategy selects (contour tracing, path joining) follow the [pluggable algorithm strategy](principles.md#pluggable-algorithm-strategies) principle.
Only implemented strategies are shown in the UI; future strategies appear as they are added.

### Export Panel

Buttons for each output format.
Downloads triggered via `web-sys` Blob URL mechanism.

| Button | Format | MIME Type |
| ------ | ------ | --------- |
| Export THR | `.thr` | `text/plain` |
| Export G-code | `.gcode` | `text/plain` |
| Export SVG | `.svg` | `image/svg+xml` |
| Export DXF | `.dxf` | `application/dxf` |
| Export PNG | `.png` | `image/png` |

### File Download Mechanism (WASM)

Dioxus has no built-in file download API.
Downloads are triggered via `web-sys`:

1. Create a `Blob` from the export data (string or bytes)
2. Create an object URL via `Url::create_object_url_with_blob()`
3. Create a temporary `<a>` element with `download` attribute
4. Programmatically click the element
5. Revoke the object URL

This code lives in `mujou-io/src/download.rs`.

## Layout

Responsive layout for desktop and mobile browsers.
Many Oasis Mini users will access from phones.

### Desktop Layout

```text
┌─────────────────────────────────────────────────────┐
│  mujou                            [Upload Image]     │
├──────────────────────────┬──────────────────────────┤
│                          │  Parameters               │
│                          │  ┌──────────────────────┐ │
│     Preview Canvas       │  │ Blur: ━━━●━━━━━━━━━  │ │
│                          │  │ Low:  ━━━━●━━━━━━━━  │ │
│     (SVG rendering)      │  │ High: ━━━━━━━●━━━━  │ │
│                          │  │ Simplify: ━●━━━━━━━  │ │
│                          │  │ ☐ Circular mask      │ │
│                          │  │ ☐ Invert             │ │
│                          │  └──────────────────────┘ │
│                          │                           │
│                          │  Export                    │
│                          │  ┌──────────────────────┐ │
│                          │  │ [THR] [G-code] [SVG] │ │
│                          │  │ [DXF] [PNG]          │ │
│                          │  └──────────────────────┘ │
└──────────────────────────┴──────────────────────────┘
```

Drag overlay (shown only while dragging a file over the window):

```text
┌─────────────────────────────────────────────────────┐
│ ┌ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─┐ │
│ │                                                  │ │
│ │            Drop image here                       │ │
│ │            PNG, JPEG, BMP, WebP                  │ │
│ │                                                  │ │
│ └ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─┘ │
└─────────────────────────────────────────────────────┘
```

### Mobile Layout

Stacked vertically: header (with upload button), preview, parameters (collapsed/expandable), export buttons.

## State Management

Dioxus signals for reactive state:

- `image_bytes: Signal<Option<Vec<u8>>>` -- uploaded image data
- `config: Signal<PipelineConfig>` -- pipeline parameters from sliders and strategy selects
- `path: Signal<Option<Polyline>>` -- pipeline output (single continuous path)
- `processing: Signal<bool>` -- loading indicator

When `image_bytes` or `config` changes, the pipeline re-runs and `path` updates, which triggers the SVG preview to re-render.

## Error Handling

- Invalid image format: show error message inline next to the upload button
- Processing failure: show error message, keep last successful result
- Oversized image: warn user, offer to downsample
