# mujou

A cross-platform Rust application that converts raster images into vector path files suitable for kinetic sand tables, pen plotters, laser cutters, and similar CNC devices.
The primary use case is converting photos and images into patterns that a sand table's steel ball can trace.

Deployed as a static WASM web app (primary target), with optional desktop and mobile builds from the same codebase.
All processing runs client-side in the browser.
No images leave the user's device.
No backend needed.

## Documentation

### Core Design

- [Principles](principles.md) -- Sans-IO philosophy, testability goals, dependencies policy
- [Architecture](architecture.md) -- Layer design, crate structure, workspace layout
- [Requirements](requirements.md) -- Platform targets, technology choices, toolchain

### Features

- [Image Processing Pipeline](pipeline.md) -- Processing steps, algorithms, tunable parameters
- [Output Formats](formats.md) -- THR, G-code, SVG, DXF, PNG specifications
- [UI Design](ui.md) -- Components, layout, interaction model

### Ecosystem

- [Overview](../ecosystem/index.md) -- Sand table manufacturers, devices, communities, and existing software

### Project Management

- [Development](development.md) -- Local development, testing, coverage
- [Implementation](implementation.md) -- Phase checklist, roadmap
- [Decisions](decisions.md) -- Resolved design decisions
- [Naming](naming.md) -- Name candidates, availability, thematic exploration
- [Open Questions](open-questions.md) -- Pending decisions, deferred items
