# File Formats by Device

This is the device-compatibility view of file formats.
For mujou's implementation details of each format, see [Output Formats](../project/formats.md).

## Format Compatibility Matrix

| Device | .thr | G-code | SVG | Proprietary | Ingestion Method |
| --- | --- | --- | --- | --- | --- |
| Sisyphus round tables (all) | Yes | -- | Yes (via [webcenter](https://webcenter.sisyphus-industries.com/)) | -- | Sisyphus app (Wi-Fi upload) |
| Sisyphus XYLA tables | [No](#sisyphus-xyla-thr-does-not-work) | Yes | Yes (via webcenter) | -- | Sisyphus app (Wi-Fi upload) |
| Oasis Mini | Yes | -- | [Partial*](#oasis) (via [app.grounded.so](https://app.grounded.so)) | -- | Oasis app / web upload |
| Oasis Side/Coffee Table | Yes | -- | [Partial*](#oasis) (via app) | -- | Oasis app / web upload |
| Sandsara (original firmware) | Yes | -- | -- | -- | SD card |
| Sandsara Mini Pro (current) | -- | -- | -- | Yes ([.bin](#sandsara)) | BLE only (via app) |
| CrunchLabs Sand Garden | -- | -- | -- | Yes (single-byte) | SD card / direct upload |
| Dune Weaver (all models) | Yes | -- | -- | -- | Web UI (Wi-Fi upload) |
| ZenXY v2 | -- | Yes (GRBL/Marlin) | -- | -- | SD card / serial / wireless (ESP32) |
| rdudhagra Sand-Table | Yes | Yes (Marlin) | -- | -- | Web UI (Raspberry Pi) |

## Key Takeaways

- **.thr is the dominant format** for polar sand tables. Supporting it covers Sisyphus (round), Oasis, Dune Weaver, and most DIY polar builds. This is mujou's primary export target.
- **G-code covers Cartesian tables** like ZenXY and other GRBL/Marlin machines.
- **SVG is a useful secondary format** -- Sisyphus accepts it via their webcenter, and it's universally viewable. Oasis accepts SVG via their web app but THR is preferred because SVG sizing and centering can be incorrect in certain cases (see [Oasis SVG sizing](#oasis)).
- **.thr does NOT work for Sisyphus XYLA** (rectangular/racetrack tables). SVG or G-code is required.
- **Sandsara has two generations** -- the original firmware accepts .thr from SD card; the current Mini Pro uses a proprietary binary format over BLE.

## SVG Sizing by Device

SVG export needs device-appropriate document sizing. The requirements differ by manufacturer.

### Oasis

> **THR is the recommended format for Oasis.** SVG upload is supported but requires precise document sizing and centering that can be wrong in certain cases. THR avoids these issues entirely.

The Oasis Mini requires specific mm-based SVG dimensions (sourced from the template on [app.grounded.so](https://app.grounded.so), behind login):

| Model | SVG Document Size | Circle Diameter | Margin | Status |
| --- | --- | --- | --- | --- |
| Oasis Mini | 200mm x 200mm | 195mm | 2.5mm per side | **Confirmed** (in use) |
| Oasis Side Table | Unknown | Unknown | Unknown | Ships March 2026 (as of Feb 2026) |
| Oasis Coffee Table | Unknown | Unknown | Unknown | Ships March 2026 (as of Feb 2026) |

The 200mm value comes from the Oasis template file. The 195mm circle diameter leaves a 2.5mm margin per side -- this likely accounts for ball clearance but is not yet confirmed exactly. mujou currently hardcodes these values in `svg.rs` with a TODO to generalize. If the sizing or centering is even slightly off, the pattern may be clipped or misaligned on the table.

### Sisyphus

Sisyphus's importer **auto-centers and auto-scales** SVGs to fit the table. Absolute document dimensions do not matter -- only the aspect ratio and relative geometry of paths within the viewBox matter.

[Sandify](https://sandify.org/) (the dominant community pattern tool) outputs SVGs with unitless `width`/`height` attributes set to the user's configured machine dimensions in mm (e.g., `width="500"` for a 250mm-radius table). A `<desc>pwidth:500;pheight:500;</desc>` metadata tag records the intended physical size but appears informational only.

The Sisyphus admin confirmed on the [forum](https://sisyphus-industries.com/forum/): *"The track will automatically be scaled to fit within the bounds no matter which table you have."*

### Sisyphus XYLA: THR Does Not Work

The .thr format is inherently polar (angle + radius from center) and does **not** map correctly to the XYLA's rectangular/racetrack sand field. The Sisyphus admin recommends exporting as SVG or G-code instead. When uploading an SVG for XYLA, the importer centers the design and maintains aspect ratio.

XYLA aspect ratios: Metal ~2.25:1 (914mm x 406mm), Hardwood ~2.34:1 (1245mm x 533mm).

### Dune Weaver

Dune Weaver does **not** accept SVG files. Only .thr is supported.

## THR Ecosystem Notes

The .thr format uses normalized rho (0.0--1.0) and continuous theta (radians), so it is inherently device-independent -- no physical dimensions appear in the file. However, there are implementation details worth noting:

- **Subsampling before polar conversion**: [Sandify](https://github.com/jeffeb3/sandify) breaks long line segments into shorter sub-segments (max length 2.0 mm in machine coordinates) before converting from Cartesian to polar coordinates. This prevents angular artifacts where a long straight XY segment maps to an unexpected arc in theta-rho space. mujou should do the same.
- **No rho clamping on consumption**: Dune Weaver passes rho values through to the motor controller without validation. If a .thr file contains rho > 1.0, it will attempt to drive the ball beyond the physical edge. Producers should ensure rho stays within [0.0, 1.0].
- **`atan2(x, y)` convention**: The ecosystem uses `atan2(x, y)` (theta=0 points up / Y+), not the standard math `atan2(y, x)`. See [Output Formats](../project/formats.md#xy-to-polar-conversion) for details.

## Sandsara

Sandsara has two distinct firmware generations with different format support.

### Original Firmware (open-source, ~2021)

The original ESP32-based firmware ([GitHub](https://github.com/Sandsara/firmwareSandsara)) directly supports:

- `.thr` files (theta-rho pairs, read from SD card)
- `.bin` files (binary format)
- `.txt` files (text format)

These are read from the SD card. The firmware also supports BLE file transfer.

### Mini Pro Firmware (current models as of Feb 2026, proprietary)

The Mini Pro uses a proprietary binary `.bin` format transferred exclusively over Bluetooth Low Energy (BLE). The format has been reverse-engineered by the [sandsara-hacs](https://github.com/monxas/sandsara-hacs) project (a Home Assistant integration):

- No header, raw binary data
- 6 bytes per coordinate point:
  - Bytes 0--1: X coordinate (int16, little-endian)
  - Byte 2: Comma separator (0x2C)
  - Bytes 3--4: Y coordinate (int16, little-endian)
  - Byte 5: Newline (0x0A)
- Coordinates range from -32768 to +32767, representing positions on a unit circle

Pattern files are named `Sandsara-trackNumber-XXXX.bin` (XXXX = 4-digit number).

There is **no web portal** (unlike Oasis's app.grounded.so). Pattern management is through the mobile app only:

- iOS: [App Store](https://apps.apple.com/us/app/the-sandsara-app/id6477485076)
- Android: [Google Play](https://play.google.com/store/apps/details?id=com.ht117.sandsaras)

The app's pattern creator is described as "powered by Sandify," suggesting it uses Sandify's algorithms internally and converts to the .bin format before transfer.

### CrunchLabs Sand Garden (Single-Byte Format)

Image2Sand's "Default" output mode generates a format specifically for the CrunchLabs Sand Garden. The format appears to use single-byte encoding rather than text-based theta-rho pairs.

**Status**: The Image2Sand source code is the primary reference for reverse-engineering this format.
