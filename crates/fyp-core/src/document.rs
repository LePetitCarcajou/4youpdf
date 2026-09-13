//! Document access: opens a file through its cross-reference table and reads
//! objects on demand (ISO 32000-2, 7.5 and 7.7), including objects stored
//! in object streams (7.5.7).
//!
//! Opening tries the declared table first and checks that every object it
//! lists is really where it says. When the table is missing, unreadable or
//! wrong, the index is rebuilt by scanning the file ([`crate::recover`]);
//! [`Document::reconstructed`] then tells why. A repaired file never passes
//! for a sound one.
//!
//! An encrypted file (ISO 32000-2, 7.6) is deciphered transparently: the
//! `/Encrypt` dictionary is read when the document opens, with the empty
//! password unless [`Document::open_with_password`] gives another, and
//! every string and stream handed out by [`Document::get`] is in the
//! clear. [`Document::encryption`] says whether that happened and how the
//! file was protected.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use crate::encryption::{Crypt, Encryption};
use crate::filters::{self, DecodeLimits};
use crate::lexer::{is_delimiter, is_whitespace, Lexer, Token};
use crate::object::{Dict, Name, ObjRef, Object};
use crate::parser::find;
use crate::parser::Parser;
use crate::recover;
use crate::version::{self, PdfVersion};
use crate::xref::{Xref, XrefEntry};
use crate::{Error, Result};

/// A PDF file opened through its cross-reference table. Objects are parsed
/// lazily from the borrowed input.
#[derive(Debug)]
pub struct Document<'a> {
    input: &'a [u8],
    version: PdfVersion,
    xref: Xref,
    limits: DecodeLimits,
    /// Why the declared cross-reference table was replaced by a scan of
    /// the file, if it was.
    reconstructed: Option<Error>,
    /// Offset of the newest section when `startxref` pointed elsewhere
    /// and the section was found nearby (see [`locate_section`]).
    relocated_startxref: Option<usize>,
    /// The security handler, when the file is encrypted.
    crypt: Option<Crypt>,
    /// Object streams already decoded, by object number. Decoding one is
    /// the expensive part; parsing an object out of it is cheap.
    object_streams: Mutex<BTreeMap<u32, Arc<ObjectStream>>>,
}

/// A decoded object stream (ISO 32000-2, 7.5.7): its data and, for each
/// object it holds, the object number and the absolute offset in `data`.
#[derive(Debug)]
pub(crate) struct ObjectStream {
    pub(crate) data: Vec<u8>,
    pub(crate) objects: Vec<(u32, usize)>,
    /// First offset listed for each object number, for the lookup by
    /// number when the index in the table is wrong. Keeps that fallback
    /// logarithmic on streams holding tens of thousands of objects.
    pub(crate) by_number: BTreeMap<u32, usize>,
}

impl ObjectStream {
    /// Read the header of decoded object-stream data: `count` pairs
    /// `objnum offset`, offsets relative to `first` (ISO 32000-2, 7.5.7).
    pub(crate) fn from_decoded(
        stream_num: u32,
        decoded: Vec<u8>,
        count: usize,
        first: usize,
    ) -> Result<ObjectStream> {
        let bad = |message: &str| Error::BadObjectStream {
            stream_num,
            message: message.into(),
        };
        if first > decoded.len() {
            return Err(bad("/First lies beyond the decoded data"));
        }
        // `count` comes from the file: never allocate from it. Each pair
        // consumes input, and the loop stops at the first token that is not
        // a pair, so it is bounded by the header's size.
        let mut lexer = Lexer::new(decoded.get(..first).unwrap_or_default());
        let mut objects = Vec::new();
        for _ in 0..count {
            lexer.skip_whitespace_and_comments();
            let Token::Integer(num) = lexer.next_token()? else {
                break;
            };
            lexer.skip_whitespace_and_comments();
            let Token::Integer(rel) = lexer.next_token()? else {
                return Err(bad("object number without offset in the header"));
            };
            let num = u32::try_from(num).map_err(|_| bad("object number out of range"))?;
            let offset = usize::try_from(rel)
                .ok()
                .and_then(|rel| first.checked_add(rel))
                .filter(|&o| o <= decoded.len())
                .ok_or_else(|| bad(&format!("offset of object {num} lies beyond the data")))?;
            objects.push((num, offset));
        }
        let mut by_number = BTreeMap::new();
        for &(num, offset) in &objects {
            by_number.entry(num).or_insert(offset);
        }
        Ok(ObjectStream {
            data: decoded,
            objects,
            by_number,
        })
    }
}

impl Clone for Document<'_> {
    fn clone(&self) -> Self {
        Document {
            input: self.input,
            version: self.version,
            xref: self.xref.clone(),
            limits: self.limits,
            reconstructed: self.reconstructed.clone(),
            relocated_startxref: self.relocated_startxref,
            crypt: self.crypt.clone(),
            object_streams: Mutex::new(self.cache().clone()),
        }
    }
}

impl<'a> Document<'a> {
    /// Check the header, then load the cross-reference chain announced by
    /// the last `startxref`, under the default [`DecodeLimits`]. Falls back
    /// to scanning the file when that chain cannot be used.
    pub fn open(input: &'a [u8]) -> Result<Document<'a>> {
        Document::open_with_limits(input, DecodeLimits::default())
    }

    /// Same as [`Document::open`] with explicit limits, applied to every
    /// stream decoded on behalf of the document (cross-reference streams,
    /// object streams, [`Document::decoded`]).
    pub fn open_with_limits(input: &'a [u8], limits: DecodeLimits) -> Result<Document<'a>> {
        Document::open_with(input, limits, b"")
    }

    /// Same as [`Document::open`] for an encrypted file whose user or
    /// owner password is not empty. The password is taken as bytes: for
    /// revisions 2 to 4 they are used as is (PDFDocEncoding, 7.6.4.3.2),
    /// for revisions 5 and 6 they must be UTF-8 (7.6.4.3.3).
    pub fn open_with_password(input: &'a [u8], password: &[u8]) -> Result<Document<'a>> {
        Document::open_with(input, DecodeLimits::default(), password)
    }

    /// The general form of [`Document::open`]: explicit limits and password.
    ///
    /// Fails with [`Error::BadHeader`] when the input is not a PDF at all,
    /// with [`Error::Unrecoverable`] when the declared table is unusable
    /// and the file holds no object to rebuild one from, with
    /// [`Error::BadEncryption`] when the file says it is encrypted but its
    /// `/Encrypt` dictionary cannot be used, and with
    /// [`Error::WrongPassword`] when `password` opens nothing.
    pub fn open_with(
        input: &'a [u8],
        limits: DecodeLimits,
        password: &[u8],
    ) -> Result<Document<'a>> {
        let info = version::quick_info(input)?;
        let mut relocated_startxref = None;
        let declared = match info.startxref {
            None => Err(Error::MissingStartxref),
            Some(startxref) => {
                let start = locate_section(input, startxref, info.header_offset);
                if start != startxref {
                    relocated_startxref = Some(start);
                }
                Xref::parse_with_limits(input, start, limits)
                    .and_then(|xref| verify(input, &xref).map(|()| xref))
            }
        };
        let (xref, reconstructed) = match declared {
            Ok(xref) => (xref, None),
            Err(declared) => {
                relocated_startxref = None;
                match recover::reconstruct(input, limits) {
                    Some(xref) => (xref, Some(declared)),
                    None => {
                        return Err(Error::Unrecoverable {
                            declared: Box::new(declared),
                        })
                    }
                }
            }
        };
        let mut doc = Document {
            input,
            version: info.version,
            xref,
            limits,
            reconstructed,
            relocated_startxref,
            crypt: None,
            object_streams: Mutex::new(BTreeMap::new()),
        };
        // With `crypt` still unset, `resolve` returns the `/Encrypt`
        // dictionary as stored, which is what the handler needs (7.6.3).
        let crypt = Crypt::open(doc.trailer(), |o| doc.resolve(o), password)?;
        doc.crypt = crypt;
        Ok(doc)
    }

    /// Offset of the cross-reference section actually read when the
    /// declared `startxref` pointed at something else and the section was
    /// found nearby (offsets off by a few bytes, junk before the header).
    /// `None` when `startxref` was right, or when the file was scanned
    /// ([`Document::reconstructed`]). Such a file is not sound; callers
    /// that report on a file should say so.
    pub fn relocated_startxref(&self) -> Option<usize> {
        self.relocated_startxref
    }

    /// How the file is encrypted, or `None` for a file stored in the
    /// clear. When `Some`, every object [`Document::get`] returns has
    /// already been deciphered.
    pub fn encryption(&self) -> Option<Encryption> {
        self.crypt.as_ref().map(Crypt::info)
    }

    /// Why the cross-reference table was rebuilt by scanning the file
    /// ([`crate::recover`]), or `None` when the declared table was used as
    /// found. A repaired file must never pass for a sound one: callers that
    /// report on a file should show this.
    pub fn reconstructed(&self) -> Option<&Error> {
        self.reconstructed.as_ref()
    }

    /// Version declared in the header.
    pub fn version(&self) -> PdfVersion {
        self.version
    }

    /// Limits applied when decoding streams for this document.
    pub fn limits(&self) -> DecodeLimits {
        self.limits
    }

    /// Trailer of the newest cross-reference section (ISO 32000-2, 7.5.5).
    pub fn trailer(&self) -> &Dict {
        self.xref.trailer()
    }

    /// Cross-reference table merged across incremental updates.
    pub fn xref(&self) -> &Xref {
        &self.xref
    }

    /// Read object `r` where the table says it is: at a byte offset, or
    /// inside an object stream.
    ///
    /// `Ok(None)` when the table does not list `r` as stored with that
    /// generation: such a reference denotes the null object
    /// (ISO 32000-2, 7.3.10). If the object found at the offset is not `r`,
    /// the table is wrong and this is an [`Error::Syntax`].
    pub fn get(&self, r: ObjRef) -> Result<Option<Object>> {
        match self.xref.get(r.num) {
            Some(XrefEntry::InUse { offset, gen }) if gen == r.gen => {
                self.parse_at(offset, r).map(Some)
            }
            // Compressed objects always have generation 0 (7.5.8.3).
            Some(XrefEntry::InStream { stream_num, index }) if r.gen == 0 => {
                self.get_compressed(r.num, stream_num, index).map(Some)
            }
            _ => Ok(None),
        }
    }

    /// Follow `obj` one level if it is a reference; a reference to a missing
    /// object gives `null`. Any other object is returned unchanged.
    pub fn resolve(&self, obj: &Object) -> Result<Object> {
        match obj {
            Object::Reference(r) => Ok(self.get(*r)?.unwrap_or(Object::Null)),
            other => Ok(other.clone()),
        }
    }

    /// Decoded data of a stream object: its `/Filter` chain applied under
    /// the document's limits, indirect `/Filter` and `/DecodeParms` values
    /// resolved. Anything but a stream is [`Error::BadStructure`].
    pub fn decoded(&self, obj: &Object) -> Result<Vec<u8>> {
        match obj {
            Object::Stream { dict, data } => {
                filters::decode_stream_with(dict, data, |o| self.resolve(o), self.limits)
            }
            _ => Err(structure("not a stream")),
        }
    }

    /// Document catalog, the trailer's `/Root` (ISO 32000-2, 7.7.2).
    pub fn catalog(&self) -> Result<Dict> {
        let root = self
            .trailer()
            .get(&Name::new("Root"))
            .ok_or_else(|| structure("trailer has no /Root"))?;
        self.resolve_dict(root, "/Root")
    }

    /// Number of pages: `/Count` of the root of the page tree
    /// (ISO 32000-2, 7.7.3.2).
    pub fn page_count(&self) -> Result<usize> {
        let catalog = self.catalog()?;
        let pages = catalog
            .get(&Name::new("Pages"))
            .ok_or_else(|| structure("catalog has no /Pages"))?;
        let pages = self.resolve_dict(pages, "/Pages")?;
        let count = pages
            .get(&Name::new("Count"))
            .ok_or_else(|| structure("page tree root has no /Count"))?;
        self.resolve(count)?
            .as_i64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| structure("page tree /Count is not a non-negative integer"))
    }

    fn resolve_dict(&self, obj: &Object, what: &str) -> Result<Dict> {
        match self.resolve(obj)? {
            Object::Dict(dict) => Ok(dict),
            _ => Err(structure(format!("{what} is not a dictionary"))),
        }
    }

    /// Parse the indirect object at `offset`, check it is `r`, and
    /// decipher it when the file is encrypted.
    fn parse_at(&self, offset: usize, r: ObjRef) -> Result<Object> {
        if offset >= self.input.len() {
            return Err(Error::BadXref {
                offset,
                message: format!("object {} {} lies beyond end of file", r.num, r.gen),
            });
        }
        let (found, obj) = Parser::at(self.input, offset).parse_indirect()?;
        if found != r {
            return Err(Error::Syntax {
                offset,
                message: format!(
                    "xref points to object {} {}, found {} {}",
                    r.num, r.gen, found.num, found.gen
                ),
            });
        }
        Ok(match &self.crypt {
            Some(crypt) => crypt.decrypt_object(r, obj),
            None => obj,
        })
    }

    /// Like [`Document::resolve`], but only for objects stored at a byte
    /// offset. Used while loading an object stream, whose dictionary must
    /// not depend on another object stream (7.5.7: object streams cannot
    /// be nested); a reference into one gives `null`.
    fn resolve_top_level(&self, obj: &Object) -> Result<Object> {
        match obj {
            Object::Reference(r) => match self.xref.get(r.num) {
                Some(XrefEntry::InUse { offset, gen }) if gen == r.gen => self.parse_at(offset, *r),
                _ => Ok(Object::Null),
            },
            other => Ok(other.clone()),
        }
    }

    /// Object `num`, the `index`-th object of object stream `stream_num`.
    fn get_compressed(&self, num: u32, stream_num: u32, index: u32) -> Result<Object> {
        let stream = self.object_stream(stream_num)?;
        let bad = |message: String| Error::BadObjectStream {
            stream_num,
            message,
        };
        let at_index = usize::try_from(index)
            .ok()
            .and_then(|i| stream.objects.get(i))
            .filter(|(n, _)| *n == num);
        // Tolerance: an index that does not match is a writer's slip; the
        // object number list is authoritative (7.5.7).
        let offset = at_index
            .map(|&(_, offset)| offset)
            .or_else(|| stream.by_number.get(&num).copied())
            .ok_or_else(|| bad(format!("does not hold object {num}")))?;
        let obj = Parser::at(&stream.data, offset)
            .parse_object()
            .map_err(|e| bad(format!("object {num} at offset {offset}: {e}")))?;
        if matches!(obj, Object::Stream { .. }) {
            return Err(bad(format!(
                "object {num} is a stream; streams cannot be inside an object stream"
            )));
        }
        Ok(obj)
    }

    /// Object stream `stream_num`, decoded and cached.
    fn object_stream(&self, stream_num: u32) -> Result<Arc<ObjectStream>> {
        if let Some(cached) = self.cache().get(&stream_num) {
            return Ok(Arc::clone(cached));
        }
        let stream = Arc::new(self.load_object_stream(stream_num)?);
        self.cache().insert(stream_num, Arc::clone(&stream));
        Ok(stream)
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, BTreeMap<u32, Arc<ObjectStream>>> {
        // A panic while the lock was held cannot happen in this crate;
        // if a caller's thread died anyway the map is still consistent.
        self.object_streams
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Read and decode object stream `stream_num` (7.5.7): a top-level
    /// stream whose data starts with `/N` pairs `objnum offset`, offsets
    /// relative to `/First`.
    fn load_object_stream(&self, stream_num: u32) -> Result<ObjectStream> {
        let bad = |message: &str| Error::BadObjectStream {
            stream_num,
            message: message.into(),
        };
        let r = ObjRef {
            num: stream_num,
            gen: 0,
        };
        let (dict, data) = match self.xref.get(stream_num) {
            Some(XrefEntry::InUse { offset, gen: 0 }) => match self.parse_at(offset, r)? {
                Object::Stream { dict, data } => (dict, data),
                _ => return Err(bad("not a stream")),
            },
            Some(XrefEntry::InStream { .. }) => {
                return Err(bad(
                    "lies inside another object stream; object streams cannot be nested",
                ))
            }
            _ => return Err(bad("not listed in the cross-reference table")),
        };
        let int = |key: &str| -> Result<Option<usize>> {
            Ok(match dict.get(&Name::new(key)) {
                None => None,
                Some(obj) => Some(
                    self.resolve_top_level(obj)?
                        .as_i64()
                        .and_then(|v| usize::try_from(v).ok())
                        .ok_or_else(|| bad(&format!("/{key} is not a non-negative integer")))?,
                ),
            })
        };
        let count = int("N")?.ok_or_else(|| bad("no /N"))?;
        let first = int("First")?.ok_or_else(|| bad("no /First"))?;
        let decoded =
            filters::decode_stream_with(&dict, &data, |o| self.resolve_top_level(o), self.limits)?;
        ObjectStream::from_decoded(stream_num, decoded, count, first)
    }
}

/// How far from the declared `startxref` a section is looked for.
const RELOCATION_WINDOW: usize = 512;

/// Where the newest cross-reference section really starts. `declared` is
/// what `startxref` says; when nothing starts there, the offset shifted by
/// the header's position is tried (junk before `%PDF` moves everything,
/// corpus: qpdf `leading-junk.pdf`), then the nearest `xref` keyword or
/// cross-reference stream within [`RELOCATION_WINDOW`] bytes (offsets off
/// by a few bytes, corpus: 16 files). A wrong pick is harmless: the
/// section is verified like any other, and the file is scanned if that
/// fails.
fn locate_section(input: &[u8], declared: usize, header_offset: usize) -> usize {
    if section_starts_at(input, declared) {
        return declared;
    }
    let shifted = declared.saturating_add(header_offset);
    if header_offset > 0 && section_starts_at(input, shifted) {
        return shifted;
    }
    let low = declared.saturating_sub(RELOCATION_WINDOW);
    let high = declared.saturating_add(RELOCATION_WINDOW).min(input.len());
    (low..high)
        .filter(|&at| section_starts_at(input, at))
        .min_by_key(|&at| at.abs_diff(declared))
        .unwrap_or(declared)
}

/// Does a cross-reference section start at `at`: the `xref` keyword as a
/// whole token, or an `n g obj` header followed by a `/XRef` dictionary?
fn section_starts_at(input: &[u8], at: usize) -> bool {
    let Some(rest) = input.get(at..) else {
        return false;
    };
    let before = at.checked_sub(1).and_then(|i| input.get(i)).copied();
    if rest.starts_with(b"xref") {
        let before_ok = before.is_none_or(|b| !b.is_ascii_alphanumeric());
        let after_ok = rest
            .get(4)
            .is_none_or(|&b| is_whitespace(b) || is_delimiter(b));
        return before_ok && after_ok;
    }
    if rest.first().is_some_and(u8::is_ascii_digit) && !before.is_some_and(|b| b.is_ascii_digit()) {
        let mut parser = Parser::at(input, at);
        if parser.parse_indirect_header().is_ok() {
            let end = parser.pos();
            let dict = input
                .get(end..end.saturating_add(512).min(input.len()))
                .unwrap_or_default();
            return find(dict, b"/XRef").is_some();
        }
    }
    false
}

/// Check that the declared table matches the file: every in-use entry has
/// its `n g obj` header at the announced offset, every compressed object
/// names an object stream stored at an offset, and the trailer's `/Root`
/// leads to a dictionary. Cheap (three tokens per object, one full parse
/// for the catalog) and decisive: any failure means the table is wrong
/// and the file must be scanned.
fn verify(input: &[u8], xref: &Xref) -> Result<()> {
    for (num, entry) in xref.entries() {
        match entry {
            XrefEntry::Free { .. } => {}
            XrefEntry::InUse { offset, gen } => {
                let found = Parser::at(input, offset).parse_indirect_header().ok();
                if found != Some(ObjRef { num, gen }) {
                    return Err(Error::BadXref {
                        offset,
                        message: match found {
                            Some(f) => format!(
                                "object {num} {gen} announced here, found {} {}",
                                f.num, f.gen
                            ),
                            None => format!("object {num} {gen} announced here, none found"),
                        },
                    });
                }
            }
            XrefEntry::InStream { stream_num, .. } => {
                if !matches!(xref.get(stream_num), Some(XrefEntry::InUse { .. })) {
                    return Err(Error::BadXref {
                        offset: 0,
                        message: format!(
                            "object {num} lies in object stream {stream_num}, which is not stored in the file"
                        ),
                    });
                }
            }
        }
    }
    // A table whose `/Root` leads nowhere is as wrong as one with a bad
    // offset: the scan may find a catalog (corpus: qpdf `issue-99.pdf`,
    // pdf.js `REDHAT-1531897-0.pdf`).
    match xref.trailer().get(&Name::new("Root")) {
        None => Err(structure("trailer has no /Root")),
        // Tolerated: a catalog written directly in the trailer (corpus:
        // pdf.js `issue9105_other.pdf`); the writer makes it indirect.
        Some(Object::Dict(_)) => Ok(()),
        Some(Object::Reference(r)) => match xref.get(r.num) {
            Some(XrefEntry::InUse { offset, gen }) if gen == r.gen => {
                match Parser::at(input, offset).parse_indirect() {
                    Ok((_, Object::Dict(_))) => Ok(()),
                    _ => Err(Error::BadXref {
                        offset,
                        message: format!(
                            "/Root {} {} announced here is not a readable dictionary",
                            r.num, r.gen
                        ),
                    }),
                }
            }
            Some(XrefEntry::InStream { .. }) if r.gen == 0 => Ok(()),
            _ => Err(Error::BadXref {
                offset: 0,
                message: format!("/Root {} {} is not listed in the table", r.num, r.gen),
            }),
        },
        Some(_) => Err(structure("/Root is neither a reference nor a dictionary")),
    }
}

fn structure(message: impl Into<String>) -> Error {
    Error::BadStructure {
        message: message.into(),
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::io::Write;

    const CATALOG: &str = "<< /Type /Catalog /Pages 2 0 R >>";

    /// One-section PDF where `objects[i]` becomes object `i + 1`. `patch`
    /// may alter the offsets written to the xref to simulate broken tables.
    fn build(objects: &[&str], patch: impl FnOnce(&mut [usize])) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
        }
        patch(offsets.as_mut_slice());
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

    /// A stream object body with the given dictionary entries and raw data.
    fn stream(extra: &str, data: &[u8]) -> Vec<u8> {
        let mut out = format!("<< {extra} /Length {} >>\nstream\n", data.len()).into_bytes();
        out.extend_from_slice(data);
        out.extend_from_slice(b"\nendstream");
        out
    }

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        enc.write_all(data).expect("compress");
        enc.finish().expect("finish")
    }

    /// PDF whose objects are given as raw bodies, with an uncompressed
    /// cross-reference stream (`/W [1 4 2]`) as the last object. `rows`
    /// gives the xref entries for objects 1..=n as `(type, field2, field3)`;
    /// `None` means "type 1 at the object's real offset".
    fn build_with_xref_stream(
        objects: &[(u32, Vec<u8>)],
        rows: &[Option<(u8, u64, u64)>],
        size: u32,
    ) -> Vec<u8> {
        let mut out = b"%PDF-1.5\n".to_vec();
        let mut offsets = BTreeMap::new();
        for (num, body) in objects {
            offsets.insert(*num, out.len());
            out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_num = size - 1;
        let startxref = out.len();
        let mut data = vec![0, 0, 0, 0, 0, 0xff, 0xff];
        for (i, row) in rows.iter().enumerate() {
            let num = u32::try_from(i + 1).unwrap();
            // `None`: type 1 at the object's real offset, or free when the
            // object was not written (a bogus offset would send the file
            // to the scan).
            let (t, f2, f3) = row.unwrap_or_else(|| match offsets.get(&num) {
                Some(&off) => (1, u64::try_from(off).unwrap(), 0),
                None => (0, 0, 0),
            });
            data.push(t);
            data.extend_from_slice(&u32::try_from(f2).unwrap().to_be_bytes());
            data.extend_from_slice(&u16::try_from(f3).unwrap().to_be_bytes());
        }
        // The xref stream lists itself.
        data.push(1);
        data.extend_from_slice(&u32::try_from(startxref).unwrap().to_be_bytes());
        data.extend_from_slice(&[0, 0]);
        out.extend_from_slice(format!("{xref_num} 0 obj\n").as_bytes());
        out.extend_from_slice(&stream(
            &format!("/Type /XRef /Size {size} /W [1 4 2] /Root 1 0 R"),
            &data,
        ));
        out.extend_from_slice(format!("\nendobj\nstartxref\n{startxref}\n%%EOF\n").as_bytes());
        out
    }

    /// Object stream body holding `objects` as `(num, source)`.
    fn object_stream(objects: &[(u32, &str)], flate: bool) -> Vec<u8> {
        let mut header = String::new();
        let mut body = String::new();
        for (num, src) in objects {
            header.push_str(&format!("{num} {} ", body.len()));
            body.push_str(src);
            body.push('\n');
        }
        let content = format!("{header}\n{body}");
        let first = header.len() + 1;
        let n = objects.len();
        if flate {
            stream(
                &format!("/Type /ObjStm /N {n} /First {first} /Filter /FlateDecode"),
                &zlib(content.as_bytes()),
            )
        } else {
            stream(
                &format!("/Type /ObjStm /N {n} /First {first}"),
                content.as_bytes(),
            )
        }
    }

    const PAGES: &str = "<< /Type /Pages /Kids [3 0 R] /Count 1 >>";
    const PAGE: &str = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>";

    #[test]
    fn indirect_count_is_resolved() {
        let file = build(
            &[CATALOG, "<< /Type /Pages /Kids [] /Count 3 0 R >>", "2"],
            |_| {},
        );
        let doc = Document::open(&file).expect("open");
        assert_eq!(doc.page_count(), Ok(2));
    }

    #[test]
    fn sound_table_is_used_as_is() {
        let file = build(&[CATALOG, "<< /Type /Pages /Kids [] /Count 0 >>"], |_| {});
        let doc = Document::open(&file).expect("open");
        assert_eq!(doc.reconstructed(), None);
        assert_eq!(doc.xref().kind(), crate::xref::SectionKind::Table);
    }

    #[test]
    fn wrong_offsets_trigger_reconstruction() {
        let file = build(&[CATALOG, "<< /Type /Pages /Kids [] /Count 0 >>"], |o| {
            o.swap(0, 1)
        });
        let doc = Document::open(&file).expect("open");
        assert!(
            matches!(doc.reconstructed(), Some(Error::BadXref { .. })),
            "{:?}",
            doc.reconstructed()
        );
        assert_eq!(doc.xref().kind(), crate::xref::SectionKind::Reconstructed);
        assert!(matches!(
            doc.get(ObjRef { num: 1, gen: 0 }),
            Ok(Some(Object::Dict(_)))
        ));
        assert_eq!(doc.page_count(), Ok(0));
        // Offset off by a few bytes, and offset beyond the end of file.
        for patch in [(|o: &mut [usize]| o[1] += 3) as fn(&mut [usize]), |o| {
            o[1] = 999_999_999
        }] {
            let file = build(&[CATALOG, "<< /Type /Pages /Kids [] /Count 0 >>"], patch);
            let doc = Document::open(&file).expect("open");
            assert!(matches!(doc.reconstructed(), Some(Error::BadXref { .. })));
            assert_eq!(doc.page_count(), Ok(0));
        }
    }

    #[test]
    fn unlisted_free_or_other_generation_is_null() {
        let file = build(&[CATALOG], |_| {});
        let doc = Document::open(&file).expect("open");
        assert_eq!(doc.get(ObjRef { num: 1, gen: 1 }), Ok(None));
        assert_eq!(doc.get(ObjRef { num: 0, gen: 65535 }), Ok(None));
        assert_eq!(doc.get(ObjRef { num: 9, gen: 0 }), Ok(None));
        assert_eq!(
            doc.resolve(&Object::Reference(ObjRef { num: 9, gen: 0 })),
            Ok(Object::Null)
        );
        assert_eq!(doc.resolve(&Object::Integer(7)), Ok(Object::Integer(7)));
    }

    #[test]
    fn broken_page_tree_is_an_error() {
        let negative = build(&[CATALOG, "<< /Type /Pages /Kids [] /Count -1 >>"], |_| {});
        let no_pages = build(&["<< /Type /Catalog >>"], |_| {});
        let root_not_dict = build(&["[1 2 3]"], |_| {});
        for file in [negative, no_pages, root_not_dict] {
            let doc = Document::open(&file).expect("open");
            let count = doc.page_count();
            assert!(
                matches!(count, Err(Error::BadStructure { .. })),
                "{count:?}"
            );
        }
        // A trailer without /Root sends the file to the scan; with no
        // object in it, there is nothing to rebuild from.
        let no_root = b"%PDF-1.7\nxref\n0 1\n0000000000 65535 f \ntrailer\n<< /Size 1 >>\nstartxref\n9\n%%EOF\n";
        let err = Document::open(no_root).map(|_| ()).unwrap_err();
        assert!(
            matches!(&err, Error::Unrecoverable { declared }
                if matches!(**declared, Error::BadStructure { .. })),
            "{err:?}"
        );
    }

    #[test]
    fn missing_startxref_is_repaired_when_objects_exist() {
        let file = b"%PDF-1.7\n1 0 obj << >> endobj\n";
        let doc = Document::open(file).expect("open");
        assert_eq!(doc.reconstructed(), Some(&Error::MissingStartxref));
        assert!(matches!(doc.catalog(), Err(Error::BadStructure { .. })));
        assert!(matches!(
            doc.get(ObjRef { num: 1, gen: 0 }),
            Ok(Some(Object::Dict(_)))
        ));
        let empty = b"%PDF-1.7\nnothing here\n";
        assert_eq!(
            Document::open(empty).map(|_| ()),
            Err(Error::Unrecoverable {
                declared: Box::new(Error::MissingStartxref)
            })
        );
        assert_eq!(
            Document::open(b"not a pdf").map(|_| ()),
            Err(Error::BadHeader)
        );
    }

    #[test]
    fn decoded_applies_filters_with_document_limits() {
        // Hex-encoded zlib data keeps the file ASCII for `build`.
        let hex: String = zlib(b"payload")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
            + ">";
        let body = String::from_utf8_lossy(&stream(
            "/Filter [/ASCIIHexDecode /FlateDecode]",
            hex.as_bytes(),
        ))
        .into_owned();
        let file = build(&[CATALOG, &body], |_| {});
        let doc = Document::open(&file).expect("open");
        let obj = doc.get(ObjRef { num: 2, gen: 0 }).unwrap().unwrap();
        assert_eq!(doc.decoded(&obj).unwrap(), b"payload");
        assert!(matches!(
            doc.decoded(&Object::Integer(1)),
            Err(Error::BadStructure { .. })
        ));
        let tight = DecodeLimits { max_output: 3 };
        let doc = Document::open_with_limits(&file, tight).expect("open");
        assert_eq!(doc.limits(), tight);
        assert!(matches!(
            doc.decoded(&obj),
            Err(Error::LimitExceeded { limit: 3, .. })
        ));
    }

    // --- Object streams (7.5.7) ---

    #[test]
    fn objects_come_out_of_a_flate_object_stream() {
        let objstm = object_stream(&[(1, CATALOG), (2, PAGES)], true);
        let file = build_with_xref_stream(
            &[(3, PAGE.into()), (4, objstm)],
            &[Some((2, 4, 0)), Some((2, 4, 1)), None, None],
            6,
        );
        let doc = Document::open(&file).expect("open");
        assert_eq!(
            doc.xref().get(1),
            Some(XrefEntry::InStream {
                stream_num: 4,
                index: 0
            })
        );
        assert_eq!(doc.page_count(), Ok(1));
        let catalog = doc.get(ObjRef { num: 1, gen: 0 }).unwrap().unwrap();
        assert_eq!(
            catalog.as_dict().unwrap().get(&Name::new("Type")),
            Some(&Object::Name(Name::new("Catalog")))
        );
        // Generation 1 of a compressed object does not exist.
        assert_eq!(doc.get(ObjRef { num: 1, gen: 1 }), Ok(None));
        // The stream is decoded once: the cache holds it after the first read.
        assert_eq!(doc.cache().len(), 1);
        let _ = doc.get(ObjRef { num: 2, gen: 0 }).unwrap();
        assert_eq!(doc.cache().len(), 1);
        // A clone carries the cache along.
        assert_eq!(doc.clone().cache().len(), 1);
        // The xref stream itself is a readable stream object.
        assert!(matches!(
            doc.get(ObjRef { num: 5, gen: 0 }),
            Ok(Some(Object::Stream { .. }))
        ));
    }

    #[test]
    fn wrong_index_falls_back_to_the_object_number_list() {
        let objstm = object_stream(&[(1, CATALOG), (2, PAGES)], false);
        // Indexes swapped, and an index beyond the list.
        let file = build_with_xref_stream(
            &[(3, PAGE.into()), (4, objstm)],
            &[Some((2, 4, 1)), Some((2, 4, 9)), None, None],
            6,
        );
        let doc = Document::open(&file).expect("open");
        assert_eq!(doc.page_count(), Ok(1));
        assert!(matches!(
            doc.get(ObjRef { num: 2, gen: 0 }),
            Ok(Some(Object::Dict(_)))
        ));
    }

    #[test]
    fn object_stream_cannot_be_nested_or_hold_a_stream() {
        // Object 4 (the object stream holding 1 and 2) is itself declared
        // inside object stream 9, a real object stream holding object 8.
        let objstm = object_stream(&[(1, CATALOG), (2, PAGES)], true);
        let outer = object_stream(&[(8, "<< >>")], false);
        let free = Some((0, 0, 0));
        let file = build_with_xref_stream(
            &[(3, PAGE.into()), (4, objstm.clone()), (9, outer)],
            &[
                Some((2, 4, 0)),
                Some((2, 4, 1)),
                None,
                Some((2, 9, 0)),
                free,
                free,
                free,
                Some((2, 9, 0)),
                None,
            ],
            11,
        );
        // The table is wrong (7.5.7 forbids nesting), so the file is
        // scanned: object stream 4 is found at its real offset and object 1
        // is read from it.
        let doc = Document::open(&file).expect("open");
        assert!(
            matches!(doc.reconstructed(), Some(Error::BadXref { message, .. })
                if message.contains("object stream 4")),
            "{:?}",
            doc.reconstructed()
        );
        assert!(matches!(doc.xref().get(4), Some(XrefEntry::InUse { .. })));
        assert!(matches!(
            doc.get(ObjRef { num: 1, gen: 0 }),
            Ok(Some(Object::Dict(_)))
        ));
        assert_eq!(doc.page_count(), Ok(1));
        // Object 1 inside the object stream is a stream.
        let inner = String::from_utf8_lossy(&stream("/Length 1", b"x")).into_owned();
        let objstm = object_stream(&[(1, &inner), (2, PAGES)], false);
        let file = build_with_xref_stream(
            &[(3, PAGE.into()), (4, objstm)],
            &[Some((2, 4, 0)), Some((2, 4, 1)), None, None],
            6,
        );
        let doc = Document::open(&file).expect("open");
        let err = doc.get(ObjRef { num: 1, gen: 0 });
        assert!(
            matches!(err, Err(Error::BadObjectStream { stream_num: 4, .. })),
            "{err:?}"
        );
        // Object 2 in the same stream is still fine.
        assert!(matches!(
            doc.get(ObjRef { num: 2, gen: 0 }),
            Ok(Some(Object::Dict(_)))
        ));
    }

    #[test]
    fn malformed_object_streams() {
        let cases: Vec<(&str, Vec<u8>)> = vec![
            (
                "not a stream",
                b"<< /Type /ObjStm /N 1 /First 4 >>".to_vec(),
            ),
            ("no /N", stream("/Type /ObjStm /First 4", b"1 0 << >>")),
            ("no /First", stream("/Type /ObjStm /N 1", b"1 0 << >>")),
            (
                "/First beyond data",
                stream("/Type /ObjStm /N 1 /First 400", b"1 0 << >>"),
            ),
            (
                "negative /N",
                stream("/Type /ObjStm /N -1 /First 4", b"1 0 << >>"),
            ),
            (
                "offset beyond data",
                stream("/Type /ObjStm /N 1 /First 6", b"1 900 << >>"),
            ),
            (
                "header without offset",
                stream("/Type /ObjStm /N 1 /First 2", b"1 << >>"),
            ),
            (
                "object missing",
                stream("/Type /ObjStm /N 1 /First 4", b"7 0 << >>"),
            ),
            (
                "garbage object",
                stream("/Type /ObjStm /N 1 /First 4", b"1 0 >>"),
            ),
        ];
        for (what, objstm) in cases {
            let file = build_with_xref_stream(
                &[(3, PAGE.into()), (4, objstm)],
                &[Some((2, 4, 0)), None, None, None],
                6,
            );
            let doc = Document::open(&file).expect("open");
            let got = doc.get(ObjRef { num: 1, gen: 0 });
            assert!(
                matches!(got, Err(Error::BadObjectStream { stream_num: 4, .. })),
                "{what}: {got:?}"
            );
        }
        // Huge /N with a tiny header: no allocation, no hang.
        let objstm = stream("/Type /ObjStm /N 4000000000 /First 4", b"1 0 << /A 1 >>");
        let file = build_with_xref_stream(
            &[(3, PAGE.into()), (4, objstm)],
            &[Some((2, 4, 0)), None, None, None],
            6,
        );
        let doc = Document::open(&file).expect("open");
        assert!(matches!(
            doc.get(ObjRef { num: 1, gen: 0 }),
            Ok(Some(Object::Dict(_)))
        ));
        // Stream number not in the table: the table is wrong, the file is
        // scanned instead, and object 1 is simply not found.
        let file = build_with_xref_stream(
            &[(3, PAGE.into())],
            &[Some((2, 40, 0)), Some((0, 0, 0)), None, Some((0, 0, 0))],
            6,
        );
        let doc = Document::open(&file).expect("open");
        assert!(matches!(doc.reconstructed(), Some(Error::BadXref { .. })));
        assert_eq!(doc.get(ObjRef { num: 1, gen: 0 }), Ok(None));
    }

    #[test]
    fn object_stream_that_inflates_past_the_limit() {
        let zeros = vec![b' '; 1 << 20];
        let mut content = b"1 0 ".to_vec();
        content.extend_from_slice(&zeros);
        content.extend_from_slice(b"<< /Type /Catalog >>");
        let objstm = stream(
            "/Type /ObjStm /N 1 /First 4 /Filter /FlateDecode",
            &zlib(&content),
        );
        let file = build_with_xref_stream(
            &[(3, PAGE.into()), (4, objstm)],
            &[Some((2, 4, 0)), None, None, None],
            6,
        );
        let limits = DecodeLimits { max_output: 4096 };
        let doc = Document::open_with_limits(&file, limits).expect("open");
        assert!(matches!(
            doc.get(ObjRef { num: 1, gen: 0 }),
            Err(Error::LimitExceeded { limit: 4096, .. })
        ));
        let doc = Document::open(&file).expect("open");
        assert!(matches!(
            doc.get(ObjRef { num: 1, gen: 0 }),
            Ok(Some(Object::Dict(_)))
        ));
    }
}
