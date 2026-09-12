//! Document layer on the hand-built fixtures of `tests/fixtures/`.

#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use fyp_core::document::Document;
use fyp_core::object::{Name, ObjRef, Object};
use fyp_core::xref::XrefEntry;
use fyp_core::Error;

const PAGE: ObjRef = ObjRef { num: 3, gen: 0 };

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn media_box(doc: &Document<'_>, page: ObjRef) -> Vec<Object> {
    let page = doc.get(page).expect("read page").expect("page listed");
    match page.as_dict().and_then(|d| d.get(&Name::new("MediaBox"))) {
        Some(Object::Array(items)) => items.clone(),
        other => panic!("expected a /MediaBox array, got {other:?}"),
    }
}

fn ints(values: &[i64]) -> Vec<Object> {
    values.iter().map(|&v| Object::Integer(v)).collect()
}

#[test]
fn minimal_objects_come_from_the_xref() {
    let bytes = fixture("minimal.pdf");
    let doc = Document::open(&bytes).expect("open");
    assert_eq!(doc.xref().object_count(), 3);
    for (num, entry) in doc.xref().entries() {
        if let XrefEntry::InUse { gen, .. } = entry {
            let obj = doc.get(ObjRef { num, gen }).expect("get").expect("listed");
            assert!(obj.as_dict().is_some(), "object {num} is not a dictionary");
        }
    }
    assert_eq!(
        doc.catalog()
            .expect("catalog")
            .get(&Name::new("Type"))
            .and_then(Object::as_name),
        Some(&Name::new("Catalog"))
    );
    assert_eq!(doc.page_count(), Ok(1));
    assert_eq!(media_box(&doc, PAGE), ints(&[0, 0, 595, 842]));
}

#[test]
fn incremental_update_replaces_object_3() {
    let bytes = fixture("incremental.pdf");
    let doc = Document::open(&bytes).expect("open");
    assert_eq!(
        doc.xref().get(3),
        Some(XrefEntry::InUse {
            offset: 352,
            gen: 0
        })
    );
    assert_eq!(media_box(&doc, PAGE), ints(&[0, 0, 612, 792]));
    // Objects 1 and 2 still come from the original section.
    assert_eq!(doc.xref().object_count(), 3);
    assert_eq!(doc.page_count(), Ok(1));
    assert_eq!(
        doc.trailer()
            .get(&Name::new("Prev"))
            .and_then(Object::as_i64),
        Some(209)
    );
}

/// Open `name` on another thread so that a hang fails the test instead of
/// blocking it; return the outcome as `(reconstruction reason, page count)`.
fn open_with_timeout(name: &str) -> Result<(Option<Error>, usize), Error> {
    let bytes = fixture(name);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let outcome = Document::open(&bytes).and_then(|doc| {
            all_listed_objects_are_readable(&doc);
            assert_eq!(media_box(&doc, PAGE), ints(&[0, 0, 595, 842]));
            assert_eq!(doc.xref().object_count(), 3);
            assert_eq!(
                doc.xref().kind(),
                fyp_core::xref::SectionKind::Reconstructed
            );
            Ok((doc.reconstructed().cloned(), doc.page_count()?))
        });
        let _ = tx.send(outcome);
    });
    rx.recv_timeout(Duration::from_secs(10))
        .unwrap_or_else(|_| panic!("Document::open did not return on {name}"))
}

#[test]
fn prev_loop_is_repaired_not_a_hang() {
    assert_eq!(
        open_with_timeout("prev-loop.pdf"),
        Ok((Some(Error::XrefLoop { offset: 209 }), 1))
    );
}

#[test]
fn sabotaged_offsets_are_repaired() {
    let (reason, pages) = open_with_timeout("bad-offsets.pdf").expect("open");
    assert!(
        matches!(reason, Some(Error::BadXref { offset: 64, .. })),
        "{reason:?}"
    );
    assert_eq!(pages, 1);
}

#[test]
fn missing_startxref_is_repaired() {
    assert_eq!(
        open_with_timeout("no-startxref.pdf"),
        Ok((Some(Error::MissingStartxref), 1))
    );
}

#[test]
fn garbage_in_place_of_the_table_is_repaired() {
    let (reason, pages) = open_with_timeout("garbage-xref.pdf").expect("open");
    assert!(
        matches!(reason, Some(Error::BadXref { offset: 209, .. })),
        "{reason:?}"
    );
    assert_eq!(pages, 1);
}

#[test]
fn truncated_file_opens_with_what_is_left() {
    let bytes = fixture("minimal.pdf");
    let cut = bytes
        .windows(4)
        .position(|w| w == b"/Med")
        .expect("object 3");
    let doc = Document::open(&bytes[..cut]).expect("open");
    assert_eq!(doc.reconstructed(), Some(&Error::MissingStartxref));
    assert_eq!(doc.xref().object_count(), 2);
    assert_eq!(doc.get(PAGE), Ok(None));
    // The page tree still says one page; the page itself is gone.
    assert_eq!(doc.page_count(), Ok(1));
}

/// Every entry of the table that denotes a stored object can be read, and
/// the object found is a dictionary or a stream.
fn all_listed_objects_are_readable(doc: &Document<'_>) {
    for (num, entry) in doc.xref().entries() {
        let r = match entry {
            XrefEntry::InUse { gen, .. } => ObjRef { num, gen },
            XrefEntry::InStream { .. } => ObjRef { num, gen: 0 },
            XrefEntry::Free { .. } => continue,
        };
        let obj = doc.get(r).expect("get").expect("listed");
        assert!(obj.as_dict().is_some(), "object {num} is not a dictionary");
    }
}

#[test]
fn xref_stream_fixture() {
    let bytes = fixture("xrefstream.pdf");
    let doc = Document::open(&bytes).expect("open");
    assert_eq!(doc.version().to_string(), "1.5");
    assert_eq!(
        doc.xref().get(1),
        Some(XrefEntry::InUse { offset: 15, gen: 0 })
    );
    assert_eq!(
        doc.xref().get(3),
        Some(XrefEntry::InUse {
            offset: 121,
            gen: 0
        })
    );
    // The stream lists itself, at the offset `startxref` points to.
    assert_eq!(
        doc.xref().get(4),
        Some(XrefEntry::InUse {
            offset: 209,
            gen: 0
        })
    );
    assert_eq!(doc.xref().object_count(), 4);
    assert_eq!(
        doc.trailer()
            .get(&Name::new("Type"))
            .and_then(Object::as_name),
        Some(&Name::new("XRef"))
    );
    all_listed_objects_are_readable(&doc);
    assert_eq!(doc.page_count(), Ok(1));
    assert_eq!(media_box(&doc, PAGE), ints(&[0, 0, 595, 842]));
}

#[test]
fn object_stream_fixture() {
    let bytes = fixture("objstm.pdf");
    let doc = Document::open(&bytes).expect("open");
    assert_eq!(
        doc.xref().get(1),
        Some(XrefEntry::InStream {
            stream_num: 4,
            index: 0
        })
    );
    assert_eq!(
        doc.xref().get(2),
        Some(XrefEntry::InStream {
            stream_num: 4,
            index: 1
        })
    );
    assert_eq!(
        doc.xref().get(3),
        Some(XrefEntry::InUse { offset: 15, gen: 0 })
    );
    assert_eq!(
        doc.xref().get(4),
        Some(XrefEntry::InUse {
            offset: 103,
            gen: 0
        })
    );
    assert_eq!(doc.xref().object_count(), 5);
    all_listed_objects_are_readable(&doc);
    assert_eq!(
        doc.catalog()
            .expect("catalog")
            .get(&Name::new("Type"))
            .and_then(Object::as_name),
        Some(&Name::new("Catalog"))
    );
    assert_eq!(doc.page_count(), Ok(1));
    assert_eq!(media_box(&doc, PAGE), ints(&[0, 0, 595, 842]));
    // The object stream's decoded data starts with its `objnum offset` pairs.
    let objstm = doc
        .get(ObjRef { num: 4, gen: 0 })
        .expect("get")
        .expect("listed");
    let decoded = doc.decoded(&objstm).expect("decode");
    assert!(decoded.starts_with(b"1 0 2 "), "{decoded:?}");
}

#[test]
fn hybrid_fixture_reads_objects_hidden_in_xrefstm() {
    let bytes = fixture("hybrid.pdf");
    let doc = Document::open(&bytes).expect("open");
    assert_eq!(
        doc.xref().get(1),
        Some(XrefEntry::InUse { offset: 15, gen: 0 })
    );
    assert_eq!(
        doc.xref().get(3),
        Some(XrefEntry::InStream {
            stream_num: 4,
            index: 0
        })
    );
    assert_eq!(
        doc.xref().get(4),
        Some(XrefEntry::InUse {
            offset: 121,
            gen: 0
        })
    );
    // Object 5, the /XRefStm stream, is not listed anywhere: null.
    assert_eq!(doc.get(ObjRef { num: 5, gen: 0 }), Ok(None));
    assert_eq!(
        doc.trailer()
            .get(&Name::new("XRefStm"))
            .and_then(Object::as_i64),
        Some(277)
    );
    assert_eq!(doc.xref().object_count(), 4);
    all_listed_objects_are_readable(&doc);
    assert_eq!(doc.page_count(), Ok(1));
    assert_eq!(media_box(&doc, PAGE), ints(&[0, 0, 595, 842]));
}

#[test]
fn newest_section_kind_of_each_fixture() {
    use fyp_core::xref::SectionKind;
    for (name, kind) in [
        ("minimal.pdf", SectionKind::Table),
        // Newest section is a table; its /Prev leads to another table.
        ("incremental.pdf", SectionKind::Table),
        ("xrefstream.pdf", SectionKind::Stream),
        ("objstm.pdf", SectionKind::Stream),
        ("hybrid.pdf", SectionKind::Hybrid),
    ] {
        let bytes = fixture(name);
        let doc = Document::open(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(doc.xref().kind(), kind, "{name}");
        assert_eq!(doc.reconstructed(), None, "{name}");
    }
}
