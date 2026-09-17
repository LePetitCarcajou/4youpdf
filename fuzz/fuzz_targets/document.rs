//! The reading path of the core on arbitrary bytes, as the application and
//! `fyp` apply it to a file: `Document::open` (header, cross-reference
//! tables and streams, `/Prev` chain, object streams, reconstruction by
//! scan, `/Encrypt` and the key derivation of fyp-crypto), then every
//! object the table lists, every stream decoded through its filters, the
//! page tree, two page operations, and the writer in both styles, each
//! output opened again. Any byte sequence yields Ok or Err, never a panic.
//! Seeded from tests/fixtures and tests/corpus (fuzz/README.md).
#![no_main]
use libfuzzer_sys::fuzz_target;

use fyp_core::document::Document;
use fyp_core::filters::DecodeLimits;
use fyp_core::object::{ObjRef, Object};
use fyp_core::writer::{Writer, XrefStyle};
use fyp_core::xref::XrefEntry;
use fyp_core::{ops, Error};

/// Ceiling on decoded data, below the 256 MiB of the product: the check is
/// the same code, and a bomb within `-max_len` is caught sooner.
const LIMITS: DecodeLimits = DecodeLimits {
    max_output: 16 << 20,
};

/// The owner password of the encrypted fixtures (tests/fixtures/README.md),
/// tried when the empty user password opens nothing: the owner path of the
/// security handler.
const OWNER_PASSWORD: &[u8] = b"owner";

fuzz_target!(|data: &[u8]| {
    let doc = match Document::open_with(data, LIMITS, b"") {
        Ok(doc) => doc,
        Err(Error::WrongPassword) => match Document::open_with(data, LIMITS, OWNER_PASSWORD) {
            Ok(doc) => doc,
            Err(_) => return,
        },
        Err(_) => return,
    };
    read_everything(&doc);
    let _ = doc.page_count();
    if let Ok(pages) = ops::pages(&doc) {
        let all: Vec<usize> = (0..pages.len()).collect();
        let _ = ops::rotate(&doc, &all, 90);
        let _ = ops::merge(&[doc.clone(), doc.clone()]);
    }
    for style in [XrefStyle::Table, XrefStyle::Stream] {
        if let Ok(out) = Writer::new(doc.version()).xref_style(style).write(&doc) {
            if let Ok(again) = Document::open_with_limits(&out, LIMITS) {
                read_everything(&again);
            }
        }
    }
});

/// Every object the table lists, each stream decoded.
fn read_everything(doc: &Document<'_>) {
    for (num, entry) in doc.xref().entries() {
        let gen = match entry {
            XrefEntry::InUse { gen, .. } => gen,
            XrefEntry::InStream { .. } => 0,
            XrefEntry::Free { .. } => continue,
        };
        if let Ok(Some(obj @ Object::Stream { .. })) = doc.get(ObjRef { num, gen }) {
            let _ = doc.decoded(&obj);
        }
    }
}
