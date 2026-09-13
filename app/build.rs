//! Build script: Tauri embeds `dist/` (the compiled interface) into the
//! binary at compile time, so the directory must exist even when the
//! interface has not been built (`tools/build_ui.py`). A placeholder page
//! saying so keeps `cargo build --workspace` working on a fresh checkout.
//!
//! `tauri.conf.json` has no `version`, so that the application and its
//! installer take the version of this crate, which is the workspace's.
//! tauri-build is the exception: it writes the version resource of the
//! Windows executable (the details of its file properties) from the
//! configuration only, merged with the JSON of `TAURI_CONFIG`, which is how
//! the Tauri CLI passes `--config`. The version is added there.

use std::path::Path;

const PLACEHOLDER: &str = "<!doctype html><meta charset=\"utf-8\">\
<title>4YouPDF</title>\
<p style=\"font-family: sans-serif; margin: 2em\">\
Interface non compilée : lancez <code>python tools/build_ui.py</code> \
puis recompilez l'application.</p>\n";

fn main() {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("dist");
    if !dist.join("index.html").exists() {
        let _ = std::fs::create_dir_all(&dist);
        let _ = std::fs::write(dist.join("index.html"), PLACEHOLDER);
    }
    println!("cargo:rerun-if-changed=dist");
    if let Some(config) = with_version(std::env::var("TAURI_CONFIG").ok().as_deref()) {
        std::env::set_var("TAURI_CONFIG", config);
    }
    tauri_build::build();
}

/// `config`, the JSON of `TAURI_CONFIG` if set, with the version of this
/// crate unless it gives one; `None` when it is not a JSON object, which
/// tauri-build then reports.
fn with_version(config: Option<&str>) -> Option<String> {
    let mut config = match config {
        Some(json) => serde_json::from_str(json).ok()?,
        None => serde_json::Value::Object(serde_json::Map::new()),
    };
    config
        .as_object_mut()?
        .entry("version")
        .or_insert_with(|| env!("CARGO_PKG_VERSION").into());
    Some(config.to_string())
}
