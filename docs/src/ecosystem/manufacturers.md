# Manufacturers & Devices

## Commercial

| Manufacturer | Product(s) | Type | Price Range | File Format(s) | Website | Community Resources |
| --- | --- | --- | --- | --- | --- | --- |
| **Sisyphus Industries** | Metal Coffee/Side Tables, XYLA (Metal/MCM/Hardwood), Hardwood Coffee/Side Tables, Mini (LE/EX/ES) | Polar (round) / Racetrack (XYLA) | $690 -- $15,000 | .thr, SVG (via app) | [sisyphus-industries.com](https://sisyphus-industries.com/) | [r/SisyphusIndustries](https://reddit.com/r/SisyphusIndustries), [Forum](https://sisyphus-industries.com/forum/), [Zendesk Support](https://sisyphus-industries.zendesk.com/hc/en-us) |
| **Oasis (Grounded)** | Oasis Mini, Side Table, Coffee Table | Polar | $129 -- $999 | .thr, SVG (via [app.grounded.so](https://app.grounded.so)) | [grounded.so](https://grounded.so/) | [Instagram](https://instagram.com/oasis.mini), [YouTube](https://youtube.com/@oasismini), [TikTok](https://tiktok.com/@oasis.mini) |
| **Sandsara** | Mini, Mini Pro, Wireless Crystal, Wireless Dark Walnut | Polar | $169 -- $750 | .thr (original firmware), .bin (Mini Pro, proprietary) | [sandsara.com](https://www.sandsara.com/) | [Instagram](https://instagram.com/sandsara.art) |
| **CrunchLabs** | Sand Garden (appears discontinued) | Polar | ~$50 (was educational kit) | Proprietary single-byte format | [crunchlabs.com](https://www.crunchlabs.com/) | [YouTube](https://youtube.com/crunchlabs) (large audience via Mark Rober) |

## Open-Source / DIY

| Project | Product(s) | Type | Cost (DIY) | File Format(s) | Source | Community Resources |
| --- | --- | --- | --- | --- | --- | --- |
| **Dune Weaver** | DW Pro (75cm), DW Mini Pro (25cm), DW Gold (45cm) | Polar | ~$100 -- $300 parts | .thr | [GitHub](https://github.com/tuanchris/dune-weaver), [duneweaver.com](https://duneweaver.com/) | [Discord](https://discord.com/invite/YZ8PTezVHt), [Patreon](https://patreon.com/cw/DuneWeaver) |
| **V1 Engineering ZenXY** | ZenXY v2 (rectangular, CoreXY) | Cartesian | ~$200 -- $400 parts | G-code (GRBL/Marlin) | [Docs](https://docs.v1engineering.com/zenxy/), [GitHub](https://github.com/V1EngineeringInc/ZenXY-v2) | [V1E Forum](https://forum.v1e.com/), [Discord](https://discord.gg/kY9tv2Gzgp), [r/mpcnc](https://reddit.com/r/mpcnc), [Facebook](https://facebook.com/groups/1156276207801920) |
| **rdudhagra Sand-Table** | DIY platform (square, CoreXY) | Cartesian | ~$200 -- $750 | .thr, G-code (Marlin) | [GitHub](https://github.com/rdudhagra/Sand-Table) | [GitHub Discussions](https://github.com/rdudhagra/Sand-Table/discussions) |

## Physical Dimensions

Sand field diameter (or dimensions for rectangular tables) determines the resolvable detail level and affects SVG export sizing. See the [reference target device](../project/decisions.md#reference-target-device) analysis for how these translate to pipeline resolution.

### Oasis (Grounded)

| Model | Sand Diameter | Overall Size | Status |
| --- | --- | --- | --- |
| Oasis Mini | 9" / 234mm | 9" dia x 3" tall | Shipping (50,000+ units as of Feb 2026) |
| Oasis Side Table | 20" / 500mm | 20" dia x 21" tall | Pre-order, shipping March 2026 (as of Feb 2026) |
| Oasis Coffee Table | 34" / 850mm | 34" dia x 16" tall | Pre-order, shipping March 2026 (as of Feb 2026) |

### Sisyphus Industries -- Round Tables

| Model | Viewable Diameter (Sand Field) | Overall Size |
| --- | --- | --- |
| Mini LE / EX / ES | 9.9" / 252mm | 15--17" dia x ~4--5" tall |
| Hardwood Side Table | 15" / 381mm | 22" round x 22" tall |
| Metal Side Table | 16" / 406mm | 22" round x 22" tall |
| Hardwood Coffee Table (3ft) | 27" / 686mm | 36" round x 16" tall |
| Metal Coffee Table | 27.25" / 692mm | 36" round x 16" tall |
| Hardwood Coffee Table (4ft) | 38" / 965mm | 48" round x 16" tall |

### Sisyphus Industries -- XYLA Tables (Racetrack / Stadium)

XYLA tables have a rectangular sand field with rounded ends (stadium shape). The .thr format does **not** work correctly for XYLA -- SVG or G-code is required. See [formats](formats.md#sisyphus-xyla-thr-does-not-work).

| Model | Sand Field Dimensions | Aspect Ratio |
| --- | --- | --- |
| Metal XYLA | 914mm x 406mm | ~2.25:1 |
| Hardwood XYLA | 1245mm x 533mm | ~2.34:1 |

Dimensions sourced from Sisyphus admin (Matt Klundt) on the [Sisyphus forum](https://sisyphus-industries.com/forum/).

### Dune Weaver

Physical diameters are from the project README. The Dune Weaver software does not store physical dimensions -- it uses a normalized coordinate system (rho 0.0--1.0) with the mapping to physical radius handled entirely by motor gearing and `steps_per_mm` in the FluidNC firmware.

| Model | Physical Diameter | Enclosure |
| --- | --- | --- |
| DW Pro | 75cm / 29.5" | IKEA VITTSJOE table |
| DW Gold | 45cm / 17.7" | IKEA TORSJOE side table |
| DW Mini Pro | 25cm / 9.8" | IKEA BLANDA bowl |

### Sandsara

| Model | Sand Diameter (approx) |
| --- | --- |
| Mini / Mini Pro | ~8" / ~200mm |
| Wireless Dark Walnut | ~14" / ~360mm |

## Notes

- **Sisyphus Industries** is the original commercial kinetic sand table, inspired by Bruce Shapiro's art installations. Their .thr format has become the de facto standard for polar sand tables.
- **Oasis (Grounded)** targets a more affordable price point. The Oasis Mini ($129) is the most accessible commercial sand table. Side Table and Coffee Table are in pre-order, shipping March 2026 (as of Feb 2026).
- **Sandsara** has two firmware generations with different format support. The original open-source firmware ([GitHub](https://github.com/Sandsara/firmwareSandsara)) reads .thr files directly from the SD card. The newer Mini Pro firmware uses a proprietary binary `.bin` format transferred over BLE only. The official app includes a pattern creator described as "powered by Sandify." See [formats](formats.md#sandsara) for details on the binary format.
- **CrunchLabs Sand Garden** was a Mark Rober educational product. Though apparently discontinued, Image2Sand's "Default" output format was built specifically for it. The large CrunchLabs/Mark Rober YouTube audience means there may be many Sand Gardens in the wild.
- **Dune Weaver** is the most active open-source sand table project (282 GitHub stars, v4.0.5 as of Feb 2026). It uses a Raspberry Pi + DLC32/ESP32 running FluidNC, with a modern React web UI. All three models use IKEA furniture as the enclosure. Accepts .thr files only -- no SVG support.
- **V1 Engineering ZenXY** is a Cartesian (CoreXY) design, part of the broader V1 Engineering CNC ecosystem (MPCNC, LowRider). Uses Sandify for pattern generation.
- **rdudhagra Sand-Table** is a well-documented DIY Cartesian build (~$750) that is natively compatible with .thr files despite being a CoreXY design.
