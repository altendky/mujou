# Development

## Prerequisites

- Rust (edition 2024, see `rust-toolchain.toml` for pinned version)
- `wasm32-unknown-unknown` target: `rustup target add wasm32-unknown-unknown`
- Dioxus CLI: `cargo install dioxus-cli` or `cargo binstall dioxus-cli`
- Node.js / npm (for Tailwind CSS)

## Local Development

```bash
# Start Dioxus dev server (web target)
# Tailwind CSS is compiled by build.rs via `npx @tailwindcss/cli` so that
# every cargo invocation (clippy, test, coverage, dx serve, etc.) works
# without relying on the Dioxus CLI's bundled Tailwind.
# See: https://github.com/altendky/mujou/issues/12
#
# Shared theme assets (site/theme.css, site/theme-toggle.js, site/theme-detect.js)
# are copied to OUT_DIR by build.rs and injected via include_str!().
# build.rs also generates crates/mujou/index.html (gitignored) with the
# theme-detect script inlined in <head> to prevent flash of wrong theme.
dx serve --platform web --package mujou

# Format
cargo fmt

# Lint
cargo clippy --all-targets --all-features -- -D warnings

# Test core crates (no WASM runtime needed)
cargo nextest run --all-features

# Coverage
cargo llvm-cov --all-features --workspace
```

### Installing Development Tools

```bash
# Dioxus CLI
cargo install dioxus-cli

# cargo-nextest (test runner)
cargo install cargo-nextest --locked

# cargo-llvm-cov (coverage)
cargo install cargo-llvm-cov --locked

# cargo-deny (dependency audit)
cargo install cargo-deny --locked
```

## Testing Strategy

| Test Type | Location | Coverage Target |
| --------- | -------- | --------------- |
| Unit tests | `crates/*/src/**/*.rs` | 100% with exclusions |
| Integration tests | `crates/mujou/tests/` | Key workflows |

### Core Crate Testing

Core crates (`mujou-pipeline`, `mujou-export`) are fully testable without a browser or WASM runtime.
All functions are pure: deterministic inputs produce deterministic outputs.

```bash
# Test just the pipeline crate
cargo nextest run -p mujou-pipeline

# Test just the export crate
cargo nextest run -p mujou-export
```

### Test Patterns

- Synthetic test images (e.g., a white rectangle on black background) for predictable edge detection output
- Known-good polyline inputs for export format tests
- Round-trip tests where applicable (e.g., export to SVG, verify SVG structure)

## Coverage Requirements

- **Tool:** `cargo-llvm-cov`
- **Target:** 100% with explicit exclusions for untestable code
- **Enforcement:** Ratchet -- fail if coverage drops more than 2% from main; new code must be fully covered or explicitly excluded

### Coverage Exclusions

Use LCOV comments with justification:

```rust
// Platform-specific WASM code not testable in native tests
some_wasm_only_code(); // LCOV_EXCL_LINE

// LCOV_EXCL_START -- web-sys DOM interaction, tested manually in browser
fn trigger_download(...) { ... }
// LCOV_EXCL_STOP
```

## Workspace Lints

```toml
[workspace.lints.rust]
unsafe_code = "deny"

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
nursery = { level = "warn", priority = -1 }
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
```

## Pre-commit Hooks

**Philosophy:** Pre-commit hooks provide developers an opt-in mechanism for fast local feedback.
They do **not** enforce policy -- CI is the source of truth.

| Hook | Stage | Purpose |
| ---- | ----- | ------- |
| `trailing-whitespace` | pre-commit | Clean whitespace |
| `end-of-file-fixer` | pre-commit | Consistent EOF |
| `check-toml` | pre-commit | TOML syntax |
| `check-yaml` | pre-commit | YAML syntax |
| `check-merge-conflict` | pre-commit | Catch conflict markers |
| `typos` | pre-commit | Spell checking |
| `markdownlint-cli2` | pre-commit | Markdown linting |
| `cargo fmt --check` | pre-commit | Formatting |
| `cargo clippy` | pre-commit | Linting |
| `cargo nextest run` | manual | Tests |
| `cargo deny` | manual | Dependency audit |
