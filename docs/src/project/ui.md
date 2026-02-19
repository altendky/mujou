# UI Design

Simple, tool-focused interface.
Built with Dioxus 0.7 RSX and Tailwind CSS.
All processing runs client-side in WASM.

## Components

### File Upload

Compact upload icon button in the header with a full-page drag-and-drop overlay.
Uses the Lucide `upload` icon via `dioxus-free-icons`.

- **Header button**: styled `<label>` wrapping a hidden `<input type="file">` — accepts PNG, JPEG, BMP, WebP
- **Drag overlay**: a fixed-position sentinel layer (`position: fixed; inset: 0`) is always in the DOM but invisible and non-interactive. When a file is dragged over the browser window the overlay becomes visible with a semi-transparent backdrop, dashed border, and "Drop image here" prompt. Uses a `dragenter`/`dragleave` counter to handle child-element event bubbling.
- Uses Dioxus built-in `onchange`/`ondrop` file events (cross-platform, no extra dependencies)
- Reads file bytes via `file.read_bytes().await`
- File validation errors display inline next to the upload button

### Canvas Preview

Traced paths rendered as inline SVG elements directly in Dioxus RSX.
No HTML `<canvas>` or JavaScript interop needed.

- SVG `viewBox` matches the mask bounding box in
  [normalized coordinates](principles.md#coordinate-system):
  - Circle: `viewBox="-1 -1 2 2"`
  - Rectangle: viewBox covers the rectangle extent
    (e.g., `"-2 -1 4 2"` for aspect ratio 2, landscape)
- The mask shape always fills the entire preview — no screen space is
  wasted on clipped-away content
- Y-axis flipped at the SVG boundary (normalized +Y up → SVG +Y down)
- Each polyline becomes a `<path>` element
- Toggle between preview modes: original, edges, paths overlaid, paths only
- Paths may use a higher RDP tolerance for display to keep the DOM lightweight

### Parameter Panel

Sliders wired to `PipelineConfig` via Dioxus signals.
Pipeline re-runs when parameters change.

Parameters operating on pixel buffers (steps 1–5) are in pixel units.
Parameters operating on polylines (steps 6–10) are in
[normalized units](principles.md#coordinate-system) and produce consistent
results regardless of `working_resolution`.

| Control | Input Type | Range | Default | Units |
| ------- | ---------- | ----- | ------- | ----- |
| Blur radius | Slider | 0.0 – 10.0 | 1.4 | pixels |
| Canny low threshold | Slider | 1 – canny max | 15 | pixels |
| Canny high threshold | Slider | canny low – canny max | 40 | pixels |
| Canny max | Slider | canny high – ~1140 | 60 | pixels |
| Contour tracing | Select | Border following / Marching squares | Border following | — |
| Zoom | Slider | 0.4 – 3.0 | 1.25 | × (zoom factor) |
| Simplify tolerance | Slider | 0.0 – TBD | TBD | normalized |
| Mask shape | Select | Circle / Rectangle | Circle | — |
| Mask aspect ratio | Slider | 1.0 – 4.0 | 1.0 | — (Rectangle only) |
| Mask orientation | Toggle | landscape/portrait | landscape | — (Rectangle only) |
| Path joining | Select | Straight line / Retrace / Mst | Mst | — |
| Border path | Select | Auto / On / Off | Auto | — |
| Invert | Toggle | on/off | off | — |
| Preview mode | Select | original/edges/paths/paths only | paths | — |

Key changes from previous design:

- **Zoom** replaces "Mask diameter" — controls image magnification into the
  fixed mask frame, not the mask size
- **Mask shape** replaces "Circular mask" toggle — always required, choose
  between Circle and Rectangle
- **Units column** added for clarity — pixel-space parameters are clearly
  distinct from normalized-space parameters

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
│  mujou                                    [⬆]        │
├──────────────────────────┬──────────────────────────┤
│                          │  Parameters               │
│                          │  ┌──────────────────────┐ │
│     Preview Canvas       │  │ Blur: ━━━●━━━━━━━━━  │ │
│                          │  │ Low:  ━━━━●━━━━━━━━  │ │
│     (SVG rendering)      │  │ High: ━━━━━━━●━━━━  │ │
│     Mask fills entire    │  │ Zoom: ━━━━●━━━━━━━  │ │
│     preview area         │  │ Simplify: ━●━━━━━━━  │ │
│                          │  │ Shape: ○ Circle       │ │
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

- `image_bytes: Signal<Option<Vec<u8>>>` — uploaded image data
- `config: Signal<PipelineConfig>` — pipeline parameters from sliders and
  strategy selects (includes `zoom`, `mask_shape`, etc.)
- `path: Signal<Option<Polyline>>` — pipeline output (single continuous path,
  in [normalized coordinates](principles.md#coordinate-system))
- `processing: Signal<bool>` — loading indicator

When `image_bytes` or `config` changes, the pipeline re-runs and `path` updates.
The SVG preview renders the normalized-space path with a `viewBox` matching the
mask bounding box, so the mask shape always fills the preview.

## Error Handling

- Invalid image format: show error message inline next to the upload button
- Processing failure: show error message, keep last successful result
- Oversized image: warn user, offer to downsample
