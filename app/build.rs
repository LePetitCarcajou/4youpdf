//! Build script: Tauri embeds `dist/` (the compiled interface) into the
//! binary at compile time, so the directory must exist even when the
//! interface has not been built (`tools/build_ui.py`). A placeholder page
//! saying so keeps `cargo build --workspace` working on a fresh checkout.

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
    tauri_build::build();
}
