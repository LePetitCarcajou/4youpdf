//! `fyp merge`: the page selection of `--pages` and what the command
//! refuses. Goes through the binary, so a refusal is also checked to
//! write no file at all.

#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use fyp_core::document::Document;
use fyp_core::ops::{self, Selection};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> PathBuf {
    root().join("tests/fixtures").join(name)
}

/// An empty directory of this test run, removed by the caller.
fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fyp-merge-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `fyp merge <args…>`, run from the root of the repository.
fn merge(args: Vec<&OsStr>) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fyp"))
        .current_dir(root())
        .arg("merge")
        .args(args)
        .output()
        .expect("run fyp merge")
}

/// The bytes of each of `paths`, which the documents opened on them
/// borrow.
fn read_all(paths: &[&Path]) -> Vec<Vec<u8>> {
    paths.iter().map(|p| std::fs::read(p).unwrap()).collect()
}

#[test]
fn without_a_selection_the_command_writes_what_ops_merge_writes() {
    let dir = temp_dir("plain");
    let out = dir.join("fusion.pdf");
    let (a, b) = (fixture("minimal.pdf"), fixture("mixed12.pdf"));
    let result = merge(vec![
        a.as_os_str(),
        b.as_os_str(),
        OsStr::new("-o"),
        out.as_os_str(),
    ]);
    let stderr = String::from_utf8_lossy(&result.stderr).into_owned();
    assert!(result.status.success(), "{stderr}");
    let files = read_all(&[&a, &b]);
    let docs: Vec<Document<'_>> = files.iter().map(|f| Document::open(f).unwrap()).collect();
    assert!(
        std::fs::read(&out).unwrap() == ops::merge(&docs).unwrap(),
        "differs from ops::merge"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pages_takes_the_selection_of_each_input_in_order() {
    let dir = temp_dir("selection");
    let out = dir.join("fusion.pdf");
    let (a, b) = (fixture("minimal.pdf"), fixture("mixed12.pdf"));
    // Every page of the first file, then the last three of the second in
    // reverse order.
    let result = merge(vec![
        a.as_os_str(),
        b.as_os_str(),
        OsStr::new("--pages"),
        OsStr::new("all"),
        OsStr::new("--pages"),
        OsStr::new("12-10"),
        OsStr::new("-o"),
        out.as_os_str(),
    ]);
    let stderr = String::from_utf8_lossy(&result.stderr).into_owned();
    assert!(result.status.success(), "{stderr}");
    let written = std::fs::read(&out).unwrap();
    let files = read_all(&[&a, &b]);
    let docs: Vec<Document<'_>> = files.iter().map(|f| Document::open(f).unwrap()).collect();
    let expected =
        ops::merge_selected(&docs, &[Selection::All, Selection::Pages(vec![11, 10, 9])]).unwrap();
    assert!(written == expected, "differs from ops::merge_selected");
    assert_eq!(Document::open(&written).unwrap().page_count(), Ok(4));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn refusals_say_what_is_wrong_and_write_nothing() {
    let dir = temp_dir("refusals");
    let out = dir.join("fusion.pdf");
    let (a, b) = (fixture("minimal.pdf"), fixture("mixed12.pdf"));
    let cases: [(&[&str], &str); 4] = [
        // One --pages for two files.
        (&["--pages", "1-3"], "une sélection par fichier"),
        // A page the second file does not have.
        (&["--pages", "all", "--pages", "13"], "hors limites"),
        // Not a number.
        (&["--pages", "all", "--pages", "2-x"], "numéro de page"),
        // An empty selection: the CLI names `all` rather than merging
        // nothing.
        (&["--pages", "all", "--pages", " "], "aucune page indiquée"),
    ];
    for (extra, expected) in cases {
        let mut args: Vec<&OsStr> = vec![a.as_os_str(), b.as_os_str()];
        args.extend(extra.iter().map(OsStr::new));
        args.push(OsStr::new("-o"));
        args.push(out.as_os_str());
        let result = merge(args);
        let stderr = String::from_utf8_lossy(&result.stderr).into_owned();
        assert!(!result.status.success(), "{expected:?} accepted:\n{stderr}");
        assert!(
            stderr.contains(expected),
            "{expected:?} missing from:\n{stderr}"
        );
        assert!(!out.exists(), "{expected:?}: a file was written");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
