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

#[test]
fn prev_loop_is_an_error_not_a_hang() {
    let bytes = fixture("prev-loop.pdf");
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(Document::open(&bytes).map(|_| ()));
    });
    let result = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("Document::open did not return on a /Prev loop");
    assert_eq!(result, Err(Error::XrefLoop { offset: 209 }));
}
