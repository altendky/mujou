// Build scripts signal errors by panicking — there is no caller to
// return Result to.  Cargo treats a non-zero exit as a build failure.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Build script for the mujou binary crate.
//!
//! ## Tailwind CSS compilation (see [issue #12])
//!
//! Runs `npx @tailwindcss/cli` to compile `crates/mujou/tailwind.css`
//! into `crates/mujou/assets/tailwind.css`.  This replaces the Dioxus
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

    // Workspace root is two levels up from crates/mujou/
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("could not find workspace root");
    let site_dir = workspace_root.join("site");

    build_tailwind_css(&manifest_dir, workspace_root);
    copy_site_assets(&site_dir, &out_dir);
    generate_index_html(&site_dir, &manifest_dir);
}

/// Compile Tailwind CSS via `npx @tailwindcss/cli`.
///
/// Input:  `crates/mujou/tailwind.css`
/// Output: `crates/mujou/assets/tailwind.css` (gitignored)
///
/// The output path is exposed as `TAILWIND_CSS_PATH` for
/// `include_str!(env!("TAILWIND_CSS_PATH"))` in `main.rs`.
///
/// See: <https://github.com/altendky/mujou/issues/12>
fn build_tailwind_css(manifest_dir: &Path, workspace_root: &Path) {
    let input = manifest_dir.join("tailwind.css");
    let assets_dir = manifest_dir.join("assets");
    let output = assets_dir.join("tailwind.css");

    // Ensure the assets directory exists.
    fs::create_dir_all(&assets_dir)
        .unwrap_or_else(|e| panic!("failed to create {}: {e}", assets_dir.display()));

    // Rerun when the Tailwind input file changes.
    println!("cargo:rerun-if-changed={}", input.display());

    // Rerun when any .rs file under crates/ changes, because Tailwind
    // scans them for utility class names via `@source "../"` in the
    // input CSS.  This is broad but ensures the CSS output stays in
    // sync with class usage across all crates.
    let crates_dir = workspace_root.join("crates");
    register_rs_sources(&crates_dir);

    let status = Command::new("npx")
        .args([
            "@tailwindcss/cli",
            "-i",
            &input.to_string_lossy(),
            "-o",
            &output.to_string_lossy(),
        ])
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

/// Generate `crates/mujou/index.html` with the theme-detect script
/// inlined in `<head>`.
fn generate_index_html(site_dir: &Path, manifest_dir: &Path) {
    let detect_js = fs::read_to_string(site_dir.join("theme-detect.js"))
        .expect("failed to read site/theme-detect.js");

    assert!(
        !detect_js.contains("</script"),
        "theme-detect.js must not contain '</script' — it is inlined in a <script> tag"
    );

    let index_html = format!(
        r#"<!DOCTYPE html>
<html>
  <head>
    <title>{{app_title}}</title>
    <meta content="text/html;charset=utf-8" http-equiv="Content-Type" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <meta charset="UTF-8" />
    <script>{detect_js}</script>
  </head>
  <body>
    <div id="main"></div>
  </body>
</html>
"#
    );

    let index_path = manifest_dir.join("index.html");
    fs::write(&index_path, index_html)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", index_path.display()));
}
