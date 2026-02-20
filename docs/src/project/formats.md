# Output Formats

All exports receive polylines in the
[normalized coordinate system](principles.md#coordinate-system) (center origin,
+Y up, mask edge at radius 1). Each serializer transforms from normalized space
to the format's native coordinate system. Serializers are pure functions in the
`mujou-export` crate (core layer, no I/O).

For a device-compatibility view of which tables accept which formats, see [File Formats by Device](../ecosystem/formats.md).

## Theta-Rho (.thr)

For Sisyphus tables, Oasis Mini, and DIY polar sand tables.

### Format Specification

- Plain text, one `theta rho` pair per line (space-separated)
- Theta: continuous radians (accumulating, does NOT wrap at 2pi)
- Rho: 0.0 (center) to 1.0 (edge), normalized
- Lines beginning with `#` are comments, ignored by table firmware

### Example

```text
# mujou
# Source: cherry-blossoms.jpg
# blur=1.4, canny=15/40, simplify=2, tracer=BorderFollowing, joiner=Mst, mask=75%, res=256
# Exported: 2026-02-14_12-30-45
# Config: {"blur_sigma":1.4,"canny_low":15.0,...}
0.00000 0.00000
0.10000 0.15000
0.20000 0.30000
0.50000 0.45000
1.00000 0.60000
```

### Metadata

Metadata is embedded as `#`-prefixed comment lines at the top of the file.
This mirrors the SVG exporter's `<title>`, `<desc>`, and `<metadata>` approach
and follows the convention established by [Sandify](https://github.com/jeffeb3/sandify),
which uses `#` comments for file name, type, and section markers.

| Line prefix | Content | Purpose |
| ----------- | ------- | ------- |
| `# mujou` | Fixed identifier | Identifies the file as mujou-generated |
| `# Source:` | Source image filename | Provenance |
| `#` (free-form) | Pipeline parameters summary | Human-readable settings (blur, canny, simplify, etc.) |
| `# Exported:` | Timestamp | When the file was exported |
| `# Config:` | Full `PipelineConfig` JSON | Machine-parseable settings for reproducibility |

All metadata lines are optional.  Parsers should skip any line beginning with `#`.

Each metadata value must occupy a single line.  Producers must not embed newline characters within a `#` comment value — continuation text after a newline would lack the `#` prefix and be misinterpreted as theta-rho data by table firmware.

The `# Config:` line contains the complete serialized `PipelineConfig` as a
single JSON object, matching the content of the SVG exporter's
`<mujou:pipeline>` element.  This allows re-importing settings to reproduce
the exact same output.

### Normalized-to-Polar Conversion

With a circle mask, the pipeline output is already in the ideal coordinate
system for THR conversion: center origin, +Y up, unit circle.

1. **Rho**: `rho = sqrt(x² + y²)` — Euclidean distance IS rho. No centering
   or radius normalization needed; the mask circle has radius 1 and points
   inside it have rho ∈ [0, 1].
2. **Theta**: `theta = atan2(x, y)` — Sisyphus convention (not standard math
   `atan2(y, x)`). Theta = 0 points up (+Y). Since normalized space is +Y up,
   no Y-flip is needed.

The Sisyphus `atan2(x, y)` convention means:

- `theta = atan2(x, y)`
- `x = rho * sin(theta)`, `y = rho * cos(theta)`

Confirmed by [Sandify](https://github.com/jeffeb3/sandify) and
[jsisyphus](https://github.com/markyland/SisyphusForTheRestOfUs).

**Continuous theta unwinding** is still critical.
Theta must accumulate across the full path without wrapping at 2π:

```rust
for each point after the first:
    raw_theta = atan2(x, y)
    delta = raw_theta - prev_theta
    while delta > PI:
        delta -= 2 * PI
    while delta < -PI:
        delta += 2 * PI
    theta = prev_theta + delta
    prev_theta = theta
```

With a rectangle mask, the rho calculation is unchanged but rho values
may exceed 1.0 for points in the corners of the rectangle that fall
outside the inscribed circle. THR producers must either clamp or handle
this at the application level.

### Path Start/End Requirements

The path must start and end with rho at 0 (center) or 1 (edge).
If the contours don't naturally start/end there, add a spiral-in or spiral-out segment.

## G-code (.gcode)

For XY/Cartesian sand tables (ZenXY, GRBL/Marlin machines).

### Format Specification

- Standard G-code text
- `G0 X... Y...` -- rapid move (travel between contours)
- `G1 X... Y... F...` -- linear move (drawing)
- Coordinates scaled to configurable bed size

### Example

```gcode
G28 ; Home
G90 ; Absolute positioning
G0 X10.00 Y15.00
G1 X12.50 Y18.30 F3000
G1 X14.00 Y20.10 F3000
G0 X30.00 Y5.00
G1 X32.50 Y7.80 F3000
```

### Coordinate Transform

Normalized coordinates are scaled to the bed dimensions. For a **circle** mask
(or square rectangle), both axes span [-1, 1]:

- `gcode_x = (norm_x + 1) × bed_width / 2`
- `gcode_y = (norm_y + 1) × bed_height / 2`

For a **rectangle** mask with `aspect_ratio = A > 1`, the long axis spans
`[-A, A]` in normalized space. Map the long axis to the long bed dimension:

```text
// landscape (long = X):
gcode_x = (norm_x / A + 1) × bed_width  / 2   // maps [-A, A] → [0, bed_width]
gcode_y = (norm_y     + 1) × bed_height / 2   // maps [-1, 1] → [0, bed_height]

// portrait (long = Y):
gcode_x = (norm_x     + 1) × bed_width  / 2   // maps [-1, 1] → [0, bed_width]
gcode_y = (norm_y / A + 1) × bed_height / 2   // maps [-A, A] → [0, bed_height]
```

### Configuration

| Parameter | Type | Default | Description |
| --------- | ---- | ------- | ----------- |
| `bed_width` | f64 | 200.0 | Bed width in mm |
| `bed_height` | f64 | 200.0 | Bed height in mm |
| `feed_rate` | f64 | 3000.0 | Feed rate (mm/min) |

## SVG (.svg)

The most versatile output format.
Also accepted by the Oasis Mini app (upload at app.grounded.so).
Useful for plotters, laser cutters, vinyl cutters, or viewing in a browser.

### Format Specification

- Standard SVG XML
- Optional `<title>` element with the source image name (for accessibility and file manager identification)
- Optional `<desc>` element with pipeline parameters and export timestamp
- Optional `<metadata>` element containing the full `PipelineConfig` as JSON, wrapped in a namespaced `<mujou:pipeline>` element for machine-parseable reproducibility
- Each polyline becomes a `<path>` element with a `d` attribute containing `M` (move to) and `L` (line to) commands
- Disconnected contours are separate `<path>` elements
- SVG uses +Y down; normalized space uses +Y up. The Y-axis is flipped
  at the SVG boundary (either via `transform` or by negating Y in path data).

### Coordinate Modes

**Circle mask (Oasis template):**

The Oasis Mini app expects a 200mm × 200mm SVG with a 195mm diameter
circle. The transform from normalized space:

- Circle radius 1 → 97.5mm (half of 195mm)
- Document center at (100, 100) in SVG coordinates
- `svg_x = norm_x × 97.5 + 100`
- `svg_y = -norm_y × 97.5 + 100` (Y flip)
- `viewBox="0 0 200 200"`, `width="200mm"`, `height="200mm"`

**Rectangle mask:**

Scale to target document dimensions (mm). The shorter mask side (half-extent = 1)
maps to the shorter document dimension. The longer side maps proportionally.

**Generic (no specific device):**

`viewBox` in normalized coordinates with Y-flip. For a circle:
`viewBox="-1 -1 2 2"` with a `transform="scale(1,-1)"` on the path group,
or negate Y values in path data.

### Example

```xml
<?xml version="1.0" encoding="UTF-8"?>
<svg height="200mm" viewBox="0 0 200 200" width="200mm" xmlns="http://www.w3.org/2000/svg">
<title>cherry-blossoms</title>
<desc>blur=1.4, canny=15/40, simplify=0.005, tracer=BorderFollowing, joiner=Mst, zoom=1.25, res=256
Exported: 2026-02-14_12-30-45</desc>
<metadata>
<mujou:pipeline xmlns:mujou="https://mujou.app/ns/1">{"blur_sigma":1.4,"canny_low":15.0,...}</mujou:pipeline>
</metadata>
<path d="M110.5,82.4 L112.2,85.1 L113.8,87.0" fill="none" stroke="black" stroke-width="0.5"/>
</svg>
```

> **Note:** SVG output is generated by the [`svg`](https://crates.io/crates/svg)
> crate. Attribute ordering is determined by the library (typically alphabetical).
> Path coordinates use the library's default `f32` precision formatting.
> The JSON inside `<mujou:pipeline>` is XML-escaped — `<` becomes `&lt;`,
> `&` becomes `&amp;`, etc. Parsers should XML-unescape the text content
> before JSON-parsing it.

## DXF (.dxf)

CAD interchange format for OnShape, Fusion 360, etc.

### Coordinate Transform

Normalized coordinates are emitted directly (unitless) or scaled to a
target dimension. DXF uses a right-hand coordinate system with +Y up,
matching the normalized coordinate system, so **no Y-flip is required**.
DXF viewers interpret units based on the `$INSUNITS` header variable;
mujou emits unitless coordinates matching the normalized space
(circle: radius 1, centered at origin).

### Format Specification

- Minimal DXF using `LINE` entities in the `ENTITIES` section
- Each segment of each polyline becomes a `LINE` entity
- ASCII DXF format (not binary)

### Example

```text
0
SECTION
2
ENTITIES
0
LINE
8
0
10
10.0
20
15.0
11
12.5
21
18.3
0
LINE
8
0
10
12.5
20
18.3
11
14.0
21
20.1
0
ENDSEC
0
EOF
```

## PNG Preview

Rasterized render of the traced paths for quick sharing and thumbnailing.

### Specification

- Render normalized-space polylines onto a pixel buffer using the `image` crate
- Map normalized coordinates to pixel coordinates (with Y-flip):

  - **Circle or square rectangle (both axes span [-1, 1]):**
    `pixel = (norm + 1) × output_resolution / 2`
  - **Rectangle mask (landscape, `aspect_ratio = A`):**
    `pixel_x = (norm_x / A + 1) × output_width  / 2` — `output_width  = output_resolution × A`
    `pixel_y = (norm_y     + 1) × output_height / 2` — `output_height = output_resolution`
  - **Rectangle mask (portrait):** swap width/height roles
- White background, black strokes (or configurable colors)
- Output as PNG-encoded bytes
- Resolution configurable (default: working resolution)

## Live SVG Preview (UI)

In the browser UI, traced paths are rendered as inline SVG elements directly
in the Dioxus DOM. The preview SVG `viewBox` matches the mask bounding box in
normalized coordinates:

- **Circle mask:** `viewBox="-1 -1 2 2"` — the circle fills the entire preview
- **Rectangle mask:** viewBox covers the rectangle extent (e.g.,
  `viewBox="-2 -1 4 2"` for aspect ratio 2 in landscape)

The Y-axis is flipped at the SVG boundary (normalized +Y up → SVG +Y down)
via `transform="scale(1,-1)"` on the path group or by negating Y values.

`preserveAspectRatio="xMidYMid meet"` ensures the preview scales to fit the
container while maintaining proportions.

No screen space is wasted on clipped-away content — the mask shape always
fills the entire preview viewport.

The preview uses a simplified version of the paths (higher RDP tolerance) to keep the DOM lightweight when the full path set is very large.

### Preview Modes

| Mode | Description |
| ---- | ----------- |
| Original | Source image displayed as-is |
| Edges | Binary edge map (Canny output) |
| Paths | Traced polylines overlaid on original |
| Paths only | Traced polylines on blank background |
