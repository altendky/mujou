// Build scripts signal errors by panicking — there is no caller to
// return Result to.  Cargo treats a non-zero exit as a build failure.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Build script for the mujou binary crate.
//!
//! ## Tailwind CSS compilation (see [issue #12])
//!
//! Runs `npx @tailwindcss/cli` to compile `crates/mujou/tailwind.css`
//! into `$OUT_DIR/assets/tailwind.css`.  This replaces the Dioxus
//! CLI's bundled Tailwind integration so we control the version and
//! every `cargo` invocation (clippy, test, coverage, etc.) can compile
//! without relying on `dx build` having run first.
//!
//! ## Shared theme assets
//!
//! Copies shared theme assets from `site/` into `OUT_DIR` so that
//! `main.rs` can `include_str!` them via stable environment variable
//! paths (instead of fragile `../../../site/` relative paths).
//!
//! ## Generated `index.html`
//!
//! Generates `index.html` at the crate root with the early theme
//! detection script inlined in `<head>`, ensuring no flash of wrong
//! theme before CSS is applied.
//!
//! [issue #12]: https://github.com/altendky/mujou/issues/12

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

/// Shared asset files that live in `site/` at the workspace root.
const SITE_ASSETS: &[(&str, &str)] = &[
    ("theme.css", "THEME_CSS_PATH"),
    ("theme-toggle.js", "THEME_TOGGLE_JS_PATH"),
    ("theme-detect.js", "THEME_DETECT_JS_PATH"),
];

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    // Workspace root is two levels up from crates/mujou/.
    // This assumes the crate lives at `<workspace>/crates/mujou/`.
    // If the directory structure changes (e.g. `crates/apps/mujou/`),
    // this will compute the wrong path — consider walking up
    // `manifest_dir.ancestors()` to find a `Cargo.toml` containing
    // `[workspace]` if the layout ever becomes more complex.
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("could not find workspace root");
    let site_dir = workspace_root.join("site");

    build_tailwind_css(&manifest_dir, workspace_root, &out_dir);
    copy_site_assets(&site_dir, &out_dir);
    copy_example_image(workspace_root, &out_dir);
    build_worker_wasm(workspace_root, &out_dir);
    generate_index_html(&site_dir, &manifest_dir);
}

/// Compile Tailwind CSS via `npx @tailwindcss/cli`.
///
/// Input:  `crates/mujou/tailwind.css`
/// Output: `$OUT_DIR/assets/tailwind.css`
///
/// Writing to `OUT_DIR` (rather than the source tree) avoids issues
/// with read-only source trees (Nix, CI sandboxes) and races from
/// concurrent `cargo` invocations.
///
/// The output path is exposed as `TAILWIND_CSS_PATH` for
/// `include_str!(env!("TAILWIND_CSS_PATH"))` in `main.rs`.
///
/// See: <https://github.com/altendky/mujou/issues/12>
fn build_tailwind_css(manifest_dir: &Path, workspace_root: &Path, out_dir: &Path) {
    let input = manifest_dir.join("tailwind.css");
    let assets_dir = out_dir.join("assets");
    let output = assets_dir.join("tailwind.css");

    // Ensure the assets directory exists.
    fs::create_dir_all(&assets_dir)
        .unwrap_or_else(|e| panic!("failed to create {}: {e}", assets_dir.display()));

    // Rerun when the Tailwind input file changes.
    println!("cargo:rerun-if-changed={}", input.display());

    // Rerun when .rs files in UI crates change, because Tailwind scans
    // them for utility class names via `@source "../"` in the input CSS.
    // Only `mujou` and `mujou-io` contain Tailwind utility classes;
    // core crates (`mujou-pipeline`, `mujou-export`) do not.
    let crates_dir = workspace_root.join("crates");
    register_rs_sources(&crates_dir.join("mujou"));
    register_rs_sources(&crates_dir.join("mujou-io"));

    let input_lossy = input.to_string_lossy();
    let output_lossy = output.to_string_lossy();
    let mut args: Vec<&str> = vec!["@tailwindcss/cli", "-i", &input_lossy, "-o", &output_lossy];

    let profile = env::var("PROFILE").unwrap_or_default();
    if profile == "release" {
        args.push("--minify");
    }

    let status = Command::new("npx")
        .args(&args)
        .status()
        .unwrap_or_else(|e| {
            panic!(
                "failed to run `npx @tailwindcss/cli`: {e}\n\
                 \n\
                 Tailwind CSS is compiled from build.rs and requires Node.js / npm.\n\
                 Install Node.js (https://nodejs.org/) and ensure `npx` is on PATH.\n\
                 See: https://github.com/altendky/mujou/issues/12"
            )
        });

    assert!(
        status.success(),
        "`npx @tailwindcss/cli` exited with {status}\n\
         \n\
         See: https://github.com/altendky/mujou/issues/12"
    );

    println!("cargo:rustc-env=TAILWIND_CSS_PATH={}", output.display());
}

/// Recursively emit `cargo:rerun-if-changed` for every `.rs` file
/// under `dir`.
fn register_rs_sources(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            register_rs_sources(&path);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}

/// Copy shared theme assets from `site/` into `OUT_DIR` and expose
/// their paths as cargo environment variables.
fn copy_site_assets(site_dir: &Path, out_dir: &Path) {
    for &(filename, env_key) in SITE_ASSETS {
        let src = site_dir.join(filename);
        let dst = out_dir.join(filename);

        println!("cargo:rerun-if-changed={}", src.display());
        fs::copy(&src, &dst).unwrap_or_else(|e| {
            panic!("failed to copy {} to {}: {e}", src.display(), dst.display())
        });
        println!("cargo:rustc-env={env_key}={}", dst.display());
    }
}

/// Copy the cherry blossoms example image into `OUT_DIR` so that
/// `main.rs` can `include_bytes!` it via a stable environment variable
/// path.  This bundles the image into the WASM binary so the app can
/// display example output on first load without requiring a user upload.
fn copy_example_image(workspace_root: &Path, out_dir: &Path) {
    let src = workspace_root.join("assets/examples/cherry-blossoms.png");
    let dst = out_dir.join("cherry-blossoms.png");

    println!("cargo:rerun-if-changed={}", src.display());
    fs::copy(&src, &dst)
        .unwrap_or_else(|e| panic!("failed to copy {} to {}: {e}", src.display(), dst.display()));
    println!("cargo:rustc-env=CHERRY_BLOSSOMS_PATH={}", dst.display());
}

/// Build the pipeline web worker WASM module via `wasm-pack`.
///
/// Compiles `crates/mujou-worker/` into a no-modules WASM package,
/// then copies the JS glue and WASM binary into `OUT_DIR` so they
/// can be embedded into the main app via `include_str!` and
/// `include_bytes!`.
///
/// At runtime the main app creates Blob URLs from the embedded data,
/// which avoids depending on the dev server to serve extra static files.
///
/// The output paths are exposed as environment variables:
/// - `WORKER_JS_PATH` — path to the JS glue (for `include_str!`)
/// - `WORKER_WASM_PATH` — path to the WASM binary (for `include_bytes!`)
fn build_worker_wasm(workspace_root: &Path, out_dir: &Path) {
    let worker_crate = workspace_root.join("crates/mujou-worker");
    let worker_pkg_dir = out_dir.join("worker-pkg");

    let pipeline_crate = workspace_root.join("crates/mujou-pipeline");

    // Rerun when the worker crate or its pipeline dependency changes.
    register_rs_sources(&worker_crate.join("src"));
    println!(
        "cargo:rerun-if-changed={}",
        worker_crate.join("Cargo.toml").display()
    );
    register_rs_sources(&pipeline_crate.join("src"));
    println!(
        "cargo:rerun-if-changed={}",
        pipeline_crate.join("Cargo.toml").display()
    );

    let js_path = worker_pkg_dir.join("mujou_worker.js");
    let wasm_path = worker_pkg_dir.join("mujou_worker_bg.wasm");

    // Skip the wasm-pack build if the output already exists and is
    // newer than all worker and pipeline source files.  This is
    // critical for dev speed: build.rs re-runs whenever *any*
    // registered file changes (including Tailwind-scanned .rs files
    // in mujou/mujou-io), but the worker only needs rebuilding when
    // its own source or its pipeline dependency changes.
    // Without this check, wasm-pack (10-30s) runs on every save.
    if wasm_path.exists() && js_path.exists() {
        let wasm_mtime = fs::metadata(&wasm_path).and_then(|m| m.modified()).ok();
        if let Some(wasm_mtime) = wasm_mtime {
            let worker_stale = is_any_newer_than(&worker_crate.join("src"), wasm_mtime)
                || fs::metadata(worker_crate.join("Cargo.toml"))
                    .and_then(|m| m.modified())
                    .is_ok_and(|t| t > wasm_mtime)
                || is_any_newer_than(&pipeline_crate.join("src"), wasm_mtime)
                || fs::metadata(pipeline_crate.join("Cargo.toml"))
                    .and_then(|m| m.modified())
                    .is_ok_and(|t| t > wasm_mtime);

            if !worker_stale {
                // Worker output is up to date — skip the expensive build.
                println!("cargo:rustc-env=WORKER_JS_PATH={}", js_path.display());
                println!("cargo:rustc-env=WORKER_WASM_PATH={}", wasm_path.display());
                return;
            }
        }
    }

    // Clear flags and wrappers that the host `cargo` (or tools like
    // cargo-llvm-cov) inject into the environment.  These target the
    // *host* toolchain and are incompatible with the wasm32-unknown-unknown
    // target that wasm-pack compiles for.
    //
    // `RUSTFLAGS` / `CARGO_ENCODED_RUSTFLAGS` -- may contain host-only
    //   codegen flags (e.g. `-C instrument-coverage`).
    //
    // `RUSTC_WRAPPER` / `RUSTC_WORKSPACE_WRAPPER` -- cargo-llvm-cov
    //   installs itself as a `RUSTC_WRAPPER` that injects
    //   `-C instrument-coverage --cfg=coverage` into every rustc
    //   invocation.  The wasm32 target lacks `profiler_builtins`, so the
    //   wrapper must not be inherited by the wasm-pack sub-build.
    let status = Command::new("wasm-pack")
        .args([
            "build",
            &worker_crate.to_string_lossy(),
            "--target",
            "no-modules",
            "--no-typescript",
            "--out-dir",
            &worker_pkg_dir.to_string_lossy(),
        ])
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .status()
        .unwrap_or_else(|e| {
            panic!(
                "failed to run `wasm-pack build`: {e}\n\
                 \n\
                 The web worker requires wasm-pack to compile.\n\
                 Install: cargo install wasm-pack\n\
                 See: https://rustwasm.github.io/wasm-pack/installer/"
            )
        });

    assert!(
        status.success(),
        "`wasm-pack build` for mujou-worker exited with {status}"
    );

    assert!(
        js_path.exists(),
        "expected worker JS at {}",
        js_path.display()
    );
    assert!(
        wasm_path.exists(),
        "expected worker WASM at {}",
        wasm_path.display()
    );

    println!("cargo:rustc-env=WORKER_JS_PATH={}", js_path.display());
    println!("cargo:rustc-env=WORKER_WASM_PATH={}", wasm_path.display());
}

/// Check if any file under `dir` has a modification time newer than
/// `reference`.  Used to skip expensive build steps when output is
/// already up to date.
fn is_any_newer_than(dir: &Path, reference: std::time::SystemTime) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if is_any_newer_than(&path, reference) {
                return true;
            }
        } else if path
            .extension()
            .is_some_and(|ext| ext == "rs" || ext == "toml")
            && fs::metadata(&path)
                .and_then(|m| m.modified())
                .is_ok_and(|t| t > reference)
        {
            return true;
        }
    }

    false
}

/// Generate `crates/mujou/index.html` with the theme-detect script
/// inlined in `<head>`.
///
/// Note: this writes to `manifest_dir` (the source tree) rather than
/// `OUT_DIR` because Dioxus CLI expects `index.html` at the crate
/// root for serving.  The file is gitignored.  This will fail in
/// read-only source trees (Nix, certain CI sandboxes).
fn generate_index_html(site_dir: &Path, manifest_dir: &Path) {
    let detect_js = fs::read_to_string(site_dir.join("theme-detect.js"))
        .expect("failed to read site/theme-detect.js");

    assert!(
        !detect_js.contains("</script"),
        "theme-detect.js must not contain '</script' — it is inlined in a <script> tag"
    );

    // Shared analytics snippet — single source of truth for both the
    // landing page (site/index.html, injected by the deploy workflow)
    // and the Dioxus app template (generated here).
    let analytics_html = fs::read_to_string(site_dir.join("analytics.html"))
        .expect("failed to read site/analytics.html");
    let analytics_html = analytics_html.trim_end();

    println!(
        "cargo:rerun-if-changed={}",
        site_dir.join("analytics.html").display()
    );

    let index_html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
  <head>
    <title>{{app_title}}</title>
    <meta content="text/html;charset=utf-8" http-equiv="Content-Type" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <meta charset="UTF-8" />
    <script>{detect_js}</script>
  </head>
  <body>
    <div id="main"></div>
{analytics_html}
  </body>
</html>
"#
    );

    let index_path = manifest_dir.join("index.html");
    fs::write(&index_path, index_html)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", index_path.display()));
}
