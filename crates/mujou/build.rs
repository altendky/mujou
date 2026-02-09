// Build scripts signal errors by panicking — there is no caller to
// return Result to.  Cargo treats a non-zero exit as a build failure.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Build script for the mujou binary crate.
//!
//! Copies shared theme assets from `site/` into `OUT_DIR` so that
//! `main.rs` can `include_str!` them via stable environment variable
//! paths (instead of fragile `../../../site/` relative paths).
//!
//! Also generates `index.html` at the crate root with the early theme
//! detection script inlined in `<head>`, ensuring no flash of wrong
//! theme before CSS is applied.

use std::path::{Path, PathBuf};
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

    // Copy each shared asset to OUT_DIR and expose its path as a cargo env var.
    for &(filename, env_key) in SITE_ASSETS {
        let src = site_dir.join(filename);
        let dst = out_dir.join(filename);

        println!("cargo:rerun-if-changed={}", src.display());
        fs::copy(&src, &dst).unwrap_or_else(|e| {
            panic!("failed to copy {} to {}: {e}", src.display(), dst.display())
        });
        println!("cargo:rustc-env={env_key}={}", dst.display());
    }

    // Generate crates/mujou/index.html with the detect script inlined.
    let detect_js = fs::read_to_string(site_dir.join("theme-detect.js"))
        .expect("failed to read site/theme-detect.js");

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
