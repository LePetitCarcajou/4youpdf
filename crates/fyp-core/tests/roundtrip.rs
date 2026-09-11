//! Milestone test: every fixture and corpus file must parse without error.
//! Once the writer exists, this test will also re-serialise each file and
//! compare the object graph of the output with the input.

#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

fn pdf_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")))
        .collect();
    v.sort();
    v
}

#[test]
fn fixtures_have_a_valid_header() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests");
    let mut checked = 0;
    for dir in ["fixtures", "corpus"] {
        for path in pdf_files(&root.join(dir)) {
            let bytes = std::fs::read(&path).expect("read fixture");
            let info = fyp_core::version::quick_info(&bytes)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!(info.has_eof_marker, "{}: missing %%EOF", path.display());
            checked += 1;
        }
    }
    assert!(checked >= 1, "no fixture found under tests/fixtures");
}

#[test]
fn minimal_fixture_objects_parse() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/minimal.pdf");
    let bytes = std::fs::read(root).expect("minimal.pdf");
    // Walk every `N G obj` by scanning; the xref layer will replace this.
    let mut offset = 0;
    let mut count = 0;
    while let Some(rel) = fyp_core::parser::find(&bytes[offset..], b" 0 obj") {
        // back up to the start of the object number
        let mut start = offset + rel;
        while start > 0 && bytes[start - 1].is_ascii_digit() {
            start -= 1;
        }
        let (_, obj) = fyp_core::parser::Parser::at(&bytes, start).parse_indirect().expect("indirect");
        assert!(obj.as_dict().is_some());
        count += 1;
        offset += rel + 6;
    }
    assert_eq!(count, 3);
}
