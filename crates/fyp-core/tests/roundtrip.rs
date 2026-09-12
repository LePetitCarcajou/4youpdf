//! Milestone test: every fixture is opened, written back, and the result
//! is opened again. The second document must hold the same objects as the
//! first, need no repair, and write back to the very same bytes.
//!
//! Objects are compared as models, not as bytes: offsets move, `/Length`
//! becomes exact, compressed objects come out of their object streams.
//! Object streams and cross-reference streams of the source describe its
//! layout, not the document, so they are outside the comparison.

#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fyp_core::document::Document;
use fyp_core::object::{Name, ObjRef, Object};
use fyp_core::writer::{Writer, XrefStyle};
use fyp_core::xref::{SectionKind, XrefEntry};
use fyp_core::Error;

fn tests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests")
}

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
    let root = tests_dir();
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
    let bytes = std::fs::read(tests_dir().join("fixtures/minimal.pdf")).expect("minimal.pdf");
    let doc = Document::open(&bytes).expect("open");
    // Read every in-use object at the offset the xref gives.
    let mut count = 0;
    for (num, entry) in doc.xref().entries() {
        if let XrefEntry::InUse { gen, .. } = entry {
            let obj = doc
                .get(ObjRef { num, gen })
                .expect("get")
                .expect("listed object");
            assert!(obj.as_dict().is_some());
            count += 1;
        }
    }
    assert_eq!(count, 3);
}

/// Is `obj` an object stream or a cross-reference stream?
fn describes_file_layout(obj: &Object) -> bool {
    match obj {
        Object::Stream { dict, .. } => matches!(
            dict.get(&Name::new("Type")).and_then(Object::as_name),
            Some(n) if n.0 == b"ObjStm" || n.0 == b"XRef"
        ),
        _ => false,
    }
}

/// Every object the document can read, except those describing the file
/// layout, by reference.
fn content_objects(doc: &Document<'_>) -> BTreeMap<ObjRef, Object> {
    let mut objects = BTreeMap::new();
    for (num, entry) in doc.xref().entries() {
        let r = match entry {
            XrefEntry::InUse { gen, .. } => ObjRef { num, gen },
            XrefEntry::InStream { .. } => ObjRef { num, gen: 0 },
            XrefEntry::Free { .. } => continue,
        };
        if let Ok(Some(obj)) = doc.get(r) {
            if !describes_file_layout(&obj) {
                objects.insert(r, obj);
            }
        }
    }
    objects
}

/// Same object model. For streams, `/Length` is left out of the
/// comparison: the writer replaces a wrong or indirect one by the exact
/// direct value.
fn same_object(a: &Object, b: &Object) -> bool {
    match (a, b) {
        (Object::Stream { dict: da, data: xa }, Object::Stream { dict: db, data: xb }) => {
            let without_length = |d: &fyp_core::object::Dict| {
                let mut d = d.clone();
                d.remove(&Name::new("Length"));
                d
            };
            xa == xb && without_length(da) == without_length(db)
        }
        _ => a == b,
    }
}

/// Open `bytes`, write it in `style`, open the result, compare, and check
/// that writing the result again gives the same bytes.
fn round_trip(name: &str, bytes: &[u8], style: XrefStyle) {
    let doc = Document::open(bytes).unwrap_or_else(|e| panic!("{name}: open: {e}"));
    let writer = Writer::new(doc.version()).xref_style(style);
    let out = writer
        .write(&doc)
        .unwrap_or_else(|e| panic!("{name}: write ({style:?}): {e}"));

    let again = Document::open(&out).unwrap_or_else(|e| panic!("{name}: reopen ({style:?}): {e}"));
    assert_eq!(
        again.reconstructed(),
        None,
        "{name} ({style:?}): the written file needed repair"
    );
    let expected_kind = match style {
        XrefStyle::Table => SectionKind::Table,
        XrefStyle::Stream => SectionKind::Stream,
    };
    assert_eq!(again.xref().kind(), expected_kind, "{name} ({style:?})");
    assert_eq!(again.version(), writer.version(), "{name} ({style:?})");
    assert_eq!(
        again.page_count().ok(),
        doc.page_count().ok(),
        "{name} ({style:?}): page count"
    );
    assert!(
        !again
            .xref()
            .entries()
            .any(|(_, e)| matches!(e, XrefEntry::InStream { .. })),
        "{name} ({style:?}): compressed objects remain"
    );
    assert!(
        !again.trailer().contains_key(&Name::new("Prev")),
        "{name} ({style:?}): /Prev in a single-section file"
    );

    let before = content_objects(&doc);
    let after = content_objects(&again);
    // The cross-reference stream the writer adds is not content: it is
    // filtered out on both sides.
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>(),
        "{name} ({style:?}): readable object numbers differ"
    );
    for (r, original) in &before {
        let rewritten = &after[r];
        assert!(
            same_object(original, rewritten),
            "{name} ({style:?}): object {} {} differs:\n{original:?}\n{rewritten:?}",
            r.num,
            r.gen
        );
    }

    let second = writer
        .write(&again)
        .unwrap_or_else(|e| panic!("{name}: second write ({style:?}): {e}"));
    assert_eq!(second, out, "{name} ({style:?}): rewriting is not stable");
}

#[test]
fn fixtures_round_trip() {
    let files = pdf_files(&tests_dir().join("fixtures"));
    assert!(!files.is_empty(), "no fixture found under tests/fixtures");
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(&path).expect("read fixture");
        for style in [XrefStyle::Table, XrefStyle::Stream] {
            round_trip(&name, &bytes, style);
        }
    }
}

/// Fixtures whose table is broken open by scan; once written they open
/// without repair and hold the same three objects as `minimal.pdf`.
#[test]
fn repaired_fixtures_write_sound_files() {
    let dir = tests_dir().join("fixtures");
    let minimal = std::fs::read(dir.join("minimal.pdf")).expect("minimal.pdf");
    let reference = content_objects(&Document::open(&minimal).expect("open minimal"));
    for name in [
        "bad-offsets.pdf",
        "no-startxref.pdf",
        "garbage-xref.pdf",
        "prev-loop.pdf",
    ] {
        let bytes = std::fs::read(dir.join(name)).expect("read fixture");
        let doc = Document::open(&bytes).expect("open");
        assert!(doc.reconstructed().is_some(), "{name}");
        let out = Writer::new(doc.version()).write(&doc).expect("write");
        let again = Document::open(&out).expect("reopen");
        assert_eq!(again.reconstructed(), None, "{name}");
        let objects = content_objects(&again);
        assert_eq!(objects.len(), reference.len(), "{name}");
        for (r, obj) in &reference {
            assert!(same_object(obj, &objects[r]), "{name}: object {}", r.num);
        }
    }
}

/// Every PDF under `tests/corpus-private/` (local, ignored by Git) goes
/// through the round trip. Absent directory: nothing to check. Files the
/// writer does not support yet (encryption) are reported and skipped.
#[test]
fn private_corpus_round_trip() {
    let dir = tests_dir().join("corpus-private");
    if !dir.is_dir() {
        eprintln!("no tests/corpus-private directory: skipped");
        return;
    }
    let mut checked = 0;
    let mut skipped = Vec::new();
    for path in pdf_files(&dir) {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(&path).expect("read corpus file");
        let doc = Document::open(&bytes).unwrap_or_else(|e| panic!("{name}: open: {e}"));
        match Writer::new(doc.version()).write(&doc) {
            Err(Error::Unsupported { feature }) => {
                skipped.push(format!("{name}: {feature}"));
                continue;
            }
            Err(e) => panic!("{name}: write: {e}"),
            Ok(_) => {}
        }
        for style in [XrefStyle::Table, XrefStyle::Stream] {
            round_trip(&name, &bytes, style);
        }
        checked += 1;
    }
    for line in &skipped {
        eprintln!("skipped {line}");
    }
    eprintln!(
        "{checked} corpus file(s) round-tripped, {} skipped",
        skipped.len()
    );
}
