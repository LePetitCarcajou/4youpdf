//! Helpers shared by the integration tests: locating PDF files and
//! comparing two documents as object models.

#![allow(dead_code, clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fyp_core::document::Document;
use fyp_core::object::{Dict, Name, ObjRef, Object};
use fyp_core::xref::XrefEntry;

/// `tests/` at the root of the workspace.
pub fn tests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests")
}

/// `*.pdf` files directly in `dir`, sorted. Empty when `dir` is missing.
pub fn pdf_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| is_pdf(p))
        .collect();
    v.sort();
    v
}

/// `*.pdf` files anywhere under `dir`, sorted. Empty when `dir` is missing.
pub fn pdf_files_recursive(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in rd.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if is_pdf(&path) {
                out.push(path);
            }
        }
    }
    let mut v = Vec::new();
    walk(dir, &mut v);
    v.sort();
    v
}

fn is_pdf(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

/// Is `obj` an object stream or a cross-reference stream? Those describe
/// the layout of one particular file, not the document, and the writer
/// replaces them.
pub fn describes_file_layout(obj: &Object) -> bool {
    match obj {
        Object::Stream { dict, .. } => matches!(
            dict.get(&Name::new("Type")).and_then(Object::as_name),
            Some(n) if n.0 == b"ObjStm" || n.0 == b"XRef"
        ),
        _ => false,
    }
}

/// Every object the document can read, except those describing the file
/// layout and the `/Encrypt` dictionary of an encrypted source (the
/// writer drops it: the output is in the clear), by reference.
pub fn content_objects(doc: &Document<'_>) -> BTreeMap<ObjRef, Object> {
    let mut objects = BTreeMap::new();
    let encrypt_ref = match doc.trailer().get(&Name::new("Encrypt")) {
        Some(Object::Reference(r)) => Some(*r),
        _ => None,
    };
    for (num, entry) in doc.xref().entries() {
        let r = match entry {
            XrefEntry::InUse { gen, .. } => ObjRef { num, gen },
            XrefEntry::InStream { .. } => ObjRef { num, gen: 0 },
            XrefEntry::Free { .. } => continue,
        };
        if Some(r) == encrypt_ref {
            continue;
        }
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
pub fn same_object(a: &Object, b: &Object) -> bool {
    match (a, b) {
        (Object::Stream { dict: da, data: xa }, Object::Stream { dict: db, data: xb }) => {
            let without_length = |d: &Dict| {
                let mut d = d.clone();
                d.remove(&Name::new("Length"));
                d
            };
            xa == xb && without_length(da) == without_length(db)
        }
        _ => a == b,
    }
}

/// Compare a document with its rewritten copy: same page count, same
/// readable object numbers, same objects. `Err` describes the first
/// difference found.
pub fn compare(original: &Document<'_>, rewritten: &Document<'_>) -> Result<(), String> {
    let pages = (original.page_count().ok(), rewritten.page_count().ok());
    if pages.0 != pages.1 {
        return Err(format!("page count {:?} became {:?}", pages.0, pages.1));
    }
    let before = content_objects(original);
    let after = content_objects(rewritten);
    let missing: Vec<String> = before
        .keys()
        .filter(|r| !after.contains_key(r))
        .take(5)
        .map(|r| format!("{} {}", r.num, r.gen))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "{} object(s) lost, first: {}",
            before.keys().filter(|r| !after.contains_key(r)).count(),
            missing.join(", ")
        ));
    }
    // A catalog written directly in the source trailer comes out as a new
    // object: expected, not an appearance.
    let promoted_root = match (
        original.trailer().get(&Name::new("Root")),
        rewritten.trailer().get(&Name::new("Root")),
    ) {
        (Some(Object::Dict(_)), Some(Object::Reference(r))) => Some(*r),
        _ => None,
    };
    let is_extra = |r: &&ObjRef| !before.contains_key(r) && Some(**r) != promoted_root;
    let extra: Vec<String> = after
        .keys()
        .filter(is_extra)
        .take(5)
        .map(|r| format!("{} {}", r.num, r.gen))
        .collect();
    if !extra.is_empty() {
        return Err(format!(
            "{} object(s) appeared, first: {}",
            after.keys().filter(is_extra).count(),
            extra.join(", ")
        ));
    }
    for (r, a) in &before {
        let b = &after[r];
        if !same_object(a, b) {
            return Err(format!(
                "object {} {} differs: {} became {}",
                r.num,
                r.gen,
                describe(a),
                describe(b)
            ));
        }
    }
    Ok(())
}

/// Short description of an object for a report line.
fn describe(obj: &Object) -> String {
    let text = match obj {
        Object::Stream { dict, data } => format!("stream {dict:?} + {} bytes", data.len()),
        other => format!("{other:?}"),
    };
    if text.len() > 160 {
        format!("{}…", text.chars().take(160).collect::<String>())
    } else {
        text
    }
}
