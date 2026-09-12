//! `fyp run`: the whole path, from the command line through the WebAssembly
//! module and back through re-validation.
//!
//! Needs `plugins/merge/module.wasm` (`python tools/build_modules.py`);
//! skipped without it unless `FYP_REQUIRE_MODULES` is set.

#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use fyp_core::document::Document;
use fyp_core::ops;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn run_merge_goes_through_the_wasm_module() {
    let root = root();
    if !root.join("plugins/merge/module.wasm").is_file() {
        assert!(
            std::env::var_os("FYP_REQUIRE_MODULES").is_none(),
            "plugins/merge/module.wasm missing: run python tools/build_modules.py"
        );
        eprintln!("plugins/merge/module.wasm not built (python tools/build_modules.py): skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("fyp-run-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let output = dir.join("fusion.pdf");
    let a = root.join("tests/fixtures/minimal.pdf");
    let b = root.join("tests/fixtures/objstm.pdf");
    let result = Command::new(env!("CARGO_BIN_EXE_fyp"))
        .current_dir(&root)
        .args(["run", "merge"])
        .arg(&a)
        .arg(&b)
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(result.status.success(), "{stdout}\n{stderr}");
    assert!(stdout.contains("org.4youpdf.merge"), "{stdout}");
    let written = std::fs::read(&output).unwrap();
    let files = [std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap()];
    let docs: Vec<Document<'_>> = files.iter().map(|f| Document::open(f).unwrap()).collect();
    assert!(
        written == ops::merge(&docs).unwrap(),
        "differs from ops::merge"
    );

    // An action no module declares: a clear refusal, nothing written.
    let missing = dir.join("absent.pdf");
    let result = Command::new(env!("CARGO_BIN_EXE_fyp"))
        .current_dir(&root)
        .args(["run", "compress"])
        .arg(&a)
        .arg("-o")
        .arg(&missing)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("compress"));
    assert!(!missing.exists());
    let _ = std::fs::remove_dir_all(&dir);
}
