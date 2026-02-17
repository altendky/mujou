# Existing Software

Tools already in the kinetic sand table ecosystem.
These are prior art, potential interop targets, and reference implementations for mujou.

## Sandify

- **Website**: [sandify.org](https://sandify.org/)
- **Source**: [github.com/jeffeb3/sandify](https://github.com/jeffeb3/sandify)
- **Author**: Jeff Eberl (V1 Engineering community)
- **License**: MIT

Sandify is a web-based **pattern generator** for sand tables. Users configure mathematical patterns (spirals, wiper, star, etc.) via sliders and visual controls, then export the resulting path.

### Output Formats

- G-code (for ZenXY, GRBL/Marlin machines)
- .thr (for Sisyphus tables and polar builds)
- SVG
- SCARA G-code (experimental)

### Machine Configuration

Sandify does **not** have built-in device presets. Users manually configure machine dimensions per-session (persisted in browser localStorage):

- **Polar machines**: `maxRadius` (mm, default 250), `polarStartPoint`, `polarEndPoint`
- **Rectangular machines**: `minX`, `maxX`, `minY`, `maxY` (mm, defaults 0--500)

Multiple machine configurations can be saved and switched between.

### Export Details

- **THR**: 5 decimal digits, `#` comments, `polarRhoMax` setting (default 1.0) can pull patterns inward
- **SVG**: Unitless `width`/`height` attributes matching machine dimensions; `<desc>pwidth:...;pheight:...;</desc>` metadata; `stroke-width="0.4mm"`
- **G-code**: `G1 X... Y...` commands, 3 decimal digits, `;` comments. Even polar machines export Cartesian G-code. Feed rate is set via user-provided pre/post code blocks, not a dedicated setting.

### Relationship to mujou

Sandify generates patterns from mathematical functions. mujou converts **raster images** into patterns. They are complementary tools -- Sandify for geometric/algorithmic art, mujou for photo-derived art. Users of the same sand tables would use both.

Sandify's export formats (.thr, G-code, SVG) are the same ones mujou targets, confirming these are the right formats to support. Its machine configuration model (polar vs. rectangular, user-defined dimensions) is a useful reference for mujou's eventual device preset system.

### Key Source Files

| File | Content |
| --- | --- |
| `src/common/geometry.js` | `toThetaRho()` conversion, `subsample()`, `atan2(x,y)` convention |
| `src/features/export/ThetaRhoExporter.js` | THR export: 5-digit precision, `#` comments |
| `src/features/export/SvgExporter.js` | SVG export: centering, Y-flip, `pwidth`/`pheight` metadata |
| `src/features/export/GCodeExporter.js` | G-code export: Cartesian XY, 3-digit precision |
| `src/features/machines/PolarMachine.js` | Polar machine config (`maxRadius`, start/end points) |
| `src/features/machines/RectMachine.js` | Rectangular machine config (`minX`/`maxX`/`minY`/`maxY`) |

## jsisyphus (SisyphusForTheRestOfUs)

- **Source**: [github.com/markyland/SisyphusForTheRestOfUs](https://github.com/markyland/SisyphusForTheRestOfUs)
- **Author**: Mark Highland
- **License**: Not specified

A Java library for generating Sisyphus table patterns programmatically. Provides a `DrawingContext` API with primitives (lines, arcs, spirals) that output .thr files.

### Relevance to mujou

jsisyphus's `Point.java` is an authoritative reference for the .thr polar coordinate convention. Its documentation explicitly states: *"The zero radial is coincident with the positive y axis, and positive angles increase clockwise from there."* This confirms the `atan2(x, y)` convention used by the ecosystem. See [Output Formats](../project/formats.md#xy-to-polar-conversion).

### Key Source Files

| File | Content |
| --- | --- |
| `src/.../Point.java` | Polar coordinate convention, `fromXY()` / `fromRT()` conversions |
| `src/.../Utils.java` | `getTheta()` -- equivalent to `atan2(x, y)` |
| `src/.../DrawingContext.java` | Drawing primitives that generate .thr output |

## Image2Sand (Orion)

- **Website**: [orionwc.github.io/Image2Sand](https://orionwc.github.io/Image2Sand/)
- **Source**: [github.com/OrionWC/Image2Sand](https://github.com/OrionWC/Image2Sand)
- **Author**: ORION
- **License**: Not specified

Image2Sand is a web-based tool that converts images to sand table patterns. It is the **most direct prior art** for mujou.

### Output Formats

- "Default" (CrunchLabs Sand Garden proprietary format)
- "Single-Byte" (compact binary encoding)
- .thr (Theta-Rho for Sisyphus-compatible tables)
- "Whitespace" (space-separated coordinates)

### Relationship to mujou

mujou aims to be a more capable replacement for Image2Sand, with:

- Better image processing (Gaussian blur, Canny edge detection, contour tracing vs. simple threshold-based conversion)
- More output formats (G-code, SVG, DXF in addition to .thr)
- Configurable pipeline with live preview
- Modern Rust/WASM architecture for performance

Image2Sand's CrunchLabs "Default" format is a potential format for mujou to support if there is demand from Sand Garden owners.

## sandsara-hacs

- **Source**: [github.com/monxas/sandsara-hacs](https://github.com/monxas/sandsara-hacs)
- **Author**: monxas
- **License**: Not specified

A Home Assistant integration for controlling the Sandsara Mini Pro over BLE. Includes reverse-engineered protocol documentation, a Python CLI controller, and a web-based pattern viewer with upload capability.

### Relevance to mujou

This is the primary reference for the Sandsara Mini Pro's proprietary binary `.bin` pattern format and BLE file transfer protocol. See [File Formats by Device](formats.md#sandsara) for the format details.

### Key Resources

| File | Content |
| --- | --- |
| `docs/PROTOCOL_NOTES.md` | Reverse-engineered BLE protocol (UUIDs, commands) |
| `docs/FILE_TRANSFER_PROTOCOL.md` | BLE file transfer: 512-byte chunks with ACK |
| `research/` | Pattern file format analysis |
| `tools/` | Python CLI controller and test server |

## fly115/Image2Sand

- **Source**: [github.com/fly115/Image2Sand](https://github.com/fly115/Image2Sand) (unconfirmed)
- **Author**: fly115

An alternative Image2Sand implementation using an Excel macro. Outputs .gcode and .thr.

### Relationship to mujou

Demonstrates demand for image-to-sand conversion across different tool preferences (Excel macro vs. web app vs. native app). The existence of multiple independent tools solving this problem validates the use case.
