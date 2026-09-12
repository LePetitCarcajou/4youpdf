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

/// Open a fixture that the reader must accept as is: no reconstruction,
/// the page of `minimal.pdf` readable.
fn open_sound(name: &str) -> Document<'static> {
    let bytes: &'static [u8] = Box::leak(fixture(name).into_boxed_slice());
    let doc = Document::open(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(doc.reconstructed(), None, "{name} was reconstructed");
    assert_eq!(doc.page_count(), Ok(1), "{name}");
    assert_eq!(media_box(&doc, PAGE), ints(&[0, 0, 595, 842]), "{name}");
    doc
}

/// Tolerance 1 (corpus: 113 files): `0000000000 65536 f` on the head of
/// the free list must not condemn the table.
#[test]
fn free_list_head_with_generation_65536_is_accepted() {
    let doc = open_sound("gen-65536.pdf");
    assert_eq!(doc.xref().kind(), fyp_core::xref::SectionKind::Table);
    assert_eq!(doc.xref().object_count(), 3);
    assert_eq!(doc.xref().get(0), Some(XrefEntry::Free { gen: 65535 }));
}

/// Tolerance 2 (corpus: 22 files): a cross-reference stream row of type 1
/// at offset 0 denotes a free number, not an object at the header.
#[test]
fn in_use_row_at_offset_zero_is_taken_as_free() {
    let doc = open_sound("inuse-offset-zero.pdf");
    assert_eq!(doc.xref().kind(), fyp_core::xref::SectionKind::Stream);
    assert_eq!(doc.xref().get(5), Some(XrefEntry::Free { gen: 0 }));
    assert_eq!(doc.get(ObjRef { num: 5, gen: 0 }), Ok(None));
    // The stream lists itself as object 4: three content objects plus it,
    // and two free entries (0 and 5).
    assert_eq!(doc.xref().object_count(), 4);
    assert_eq!(doc.xref().entries().count(), 6);
}

/// Tolerance 3a (corpus: pdf.js `issue9105_other.pdf`): `%PDF-1.` with no
/// minor digit is version 1.0, not a refusal.
#[test]
fn header_without_minor_digit_is_accepted() {
    let doc = open_sound("no-minor-version.pdf");
    assert_eq!(doc.version().to_string(), "1.0");
    assert_eq!(doc.xref().object_count(), 3);
    let bytes = fixture("no-minor-version.pdf");
    assert!(
        fyp_core::version::quick_info(&bytes)
            .expect("info")
            .header_present
    );
}

/// Tolerance 3b (corpus: pdf.js `bug1606566.pdf`): no `%PDF` header at
/// all, only the binary comment line; the file is tried with an assumed
/// version, and reported as headerless.
#[test]
fn headerless_file_with_a_comment_line_is_accepted() {
    let doc = open_sound("no-header.pdf");
    assert_eq!(doc.version(), fyp_core::version::ASSUMED_VERSION);
    assert_eq!(doc.xref().object_count(), 3);
    let bytes = fixture("no-header.pdf");
    let info = fyp_core::version::quick_info(&bytes).expect("info");
    assert!(!info.header_present);
    assert_eq!(info.startxref, Some(209));
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

// ---------------------------------------------------------------------------
// Tolerances found in the corpus survey (see `tests/fixtures/README.md`)
// ---------------------------------------------------------------------------

/// Replace the value after the last `startxref` by `value`, at byte level.
fn with_startxref(bytes: &[u8], value: &str) -> Vec<u8> {
    let marker = b"startxref\n";
    let at = bytes
        .windows(marker.len())
        .rposition(|w| w == marker)
        .expect("startxref")
        + marker.len();
    let digits = bytes[at..]
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    let mut out = bytes[..at].to_vec();
    out.extend_from_slice(value.as_bytes());
    out.extend_from_slice(&bytes[at + digits..]);
    out
}

#[test]
fn headerless_file_with_a_junk_first_line_is_accepted() {
    let bytes = fixture("no-header-junk.pdf");
    let info = fyp_core::version::quick_info(&bytes).expect("info");
    assert!(!info.header_present);
    let doc = Document::open(&bytes).expect("open");
    assert_eq!(doc.reconstructed(), None);
    assert_eq!(doc.page_count(), Ok(1));
}

#[test]
fn dangling_root_sends_the_file_to_the_scan() {
    let bytes = fixture("root-dangling.pdf");
    let doc = Document::open(&bytes).expect("open");
    assert!(
        matches!(doc.reconstructed(), Some(Error::BadXref { message, .. }) if message.contains("/Root 9 0")),
        "{:?}",
        doc.reconstructed()
    );
    // The scan replaced the dangling reference by the catalog it found.
    assert_eq!(
        doc.trailer().get(&Name::new("Root")),
        Some(&Object::Reference(ObjRef { num: 1, gen: 0 }))
    );
    assert_eq!(doc.page_count(), Ok(1));
}

#[test]
fn object_zero_is_never_in_use() {
    let bytes = fixture("object-zero.pdf");
    let doc = Document::open(&bytes).expect("open");
    assert_eq!(doc.reconstructed(), None);
    assert!(matches!(doc.xref().get(0), Some(XrefEntry::Free { .. })));
    assert_eq!(doc.get(ObjRef { num: 0, gen: 0 }), Ok(None));
    assert_eq!(doc.xref().object_count(), 3);
    assert_eq!(doc.page_count(), Ok(1));
    // Same when the table is rebuilt: the scan skips `0 0 obj`.
    let broken = with_startxref(&bytes, "999999");
    let doc = Document::open(&broken).expect("open by scan");
    assert!(doc.reconstructed().is_some());
    assert_eq!(doc.get(ObjRef { num: 0, gen: 0 }), Ok(None));
    assert_eq!(doc.xref().object_count(), 3);
    assert_eq!(doc.page_count(), Ok(1));
}

#[test]
fn wrong_startxref_is_relocated_to_the_nearby_table() {
    let bytes = fixture("startxref-off.pdf");
    let doc = Document::open(&bytes).expect("open");
    assert_eq!(doc.reconstructed(), None);
    assert_eq!(doc.relocated_startxref(), Some(209));
    assert_eq!(doc.page_count(), Ok(1));
    // Beyond the end of the file, nothing nearby: scanned, not relocated.
    let far = with_startxref(&fixture("minimal.pdf"), "999999");
    let doc = Document::open(&far).expect("open by scan");
    assert!(doc.reconstructed().is_some());
    assert_eq!(doc.relocated_startxref(), None);
    // Right on target: nothing to relocate.
    let minimal = fixture("minimal.pdf");
    let doc = Document::open(&minimal).expect("open");
    assert_eq!(doc.relocated_startxref(), None);
    // A cross-reference stream is found the same way.
    let off = with_startxref(&fixture("xrefstream.pdf"), "300");
    let doc = Document::open(&off).expect("open");
    assert_eq!(doc.reconstructed(), None);
    assert!(doc.relocated_startxref().is_some());
    assert_eq!(doc.page_count(), Ok(1));
}

#[test]
fn direct_root_dictionary_is_accepted_and_written_as_an_object() {
    let bytes = fixture("root-direct.pdf");
    let doc = Document::open(&bytes).expect("open");
    assert_eq!(doc.reconstructed(), None);
    assert!(matches!(
        doc.trailer().get(&Name::new("Root")),
        Some(Object::Dict(_))
    ));
    assert_eq!(doc.page_count(), Ok(1));
    let out = fyp_core::writer::Writer::new(doc.version())
        .write(&doc)
        .expect("write");
    let again = Document::open(&out).expect("reopen");
    assert_eq!(again.reconstructed(), None);
    assert_eq!(
        again.trailer().get(&Name::new("Root")),
        Some(&Object::Reference(ObjRef { num: 4, gen: 0 }))
    );
    assert_eq!(again.page_count(), Ok(1));
    // Also when the file is scanned: the direct catalog is kept.
    let broken = with_startxref(&bytes, "999999");
    let doc = Document::open(&broken).expect("open by scan");
    assert!(doc.reconstructed().is_some());
    assert_eq!(doc.page_count(), Ok(1));
}
