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

/// `root/<name>/` holding `manifest` and a module that does nothing.
fn install(root: &Path, name: &str, manifest: &str) {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("manifest.toml"), manifest).unwrap();
    let idle = wat::parse_str(r#"(module (memory (export "memory") 1) (func (export "_start")))"#)
        .unwrap();
    std::fs::write(dir.join("module.wasm"), idle).unwrap();
}

/// `fyp run merge` on two fixtures with the modules of `modules`: whether
/// it succeeded, and its standard error.
fn run_with_modules(modules: &Path, output: &Path) -> (bool, String) {
    let a = root().join("tests/fixtures/minimal.pdf");
    let result = Command::new(env!("CARGO_BIN_EXE_fyp"))
        .args(["run", "merge"])
        .arg(&a)
        .arg(&a)
        .arg("-o")
        .arg(output)
        .arg("--modules")
        .arg(modules)
        .output()
        .unwrap();
    (
        result.status.success(),
        String::from_utf8_lossy(&result.stderr).into_owned(),
    )
}

#[test]
fn host_refusals_say_what_is_wrong_and_what_to_change() {
    // Modules written here from the merge module's manifest: no
    // module.wasm needs to be built.
    let dir = std::env::temp_dir().join(format!("fyp-run-refusals-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let merge = std::fs::read_to_string(root().join("plugins/merge/manifest.toml")).unwrap();
    let output = dir.join("out.pdf");
    let shown = |stderr: &str, expected: &[&str]| {
        for part in expected {
            assert!(stderr.contains(part), "{part:?} missing from:\n{stderr}");
        }
    };

    // A time limit of a day, above the host's ten minutes.
    let greedy_manifest = merge
        .replace("org.4youpdf.merge", "org.example.greedy")
        .replace("timeout_ms = 60000", "timeout_ms = 86400000");
    assert!(greedy_manifest.contains("86400000"), "{merge}");
    let greedy = dir.join("greedy");
    install(&greedy, "greedy", &greedy_manifest);
    let (ok, stderr) = run_with_modules(&greedy, &output);
    assert!(!ok);
    shown(
        &stderr,
        &[
            "org.example.greedy",
            "timeout_ms = 86400000",
            "au plus 600000 ms",
            "manifest.toml",
        ],
    );
    assert!(!output.exists());

    // Two modules declaring one identifier: neither is loaded.
    let twin = merge.replace("org.4youpdf.merge", "org.example.twin");
    let twins = dir.join("twins");
    install(&twins, "twin-a", &twin);
    install(&twins, "twin-b", &twin);
    let (ok, stderr) = run_with_modules(&twins, &output);
    assert!(!ok);
    shown(
        &stderr,
        &["org.example.twin", "twin-a", "twin-b", "aucun n'est chargé"],
    );
    assert!(!output.exists());
    let _ = std::fs::remove_dir_all(&dir);
}
