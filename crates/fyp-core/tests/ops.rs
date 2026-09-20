//! Page operations (`fyp_core::ops`) on the fixtures and on a few corpus
//! files: every result opens without repair, holds the expected pages,
//! keeps each page equal to its model in the source, and points at no
//! missing object or dropped page.

#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::{Path, PathBuf};

use common::{dangling_references, dead_destinations, deep_equal, tests_dir};
use fyp_core::document::Document;
use fyp_core::object::{Dict, Name, ObjRef, Object};
use fyp_core::ops::{self, Page};
use fyp_core::Error;

const DEPTH: usize = 64;

fn fixture(name: &str) -> Vec<u8> {
    let path = tests_dir().join("fixtures").join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Corpus files worth exercising, when the corpus is fetched: many
/// pages, outlines, named destinations, forms, object streams, a deep and
/// a looping page tree.
fn corpus_files() -> Vec<(String, Vec<u8>)> {
    let root = tests_dir().join("corpus");
    [
        "qpdf/outlines-with-actions.pdf",
        "qpdf/outlines-with-old-root-dests.pdf",
        "qpdf/page-labels-and-outlines.pdf",
        "qpdf/form-fields-and-annotations.pdf",
        "qpdf/pages-loop.pdf",
        "qpdf/direct-outlines.pdf",
        "qpdf/duplicate-page-inherited.pdf",
        "pdfjs/issue15367.pdf",
        "pdfjs/tracemonkey.pdf",
        "pdfjs/160F-2019.pdf",
    ]
    .iter()
    .filter_map(|rel| {
        let path: PathBuf = root.join(rel);
        std::fs::read(&path)
            .ok()
            .map(|bytes| (rel.to_string(), bytes))
    })
    .collect()
}

/// Every source worth testing: the fixtures with a page tree, plus the
/// corpus files present.
fn sources() -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = common::pdf_files(&tests_dir().join("fixtures"))
        .into_iter()
        .filter_map(|path| {
            let bytes = std::fs::read(&path).ok()?;
            let doc = Document::open(&bytes).ok()?;
            ops::pages(&doc).ok()?;
            Some((path.file_name()?.to_string_lossy().into_owned(), bytes))
        })
        .collect();
    out.extend(corpus_files());
    out
}

/// Open a result and check what every operation guarantees.
fn check_output<'o>(name: &str, out: &'o [u8], expected_pages: usize) -> Document<'o> {
    let doc = Document::open(out).unwrap_or_else(|e| panic!("{name}: reopen: {e}"));
    assert_eq!(doc.reconstructed(), None, "{name}: output needed repair");
    assert_eq!(doc.encryption(), None, "{name}: output is encrypted");
    assert_eq!(doc.page_count(), Ok(expected_pages), "{name}: /Count");
    let pages = ops::pages(&doc).unwrap_or_else(|e| panic!("{name}: pages: {e}"));
    assert_eq!(pages.len(), expected_pages, "{name}: pages in the tree");
    let dangling = dangling_references(&doc);
    assert!(
        dangling.is_empty(),
        "{name}: dangling references {dangling:?}"
    );
    let dead = dead_destinations(&doc);
    assert!(dead.is_empty(), "{name}: destinations to no page {dead:?}");
    doc
}

/// A kept page equals its source model: same dictionary, references
/// followed on both sides, `/Parent` aside.
fn same_page(name: &str, src_doc: &Document<'_>, src: &Page, out_doc: &Document<'_>, out: &Page) {
    let strip = |p: &Page| {
        let mut d = p.dict.clone();
        d.remove(&Name::new("Parent"));
        d.remove(&Name::new("Type"));
        d
    };
    let (a, b) = (strip(src), strip(out));
    for (key, va) in &a {
        let vb = b
            .get(key)
            .unwrap_or_else(|| panic!("{name}: page lost /{}", key.as_str_lossy()));
        assert!(
            deep_equal(src_doc, va, out_doc, vb, DEPTH),
            "{name}: /{} differs: {va:?} vs {vb:?}, resolved {:?} vs {:?}",
            key.as_str_lossy(),
            src_doc.resolve(va),
            out_doc.resolve(vb)
        );
    }
    for key in b.keys() {
        assert!(
            a.contains_key(key),
            "{name}: page gained /{}",
            key.as_str_lossy()
        );
    }
}

#[test]
fn extracting_every_page_in_order_gives_an_equivalent_document() {
    for (name, bytes) in sources() {
        let doc = Document::open(&bytes).expect("open");
        let pages = ops::pages(&doc).expect("pages");
        let all: Vec<usize> = (0..pages.len()).collect();
        let out = ops::extract_pages(&doc, &all).unwrap_or_else(|e| panic!("{name}: {e}"));
        let again = check_output(&name, &out, pages.len());
        let out_pages = ops::pages(&again).unwrap();
        for (src, dst) in pages.iter().zip(&out_pages) {
            same_page(&name, &doc, src, &again, dst);
        }
        // Extracting again from the result gives the same bytes: the
        // operation is a fixed point once the structure is normalised.
        let twice = ops::extract_pages(&again, &all).unwrap();
        assert_eq!(twice, out, "{name}: not stable");
    }
}

#[test]
fn extract_reorders_and_drops_pages() {
    for (name, bytes) in sources() {
        let doc = Document::open(&bytes).expect("open");
        let pages = ops::pages(&doc).expect("pages");
        if pages.len() < 3 {
            continue;
        }
        let pick = [pages.len() - 1, 0, 1];
        let out = ops::extract_pages(&doc, &pick).unwrap_or_else(|e| panic!("{name}: {e}"));
        let again = check_output(&name, &out, 3);
        let out_pages = ops::pages(&again).unwrap();
        for (i, &p) in pick.iter().enumerate() {
            same_page(&name, &doc, &pages[p], &again, &out_pages[i]);
        }
    }
}

#[test]
fn delete_rotate_and_split() {
    for (name, bytes) in sources() {
        let doc = Document::open(&bytes).expect("open");
        let pages = ops::pages(&doc).expect("pages");
        let n = pages.len();
        // Delete the first page (when another remains).
        if n > 1 {
            let out = ops::delete_pages(&doc, &[0]).unwrap_or_else(|e| panic!("{name}: {e}"));
            let again = check_output(&name, &out, n - 1);
            let out_pages = ops::pages(&again).unwrap();
            same_page(&name, &doc, &pages[1], &again, &out_pages[0]);
        }
        // Rotate the last page by 90: relative to its current value.
        let last = n - 1;
        let before = match pages[last].dict.get(&Name::new("Rotate")) {
            Some(Object::Integer(i)) => *i,
            _ => 0,
        };
        let out = ops::rotate(&doc, &[last], 90).unwrap_or_else(|e| panic!("{name}: {e}"));
        let again = check_output(&name, &out, n);
        let out_pages = ops::pages(&again).unwrap();
        let after = match out_pages[last].dict.get(&Name::new("Rotate")) {
            Some(Object::Integer(i)) => *i,
            None => 0,
            other => panic!("{name}: /Rotate became {other:?}"),
        };
        assert_eq!(after, (before + 90).rem_euclid(360), "{name}");
        // Split in parts of 4 pages.
        let ranges = ops::ranges_every(n, 4);
        let parts = ops::split(&doc, &ranges).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(parts.len(), ranges.len(), "{name}");
        for (part, range) in parts.iter().zip(&ranges) {
            let again = check_output(&name, part, range.len());
            let out_pages = ops::pages(&again).unwrap();
            for (i, p) in range.clone().enumerate() {
                same_page(&name, &doc, &pages[p], &again, &out_pages[i]);
            }
        }
    }
}

#[test]
fn merge_three_files_of_different_structure() {
    // Classic table, cross-reference stream, object streams.
    let files = [
        fixture("minimal.pdf"),
        fixture("xrefstream.pdf"),
        fixture("objstm.pdf"),
    ];
    let docs: Vec<Document<'_>> = files
        .iter()
        .map(|b| Document::open(b).expect("open"))
        .collect();
    let out = ops::merge(&docs).expect("merge");
    let again = check_output("merge", &out, 3);
    let out_pages = ops::pages(&again).unwrap();
    for (i, doc) in docs.iter().enumerate() {
        let src = ops::pages(doc).unwrap();
        same_page("merge", doc, &src[0], &again, &out_pages[i]);
    }
    // No object of the sources is shared: the three pages have distinct
    // numbers, and the version is the highest of the inputs (1.7).
    let numbers: std::collections::BTreeSet<u32> = out_pages
        .iter()
        .filter_map(|p| p.reference.map(|r| r.num))
        .collect();
    assert_eq!(numbers.len(), 3);
    assert_eq!(again.version(), docs[0].version().max(docs[1].version()));
}

#[test]
fn merge_corpus_files_chains_outlines_and_keeps_destinations() {
    let files = corpus_files();
    if files.len() < 2 {
        eprintln!("corpus not fetched: skipped");
        return;
    }
    let docs: Vec<Document<'_>> = files
        .iter()
        .map(|(_, b)| Document::open(b).expect("open"))
        .collect();
    let total: usize = docs.iter().map(|d| ops::pages(d).unwrap().len()).sum();
    let out = ops::merge(&docs).expect("merge");
    let again = check_output("merge corpus", &out, total);
    let out_pages = ops::pages(&again).unwrap();
    let mut position = 0;
    for doc in &docs {
        for src in ops::pages(doc).unwrap() {
            same_page("merge corpus", doc, &src, &again, &out_pages[position]);
            position += 1;
        }
    }
    // Outlines: one root whose top-level chain is walkable from /First to
    // /Last, every item pointing back at the root.
    let catalog = again.catalog().unwrap();
    let outlines = match catalog.get(&Name::new("Outlines")) {
        Some(o) => again.resolve(o).unwrap(),
        None => panic!("merged file lost its outlines"),
    };
    let root = outlines.as_dict().unwrap().clone();
    let root_ref = catalog.get(&Name::new("Outlines")).cloned().unwrap();
    let mut item = root.get(&Name::new("First")).cloned().unwrap();
    let mut seen = 0;
    let mut last = None;
    while let Object::Reference(r) = item {
        let dict = again.get(r).unwrap().unwrap().as_dict().unwrap().clone();
        assert_eq!(dict.get(&Name::new("Parent")), Some(&root_ref));
        seen += 1;
        last = Some(item.clone());
        item = dict
            .get(&Name::new("Next"))
            .cloned()
            .unwrap_or(Object::Null);
        assert!(seen < 10_000, "outline chain loops");
    }
    assert!(
        seen > 1,
        "only {seen} top-level outline item(s) after merging"
    );
    assert_eq!(last, root.get(&Name::new("Last")).cloned());
    // Named destinations: one leaf node, sorted names.
    let names = again
        .resolve(catalog.get(&Name::new("Names")).expect("/Names"))
        .unwrap();
    let dests = names
        .as_dict()
        .unwrap()
        .get(&Name::new("Dests"))
        .cloned()
        .unwrap();
    let leaf = again.resolve(&dests).unwrap();
    let Some(Object::Array(pairs)) = leaf.as_dict().unwrap().get(&Name::new("Names")) else {
        panic!("merged destinations are not a leaf node");
    };
    let keys: Vec<&Vec<u8>> = pairs
        .iter()
        .step_by(2)
        .map(|k| match k {
            Object::String(s) => s,
            other => panic!("{other:?}"),
        })
        .collect();
    assert!(keys.len() > 1);
    assert!(keys.windows(2).all(|w| w[0] < w[1]), "names not sorted");
}

#[test]
fn encrypted_input_gives_a_clear_output() {
    for name in ["encrypted-rc4.pdf", "encrypted-aes256.pdf"] {
        let bytes = fixture(name);
        let doc = Document::open(&bytes).expect("open");
        assert!(doc.encryption().is_some());
        let out = ops::extract_pages(&doc, &[0]).expect("extract");
        let again = check_output(name, &out, 1);
        let src = ops::pages(&doc).unwrap();
        let dst = ops::pages(&again).unwrap();
        same_page(name, &doc, &src[0], &again, &dst[0]);
        // The content stream is readable in the clear.
        let contents = dst[0].dict.get(&Name::new("Contents")).unwrap();
        let stream = again.resolve(contents).unwrap();
        assert_eq!(
            again.decoded(&stream).unwrap(),
            b"BT /F1 24 Tf 72 720 Td (Hello) Tj ET"
        );
    }
}

#[test]
fn selection_errors_are_clear() {
    let bytes = fixture("minimal.pdf");
    let doc = Document::open(&bytes).expect("open");
    assert_eq!(
        ops::extract_pages(&doc, &[1]).map(|_| ()),
        Err(Error::NoSuchPage { index: 1, count: 1 })
    );
    assert!(matches!(
        ops::extract_pages(&doc, &[0, 0]).map(|_| ()),
        Err(Error::BadOperation { message }) if message.contains("more than once")
    ));
    assert!(matches!(
        ops::extract_pages(&doc, &[]).map(|_| ()),
        Err(Error::BadOperation { .. })
    ));
    assert!(matches!(
        ops::delete_pages(&doc, &[0]).map(|_| ()),
        Err(Error::BadOperation { .. })
    ));
    assert!(matches!(
        ops::rotate(&doc, &[0], 45).map(|_| ()),
        Err(Error::BadOperation { message }) if message.contains("multiple of 90")
    ));
    assert!(matches!(
        ops::split(&doc, std::slice::from_ref(&(0..0))).map(|_| ()),
        Err(Error::BadOperation { .. })
    ));
    assert_eq!(
        ops::split(&doc, std::slice::from_ref(&(0..2))).map(|_| ()),
        Err(Error::NoSuchPage { index: 1, count: 1 })
    );
    assert!(matches!(
        ops::merge(&[]).map(|_| ()),
        Err(Error::BadOperation { .. })
    ));
    // Rotation wraps: 90 three times more is back to 0, -90 is 270.
    let turned = ops::rotate(&doc, &[0], 90).unwrap();
    let doc2 = Document::open(&turned).unwrap();
    assert_eq!(
        ops::pages(&doc2).unwrap()[0].dict.get(&Name::new("Rotate")),
        Some(&Object::Integer(90))
    );
    let back = ops::rotate(&doc2, &[0], 270).unwrap();
    let doc3 = Document::open(&back).unwrap();
    assert_eq!(
        ops::pages(&doc3).unwrap()[0].dict.get(&Name::new("Rotate")),
        None
    );
    let minus = ops::rotate(&doc3, &[0], -90).unwrap();
    let doc4 = Document::open(&minus).unwrap();
    assert_eq!(
        ops::pages(&doc4).unwrap()[0].dict.get(&Name::new("Rotate")),
        Some(&Object::Integer(270))
    );
}

// ---------------------------------------------------------------------------
// Hand-built documents for the cases that break naive implementations
// ---------------------------------------------------------------------------

/// One-section PDF where `objects[i]` is object `i + 1`.
fn build(objects: &[String]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let startxref = out.len();
    let size = offsets.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{startxref}\n%%EOF\n")
            .as_bytes(),
    );
    out
}

fn s(text: &str) -> String {
    text.to_string()
}

#[test]
fn inherited_attributes_are_resolved_into_each_page() {
    // Root: MediaBox and Rotate; middle node: Resources and its own
    // MediaBox; page 5 overrides Rotate; page 6 inherits everything.
    let file = build(&[
        s("<< /Type /Catalog /Pages 2 0 R >>"),
        s("<< /Type /Pages /Kids [3 0 R] /Count 2 /MediaBox [0 0 100 100] /Rotate 90 >>"),
        s("<< /Type /Pages /Parent 2 0 R /Kids [4 0 R] /Count 2 /Resources << /ProcSet [/PDF] >> /MediaBox [0 0 200 200] >>"),
        s("<< /Type /Pages /Parent 3 0 R /Kids [5 0 R 6 0 R] /Count 2 >>"),
        s("<< /Type /Page /Parent 4 0 R /Rotate 180 >>"),
        s("<< /Type /Page /Parent 4 0 R >>"),
    ]);
    let doc = Document::open(&file).expect("open");
    let pages = ops::pages(&doc).expect("pages");
    assert_eq!(pages.len(), 2);
    let media = Object::Array(vec![
        Object::Integer(0),
        Object::Integer(0),
        Object::Integer(200),
        Object::Integer(200),
    ]);
    assert_eq!(pages[0].dict.get(&Name::new("MediaBox")), Some(&media));
    assert_eq!(
        pages[0].dict.get(&Name::new("Rotate")),
        Some(&Object::Integer(180))
    );
    assert_eq!(
        pages[1].dict.get(&Name::new("Rotate")),
        Some(&Object::Integer(90))
    );
    assert!(pages[1].dict.contains_key(&Name::new("Resources")));
    assert_eq!(pages[1].reference, Some(ObjRef { num: 6, gen: 0 }));
    // Extract page 6 alone: the tree is flat and the page carries it all.
    let out = ops::extract_pages(&doc, &[1]).expect("extract");
    let again = check_output("inherited", &out, 1);
    let page = &ops::pages(&again).unwrap()[0];
    assert_eq!(page.dict.get(&Name::new("MediaBox")), Some(&media));
    assert_eq!(
        page.dict.get(&Name::new("Rotate")),
        Some(&Object::Integer(90))
    );
    let root = again.catalog().unwrap();
    let pages_root = again
        .resolve(root.get(&Name::new("Pages")).unwrap())
        .unwrap();
    let kids = pages_root
        .as_dict()
        .unwrap()
        .get(&Name::new("Kids"))
        .unwrap();
    assert!(matches!(kids, Object::Array(k) if k.len() == 1));
    // Rotating the page that inherits 90 by 270 lands on 0: no /Rotate.
    let out = ops::rotate(&doc, &[1], 270).expect("rotate");
    let again = Document::open(&out).unwrap();
    assert_eq!(
        ops::pages(&again).unwrap()[1]
            .dict
            .get(&Name::new("Rotate")),
        None
    );
}

/// `mixed12.pdf`: the twelve pages the thumbnail alignment of the
/// application is checked on (`tests/fixtures/README.md`). Every one of
/// them comes out of `ops::pages` with the size and the rotation the
/// interface draws it from.
#[test]
fn mixed12_pages_keep_their_sizes_and_rotations() {
    let bytes = fixture("mixed12.pdf");
    let doc = Document::open(&bytes).expect("open");
    assert_eq!(doc.reconstructed(), None, "mixed12.pdf needed repair");
    assert_eq!(doc.page_count(), Ok(12));
    let pages = ops::pages(&doc).expect("pages");
    // Width, height and `/Rotate` of every page in reading order: A4
    // portrait and landscape, one of each turned by 90, Letter, and a
    // 300 x 800 page taller than A4 for its width.
    let expected = [
        (595, 842, None),
        (842, 595, None),
        (595, 842, Some(90)),
        (612, 792, None),
        (842, 595, None),
        (595, 842, None),
        (595, 842, None),
        (842, 595, Some(90)),
        (300, 800, None),
        (595, 842, None),
        (842, 595, None),
        (595, 842, None),
    ];
    assert_eq!(pages.len(), expected.len());
    for (i, (page, &(width, height, rotate))) in pages.iter().zip(expected.iter()).enumerate() {
        let media = Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(width),
            Object::Integer(height),
        ]);
        assert_eq!(
            page.dict.get(&Name::new("MediaBox")),
            Some(&media),
            "page {}: /MediaBox",
            i + 1
        );
        assert_eq!(
            page.dict.get(&Name::new("Rotate")),
            rotate.map(Object::Integer).as_ref(),
            "page {}: /Rotate",
            i + 1
        );
    }
}

#[test]
fn links_to_dropped_pages_are_cleaned() {
    // Two pages. Page 3 carries a link to page 4 and a text annotation;
    // page 4 carries a link to page 3 and a GoTo action to page 3. The
    // catalog names a destination on each page and an outline item to
    // page 4.
    let file = build(&[
        s("<< /Type /Catalog /Pages 2 0 R /Outlines 9 0 R /Names << /Dests << /Names [(to3) [3 0 R /Fit] (to4) [4 0 R /XYZ 0 0 0]] >> >> /Dests << /old3 [3 0 R /Fit] /old4 [4 0 R /Fit] >> >>"),
        s("<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 10 10] >>"),
        s("<< /Type /Page /Parent 2 0 R /Annots [5 0 R 6 0 R] >>"),
        s("<< /Type /Page /Parent 2 0 R /Annots [7 0 R 8 0 R] >>"),
        s("<< /Type /Annot /Subtype /Link /Rect [0 0 1 1] /P 3 0 R /Dest [4 0 R /Fit] >>"),
        s("<< /Type /Annot /Subtype /Text /Rect [0 0 1 1] /P 3 0 R /Contents (note) >>"),
        s("<< /Type /Annot /Subtype /Link /Rect [0 0 1 1] /P 4 0 R /Dest [3 0 R /Fit] >>"),
        s("<< /Type /Annot /Subtype /Link /Rect [0 0 1 1] /P 4 0 R /A << /S /GoTo /D [3 0 R /Fit] >> >>"),
        s("<< /Type /Outlines /First 10 0 R /Last 10 0 R /Count 1 >>"),
        s("<< /Title (page four) /Parent 9 0 R /Dest [4 0 R /Fit] >>"),
    ]);
    let doc = Document::open(&file).expect("open");
    assert_eq!(dangling_references(&doc), Vec::<String>::new());

    // Keep page 3 only: its link to page 4 goes, the note stays; the
    // outline item loses its destination; named destinations to page 4
    // are forgotten.
    let out = ops::extract_pages(&doc, &[0]).expect("extract");
    let again = check_output("links", &out, 1);
    let page = &ops::pages(&again).unwrap()[0];
    let annots = match page.dict.get(&Name::new("Annots")) {
        Some(Object::Array(a)) => a.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(annots.len(), 1);
    let note = again.resolve(&annots[0]).unwrap();
    assert_eq!(
        note.as_dict().unwrap().get(&Name::new("Subtype")),
        Some(&Object::Name(Name::new("Text")))
    );
    let catalog = again.catalog().unwrap();
    let item = again
        .resolve(
            again
                .resolve(catalog.get(&Name::new("Outlines")).unwrap())
                .unwrap()
                .as_dict()
                .unwrap()
                .get(&Name::new("First"))
                .unwrap(),
        )
        .unwrap();
    assert!(item.as_dict().unwrap().get(&Name::new("Dest")).is_none());
    assert_eq!(
        item.as_dict().unwrap().get(&Name::new("Title")),
        Some(&Object::String(b"page four".to_vec()))
    );
    let names = again
        .resolve(catalog.get(&Name::new("Names")).unwrap())
        .unwrap();
    let dests = names.as_dict().unwrap().get(&Name::new("Dests")).unwrap();
    let leaf = dests.as_dict().unwrap().get(&Name::new("Names")).unwrap();
    assert!(
        matches!(leaf, Object::Array(a) if a.len() == 2 && a[0] == Object::String(b"to3".to_vec()))
    );
    let old: Dict = catalog
        .get(&Name::new("Dests"))
        .and_then(Object::as_dict)
        .cloned()
        .unwrap();
    assert_eq!(
        old.keys().map(|k| k.as_str_lossy()).collect::<Vec<_>>(),
        ["old3"]
    );

    // Keep page 4 only: both its links pointed at page 3, both go.
    let out = ops::extract_pages(&doc, &[1]).expect("extract");
    let again = check_output("links", &out, 1);
    let page = &ops::pages(&again).unwrap()[0];
    assert!(
        matches!(page.dict.get(&Name::new("Annots")), Some(Object::Array(a)) if a.is_empty()),
        "{:?}",
        page.dict.get(&Name::new("Annots"))
    );

    // Both pages, swapped: everything survives, every link still lands.
    let out = ops::extract_pages(&doc, &[1, 0]).expect("extract");
    let again = check_output("links", &out, 2);
    let pages = ops::pages(&again).unwrap();
    for page in &pages {
        let Some(Object::Array(annots)) = page.dict.get(&Name::new("Annots")) else {
            panic!("annotations lost");
        };
        assert_eq!(annots.len(), 2);
    }
}

#[test]
fn page_tree_loops_and_direct_kids_are_tolerated() {
    // Node 3 lists itself and its parent among its kids; page 4 is a
    // direct dictionary in /Kids.
    let file = build(&[
        s("<< /Type /Catalog /Pages 2 0 R >>"),
        s("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        s("<< /Type /Pages /Parent 2 0 R /Kids [3 0 R 2 0 R << /Type /Page /MediaBox [0 0 5 5] >> 4 0 R 4 0 R] /Count 1 >>"),
        s("<< /Type /Page /Parent 3 0 R /MediaBox [0 0 7 7] >>"),
    ]);
    let doc = Document::open(&file).expect("open");
    let pages = ops::pages(&doc).expect("pages");
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].reference, None);
    assert_eq!(pages[1].reference, Some(ObjRef { num: 4, gen: 0 }));
    let out = ops::extract_pages(&doc, &[0, 1]).expect("extract");
    let again = check_output("loop", &out, 2);
    let out_pages = ops::pages(&again).unwrap();
    same_page("loop", &doc, &pages[0], &again, &out_pages[0]);
    same_page("loop", &doc, &pages[1], &again, &out_pages[1]);
    // No page at all: an error, not a panic.
    let empty = build(&[
        s("<< /Type /Catalog /Pages 2 0 R >>"),
        s("<< /Type /Pages /Kids [] /Count 0 >>"),
    ]);
    let doc = Document::open(&empty).expect("open");
    assert!(matches!(ops::pages(&doc), Err(Error::BadStructure { .. })));
    assert!(matches!(
        ops::extract_pages(&doc, &[0]),
        Err(Error::BadStructure { .. })
    ));
}

/// qpdf's `deep-pages.pdf`: 108 nested `/Pages` nodes, the deepest one
/// listing itself, and no page reachable. An error, never a stack
/// overflow.
#[test]
fn hostile_page_tree_from_the_corpus_is_an_error() {
    let path = tests_dir().join("corpus/qpdf/deep-pages.pdf");
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("corpus not fetched: skipped");
        return;
    };
    let doc = Document::open(&bytes).expect("open");
    assert!(matches!(ops::pages(&doc), Err(Error::BadStructure { .. })));
    assert!(matches!(
        ops::extract_pages(&doc, &[0]),
        Err(Error::BadStructure { .. })
    ));
}

#[test]
fn split_files_land_in_order() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/corpus/qpdf/outlines-with-actions.pdf");
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("corpus not fetched: skipped");
        return;
    };
    let doc = Document::open(&bytes).expect("open");
    let n = ops::pages(&doc).unwrap().len();
    assert_eq!(n, 30);
    let parts = ops::split(&doc, &ops::ranges_every(n, 7)).expect("split");
    assert_eq!(parts.len(), 5);
    let counts: Vec<usize> = parts
        .iter()
        .map(|p| Document::open(p).unwrap().page_count().unwrap())
        .collect();
    assert_eq!(counts, [7, 7, 7, 7, 2]);
}
