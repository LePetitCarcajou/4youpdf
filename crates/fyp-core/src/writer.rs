//! Serialisation of a document to a conformant PDF file: header, every
//! indirect object, a cross-reference section as a classic table with its
//! trailer (ISO 32000-2, 7.5.4 and 7.5.5) or as a cross-reference stream
//! (7.5.8), then `startxref` and `%%EOF`.
//!
//! Reading is tolerant; writing is strict. Whatever the source looked like
//! (rebuilt table, wrong `/Length`, objects packed in object streams), the
//! output is a clean, single-section file that any reader loads without
//! repair:
//!
//! - every object is written at top level under its own number and
//!   generation, so compressed objects come out of their object streams;
//! - object streams and cross-reference streams of the source are not
//!   copied: they describe the old file's layout, not the document;
//! - every stream gets a direct `/Length` equal to its data;
//! - the trailer keeps `/Root`, `/Info` and `/ID` (7.5.5, table 15), with
//!   `/ID` generated when the source has none and its second string
//!   refreshed, as a modified file must have (14.4);
//! - an object that cannot be read from the source is left out: readers
//!   treat a reference to it as `null`, as they did in the source.
//!
//! An encrypted source is written in the clear: [`Document`] hands over
//! deciphered strings and streams, the `/Encrypt` dictionary is not
//! copied and the trailer has no `/Encrypt` entry (7.6). Encrypting on
//! output is a later milestone; callers that report on a rewrite must say
//! that the protection is gone.

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write as _;

use crate::document::Document;
use crate::lexer::is_delimiter;
use crate::object::{Dict, Name, ObjRef, Object};
use crate::parser::MAX_DEPTH;
use crate::version::PdfVersion;
use crate::xref::XrefEntry;
use crate::{Error, Result};

/// How the cross-reference section of the output is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum XrefStyle {
    /// Classic `xref` table and `trailer` dictionary (7.5.4). Readable by
    /// every PDF reader; one 20-byte line per object number.
    #[default]
    Table,
    /// Cross-reference stream (7.5.8), Flate-compressed. Needs PDF 1.5:
    /// the header version is raised to 1.5 when lower.
    Stream,
}

/// Serialises a [`Document`] to bytes. Build one with [`Writer::new`],
/// tune it with [`Writer::xref_style`], call [`Writer::write`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Writer {
    version: PdfVersion,
    style: XrefStyle,
}

/// Comment line that follows the header: four bytes above 127 so that
/// transfer software treats the file as binary (7.5.2).
const BINARY_COMMENT: &[u8] = b"%\xE2\xE3\xCF\xD3\n";

/// Largest byte offset a classic table entry can hold: ten digits (7.5.4).
const MAX_TABLE_OFFSET: usize = 9_999_999_999;

/// A file that was never updated has one cross-reference subsection
/// starting at object 0 (7.5.4), so every unused number below the highest
/// one costs a 20-byte free entry. Above this many filler entries the
/// numbering is too sparse for the table style: [`XrefStyle::Stream`]
/// lists ranges through `/Index` instead. Protection against a hostile
/// object number making the output huge.
pub const MAX_TABLE_PADDING: usize = 1 << 20;

/// Generation of object 0, the head of the free list: never reused.
const FREE_HEAD_GEN: u16 = 65535;

impl Writer {
    /// A writer producing files declaring `version` in their header, with
    /// a classic cross-reference table.
    pub fn new(version: PdfVersion) -> Writer {
        Writer {
            version,
            style: XrefStyle::Table,
        }
    }

    /// Choose how the cross-reference section is stored.
    pub fn xref_style(mut self, style: XrefStyle) -> Writer {
        self.style = style;
        self
    }

    /// Version written in the header: the requested one, raised to 1.5
    /// for a cross-reference stream.
    pub fn version(&self) -> PdfVersion {
        let floor = PdfVersion { major: 1, minor: 5 };
        match self.style {
            XrefStyle::Stream => self.version.max(floor),
            XrefStyle::Table => self.version,
        }
    }

    /// Serialise `doc`, a document opened from any file, sound or repaired.
    /// Every object the document can read is written at top level.
    pub fn write(&self, doc: &Document<'_>) -> Result<Vec<u8>> {
        let source_trailer = doc.trailer();
        // The security handler's dictionary describes the source file's
        // protection, which the output does not have.
        let encrypt_ref = match source_trailer.get(&Name::new("Encrypt")) {
            Some(Object::Reference(r)) => Some(*r),
            _ => None,
        };

        let mut out = Vec::new();
        let _ = writeln!(out, "%PDF-{}", self.version());
        out.extend_from_slice(BINARY_COMMENT);

        let mut written: BTreeMap<u32, (usize, u16)> = BTreeMap::new();
        let mut free: BTreeMap<u32, u16> = BTreeMap::new();
        for (num, entry) in doc.xref().entries() {
            let r = match entry {
                XrefEntry::Free { gen } => {
                    free.insert(num, gen);
                    continue;
                }
                XrefEntry::InUse { gen, .. } => ObjRef { num, gen },
                // Compressed objects always have generation 0 (7.5.8.3).
                XrefEntry::InStream { .. } => ObjRef { num, gen: 0 },
            };
            // Object 0 is the head of the free list, never a stored object.
            if num == 0 {
                continue;
            }
            let Ok(Some(obj)) = doc.get(r) else {
                continue;
            };
            if describes_file_layout(&obj) || Some(r) == encrypt_ref {
                continue;
            }
            written.insert(num, (out.len(), r.gen));
            write_indirect(&mut out, r, &obj)?;
        }

        // A catalog written directly in the trailer (tolerated on reading)
        // becomes an indirect object: the standard wants a reference (7.5.5).
        let mut promoted_root = None;
        if let Some(Object::Dict(root)) = source_trailer.get(&Name::new("Root")) {
            let num = written
                .keys()
                .next_back()
                .map_or(Some(1), |max| max.checked_add(1))
                .ok_or_else(|| unwritable("no object number left for the catalog"))?;
            let r = ObjRef { num, gen: 0 };
            written.insert(num, (out.len(), 0));
            write_indirect(&mut out, r, &Object::Dict(root.clone()))?;
            promoted_root = Some(r);
        }

        let trailer = build_trailer(source_trailer, &written, &out, promoted_root)?;
        let startxref = match self.style {
            XrefStyle::Table => write_table(&mut out, &written, &free, trailer)?,
            XrefStyle::Stream => write_xref_stream(&mut out, &written, trailer)?,
        };
        let _ = writeln!(out, "startxref\n{startxref}\n%%EOF");
        Ok(out)
    }
}

/// The trailer entries of table 15 that describe the document rather than
/// the file: `/Root` (or `promoted_root`, the object made out of a direct
/// catalog), `/Info`, `/ID`. `/Size` is added by the section writer;
/// `/Prev` and `/XRefStm` make no sense in a fresh file.
fn build_trailer(
    source: &Dict,
    written: &BTreeMap<u32, (usize, u16)>,
    body: &[u8],
    promoted_root: Option<ObjRef>,
) -> Result<Dict> {
    let points_to_written = |obj: Option<&Object>| match obj {
        Some(Object::Reference(r)) => written
            .get(&r.num)
            .is_some_and(|&(_, gen)| gen == r.gen)
            .then_some(*r),
        _ => None,
    };
    let root = match promoted_root {
        Some(r) => r,
        None => points_to_written(source.get(&Name::new("Root")))
            .ok_or_else(|| unwritable("the trailer's /Root does not lead to a written object"))?,
    };
    let mut trailer = Dict::new();
    trailer.insert(Name::new("Root"), Object::Reference(root));
    if let Some(info) = points_to_written(source.get(&Name::new("Info"))) {
        trailer.insert(Name::new("Info"), Object::Reference(info));
    }
    // 14.4: the first string identifies the document for life, the
    // second one changes with every modification. Both are derived
    // from the content when the source has none.
    let fresh = file_id(body).to_vec();
    let permanent = match source.get(&Name::new("ID")) {
        Some(Object::Array(items)) => match items.first() {
            Some(Object::String(s)) if !s.is_empty() => s.clone(),
            _ => fresh.clone(),
        },
        _ => fresh.clone(),
    };
    trailer.insert(
        Name::new("ID"),
        Object::Array(vec![Object::String(permanent), Object::String(fresh)]),
    );
    Ok(trailer)
}

/// Object streams and cross-reference streams hold the old file's layout,
/// not document content: their objects are written at top level instead.
fn describes_file_layout(obj: &Object) -> bool {
    match obj {
        Object::Stream { dict, .. } => matches!(
            dict.get(&Name::new("Type")).and_then(Object::as_name),
            Some(n) if n.0 == b"ObjStm" || n.0 == b"XRef"
        ),
        _ => false,
    }
}

/// Sixteen bytes derived from `body`, for `/ID`. Deterministic: the same
/// content gives the same identifier.
fn file_id(body: &[u8]) -> [u8; 16] {
    let mut first = DefaultHasher::new();
    body.hash(&mut first);
    let mut second = DefaultHasher::new();
    0x3446_5950_4446_4944_u64.hash(&mut second);
    body.hash(&mut second);
    let mut id = [0u8; 16];
    id[..8].copy_from_slice(&first.finish().to_be_bytes());
    id[8..].copy_from_slice(&second.finish().to_be_bytes());
    id
}

/// Classic table and trailer (7.5.4, 7.5.5). Returns the offset of `xref`.
fn write_table(
    out: &mut Vec<u8>,
    written: &BTreeMap<u32, (usize, u16)>,
    free: &BTreeMap<u32, u16>,
    mut trailer: Dict,
) -> Result<usize> {
    let size = written.keys().next_back().map_or(1, |&max| {
        usize::try_from(max).unwrap_or(usize::MAX).saturating_add(1)
    });
    // Every number below `size` that holds no object is a free entry;
    // object 0 always is.
    let padding = size.saturating_sub(1).saturating_sub(written.len());
    if padding > MAX_TABLE_PADDING {
        return Err(unwritable(format!(
            "object numbers too sparse for a single cross-reference subsection \
             ({padding} filler entries); use the cross-reference stream style"
        )));
    }
    // Bounded by `padding + 1`, checked just above.
    let free_numbers: Vec<u32> = (0..size)
        .filter_map(|n| u32::try_from(n).ok())
        .filter(|n| !written.contains_key(n))
        .collect();

    let xref_pos = out.len();
    let _ = writeln!(out, "xref\n0 {size}");
    let mut next_free = free_numbers.iter().skip(1).copied();
    for num in 0..size {
        let Ok(n) = u32::try_from(num) else {
            break;
        };
        match written.get(&n) {
            Some(&(offset, gen)) => {
                if offset > MAX_TABLE_OFFSET {
                    return Err(unwritable(
                        "object offset beyond the ten digits of a table entry",
                    ));
                }
                let _ = writeln!(out, "{offset:010} {gen:05} n ");
            }
            None => {
                // Free entries link to the next free number, the last one
                // back to 0 (7.5.4).
                let link = next_free.next().unwrap_or(0);
                let gen = if n == 0 {
                    FREE_HEAD_GEN
                } else {
                    free.get(&n).copied().unwrap_or(0)
                };
                let _ = writeln!(out, "{link:010} {gen:05} f ");
            }
        }
    }
    trailer.insert(Name::new("Size"), Object::Integer(int(size)?));
    out.extend_from_slice(b"trailer\n");
    write_dict(&trailer, out, 0)?;
    out.push(b'\n');
    Ok(xref_pos)
}

/// Cross-reference stream (7.5.8), as the last object, listing itself.
/// Returns its offset. Only object numbers in use are listed (`/Index`
/// ranges); numbers left out are free.
fn write_xref_stream(
    out: &mut Vec<u8>,
    written: &BTreeMap<u32, (usize, u16)>,
    mut trailer: Dict,
) -> Result<usize> {
    let max = written.keys().next_back().copied().unwrap_or(0);
    let num = max
        .checked_add(1)
        .ok_or_else(|| unwritable("no object number left for the cross-reference stream"))?;
    let xref_pos = out.len();

    // Field widths (table 17): type, offset or next free number, generation.
    let offset_width = bytes_needed(xref_pos);
    let width = [1usize, offset_width, 2];
    let mut rows = Vec::new();
    let mut index = Vec::new();
    let mut push_row = |n: u32, kind: u8, field2: usize, gen: u16| {
        match index.last_mut() {
            Some((start, count)) if *start + *count == n => *count += 1,
            _ => index.push((n, 1u32)),
        }
        rows.push(kind);
        let field2 = u64::try_from(field2).unwrap_or(u64::MAX).to_be_bytes();
        rows.extend_from_slice(field2.get(8 - offset_width..).unwrap_or_default());
        rows.extend_from_slice(&gen.to_be_bytes());
    };
    push_row(0, 0, 0, FREE_HEAD_GEN);
    for (&n, &(offset, gen)) in written {
        push_row(n, 1, offset, gen);
    }
    push_row(num, 1, xref_pos, 0);

    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    encoder
        .write_all(&rows)
        .and_then(|()| encoder.finish())
        .map(|data| {
            trailer.insert(Name::new("Type"), Object::Name(Name::new("XRef")));
            trailer.insert(Name::new("Size"), Object::Integer(i64::from(num) + 1));
            trailer.insert(
                Name::new("W"),
                Object::Array(
                    width
                        .iter()
                        .map(|&w| Object::Integer(int(w).unwrap_or(0)))
                        .collect(),
                ),
            );
            trailer.insert(
                Name::new("Index"),
                Object::Array(
                    index
                        .iter()
                        .flat_map(|&(start, count)| {
                            [
                                Object::Integer(i64::from(start)),
                                Object::Integer(i64::from(count)),
                            ]
                        })
                        .collect(),
                ),
            );
            trailer.insert(Name::new("Filter"), Object::Name(Name::new("FlateDecode")));
            Object::Stream {
                dict: trailer,
                data,
            }
        })
        .map_err(|e| unwritable(format!("compressing the cross-reference stream: {e}")))
        .and_then(|stream| write_indirect(out, ObjRef { num, gen: 0 }, &stream))?;
    Ok(xref_pos)
}

/// Bytes needed to hold `value` big-endian, at least 1.
fn bytes_needed(value: usize) -> usize {
    let bits = usize::BITS - value.leading_zeros();
    usize::try_from(bits.div_ceil(8)).unwrap_or(8).max(1)
}

/// `n g obj`, the object, `endobj`. Streams get an exact direct `/Length`.
fn write_indirect(out: &mut Vec<u8>, r: ObjRef, obj: &Object) -> Result<()> {
    let _ = writeln!(out, "{} {} obj", r.num, r.gen);
    serialize(obj, out)?;
    out.extend_from_slice(b"\nendobj\n");
    Ok(())
}

/// Append the serialisation of a direct object to `out` (7.3). Streams
/// are written with `/Length` set to the size of their data.
pub fn serialize(obj: &Object, out: &mut Vec<u8>) -> Result<()> {
    serialize_at_depth(obj, out, 0)
}

fn serialize_at_depth(obj: &Object, out: &mut Vec<u8>, depth: usize) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(Error::TooDeep);
    }
    match obj {
        Object::Null => out.extend_from_slice(b"null"),
        Object::Bool(true) => out.extend_from_slice(b"true"),
        Object::Bool(false) => out.extend_from_slice(b"false"),
        Object::Integer(i) => {
            let _ = write!(out, "{i}");
        }
        Object::Real(v) => write_real(*v, out)?,
        Object::String(s) => write_string(s, out),
        Object::Name(n) => write_name(n, out),
        Object::Reference(r) => {
            let _ = write!(out, "{} {} R", r.num, r.gen);
        }
        Object::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b' ');
                }
                serialize_at_depth(item, out, depth + 1)?;
            }
            out.push(b']');
        }
        Object::Dict(dict) => write_dict(dict, out, depth)?,
        Object::Stream { dict, data } => {
            let mut dict = dict.clone();
            dict.insert(Name::new("Length"), Object::Integer(int(data.len())?));
            write_dict(&dict, out, depth)?;
            out.extend_from_slice(b"\nstream\n");
            out.extend_from_slice(data);
            out.extend_from_slice(b"\nendstream");
        }
    }
    Ok(())
}

fn write_dict(dict: &Dict, out: &mut Vec<u8>, depth: usize) -> Result<()> {
    out.extend_from_slice(b"<<");
    for (key, value) in dict {
        out.push(b' ');
        write_name(key, out);
        out.push(b' ');
        serialize_at_depth(value, out, depth + 1)?;
    }
    out.extend_from_slice(b" >>");
    Ok(())
}

/// Reals never use exponent notation (7.3.3). `Display` for `f64` prints
/// the shortest decimal that reads back to the same value and never uses
/// an exponent; a trailing `.0` keeps integral values real.
fn write_real(v: f64, out: &mut Vec<u8>) -> Result<()> {
    if !v.is_finite() {
        return Err(unwritable("a real number is infinite or not a number"));
    }
    let text = format!("{v}");
    out.extend_from_slice(text.as_bytes());
    if !text.contains('.') {
        out.extend_from_slice(b".0");
    }
    Ok(())
}

/// Literal string when every byte is printable ASCII or a tab, CR or LF,
/// with the escapes of 7.3.4.2; hexadecimal string otherwise (7.3.4.3).
/// CR and LF are escaped because a raw CR inside a literal reads back as
/// LF.
fn write_string(s: &[u8], out: &mut Vec<u8>) {
    let printable = s
        .iter()
        .all(|&b| (0x20..=0x7E).contains(&b) || matches!(b, b'\n' | b'\r' | b'\t'));
    if printable {
        out.push(b'(');
        for &b in s {
            match b {
                b'(' | b')' | b'\\' => {
                    out.push(b'\\');
                    out.push(b);
                }
                b'\n' => out.extend_from_slice(b"\\n"),
                b'\r' => out.extend_from_slice(b"\\r"),
                b'\t' => out.extend_from_slice(b"\\t"),
                _ => out.push(b),
            }
        }
        out.push(b')');
    } else {
        out.push(b'<');
        for &b in s {
            let _ = write!(out, "{b:02X}");
        }
        out.push(b'>');
    }
}

/// `/Name`, with `#xx` for every byte outside `!`..`~`, for delimiters and
/// for `#` itself (7.3.5).
fn write_name(name: &Name, out: &mut Vec<u8>) {
    out.push(b'/');
    for &b in &name.0 {
        if (0x21..=0x7E).contains(&b) && !is_delimiter(b) && b != b'#' {
            out.push(b);
        } else {
            let _ = write!(out, "#{b:02X}");
        }
    }
}

fn int(value: usize) -> Result<i64> {
    i64::try_from(value).map_err(|_| unwritable("a size does not fit a PDF integer"))
}

fn unwritable(message: impl Into<String>) -> Error {
    Error::Unwritable {
        message: message.into(),
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use crate::xref::SectionKind;

    fn bytes(obj: &Object) -> Vec<u8> {
        let mut out = Vec::new();
        serialize(obj, &mut out).expect("serialize");
        out
    }

    fn text(obj: &Object) -> String {
        String::from_utf8(bytes(obj)).expect("ascii")
    }

    fn reparse(obj: &Object) -> Object {
        Parser::new(&bytes(obj)).parse_object().expect("reparse")
    }

    fn version(major: u8, minor: u8) -> PdfVersion {
        PdfVersion { major, minor }
    }

    #[test]
    fn scalars_and_reals() {
        assert_eq!(text(&Object::Null), "null");
        assert_eq!(text(&Object::Bool(true)), "true");
        assert_eq!(text(&Object::Integer(-42)), "-42");
        assert_eq!(text(&Object::Real(1.5)), "1.5");
        assert_eq!(text(&Object::Real(5.0)), "5.0");
        assert_eq!(text(&Object::Real(-0.0)), "-0.0");
        assert_eq!(text(&Object::Real(1e-7)), "0.0000001");
        assert_eq!(text(&Object::Real(1e21)), "1000000000000000000000.0");
        for v in [0.1, -2.5e-9, 123_456.789, 1e300] {
            assert_eq!(reparse(&Object::Real(v)), Object::Real(v), "{v}");
        }
        assert!(matches!(
            serialize(&Object::Real(f64::INFINITY), &mut Vec::new()),
            Err(Error::Unwritable { .. })
        ));
        assert!(matches!(
            serialize(&Object::Real(f64::NAN), &mut Vec::new()),
            Err(Error::Unwritable { .. })
        ));
    }

    #[test]
    fn strings_literal_and_hex() {
        assert_eq!(text(&Object::String(b"a(b)c\\d".to_vec())), r"(a\(b\)c\\d)");
        assert_eq!(
            text(&Object::String(b"l1\r\nl2\t".to_vec())),
            r"(l1\r\nl2\t)"
        );
        assert_eq!(text(&Object::String(Vec::new())), "()");
        assert_eq!(text(&Object::String(b"\x00\xFFA".to_vec())), "<00FF41>");
        for s in [
            &b"plain"[..],
            b"(((",
            b"\\",
            b"\r",
            b"\r\n",
            b"\n\r",
            b"bin\x01\x80\xFE",
            b"",
        ] {
            assert_eq!(
                reparse(&Object::String(s.to_vec())),
                Object::String(s.to_vec())
            );
        }
    }

    #[test]
    fn names_with_escapes() {
        assert_eq!(text(&Object::Name(Name::new("Type"))), "/Type");
        assert_eq!(text(&Object::Name(Name::new("A B"))), "/A#20B");
        assert_eq!(
            text(&Object::Name(Name::new("a#b/c(d)"))),
            "/a#23b#2Fc#28d#29"
        );
        assert_eq!(text(&Object::Name(Name(vec![0xE9, 0x00]))), "/#E9#00");
        assert_eq!(text(&Object::Name(Name(Vec::new()))), "/");
        for n in [&b"plain"[..], b"with space", b"#", b"<>[]{}/%", b"\xFF\x00"] {
            assert_eq!(
                reparse(&Object::Name(Name(n.to_vec()))),
                Object::Name(Name(n.to_vec()))
            );
        }
    }

    #[test]
    fn containers_and_streams() {
        let mut dict = Dict::new();
        dict.insert(Name::new("Type"), Object::Name(Name::new("Page")));
        dict.insert(
            Name::new("Kids"),
            Object::Array(vec![Object::Reference(ObjRef { num: 4, gen: 0 })]),
        );
        dict.insert(Name::new("Empty"), Object::Array(Vec::new()));
        dict.insert(Name::new("Sub"), Object::Dict(Dict::new()));
        assert_eq!(
            text(&Object::Dict(dict.clone())),
            "<< /Empty [] /Kids [4 0 R] /Sub << >> /Type /Page >>"
        );
        assert_eq!(
            reparse(&Object::Dict(dict.clone())),
            Object::Dict(dict.clone())
        );

        // A wrong or indirect /Length is replaced by the exact one.
        dict.insert(
            Name::new("Length"),
            Object::Reference(ObjRef { num: 9, gen: 0 }),
        );
        let stream = Object::Stream {
            dict,
            data: b"hello\nendstream?".to_vec(),
        };
        let out = text(&stream);
        assert!(out.contains("/Length 16 /Sub"), "{out}");
        assert!(
            out.ends_with(">>\nstream\nhello\nendstream?\nendstream"),
            "{out}"
        );
        match reparse(&stream) {
            Object::Stream { dict, data } => {
                assert_eq!(data, b"hello\nendstream?");
                assert_eq!(dict.get(&Name::new("Length")), Some(&Object::Integer(16)));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn nesting_is_bounded() {
        let mut obj = Object::Null;
        for _ in 0..(MAX_DEPTH + 2) {
            obj = Object::Array(vec![obj]);
        }
        assert_eq!(serialize(&obj, &mut Vec::new()), Err(Error::TooDeep));
    }

    /// A small file with a catalog (1), page tree (2), page (3), a stream
    /// whose /Length is indirect (4, length in 5) and gaps in numbering.
    fn source() -> Vec<u8> {
        let mut f = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (num, body) in [
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>",
            ),
            (4, "<< /Length 5 0 R >>\nstream\nBT ET\nendstream"),
            (5, "5"),
            (8, "(free numbers 6 and 7)"),
        ] {
            offsets.push((num, f.len()));
            f.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
        }
        let xref = f.len();
        f.extend_from_slice(b"xref\n0 9\n0000000000 65535 f \n");
        for num in 1..9u32 {
            match offsets.iter().find(|(n, _)| *n == num) {
                Some((_, off)) => f.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
                None => f.extend_from_slice(b"0000000000 00001 f \n"),
            }
        }
        f.extend_from_slice(
            format!("trailer\n<< /Size 9 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );
        f
    }

    #[test]
    fn classic_table_layout() {
        let src = source();
        let doc = Document::open(&src).expect("open source");
        let out = Writer::new(version(1, 4)).write(&doc).expect("write");
        assert!(out.starts_with(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n"));
        assert!(out.ends_with(b"%%EOF\n"));
        // One subsection from 0, 20-byte entries, free list 0 -> 6 -> 7 -> 0
        // with the generations the source declared.
        let table_at = crate::parser::find(&out, b"xref\n0 9\n").expect("xref keyword");
        let entries =
            std::str::from_utf8(&out[table_at + 9..table_at + 9 + 9 * 20]).expect("ascii");
        let lines: Vec<&str> = entries.split_inclusive('\n').collect();
        assert_eq!(lines.len(), 9);
        assert!(
            lines.iter().all(|l| l.len() == 20 && l.ends_with(" \n")),
            "{lines:?}"
        );
        assert_eq!(lines[0], "0000000006 65535 f \n");
        assert_eq!(lines[6], "0000000007 00001 f \n");
        assert_eq!(lines[7], "0000000000 00001 f \n");
        for (num, line) in lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.ends_with("n \n"))
        {
            let offset: usize = line[..10].parse().unwrap();
            assert!(
                out[offset..].starts_with(format!("{num} 0 obj\n").as_bytes()),
                "object {num}"
            );
        }
        assert!(out.ends_with(format!("startxref\n{table_at}\n%%EOF\n").as_bytes()));
        // Trailer: /Size, /Root, a generated /ID of two equal strings, no /Prev.
        let again = Document::open(&out).expect("reopen");
        assert_eq!(again.reconstructed(), None);
        assert_eq!(again.xref().kind(), SectionKind::Table);
        let trailer = again.trailer();
        assert_eq!(trailer.get(&Name::new("Size")), Some(&Object::Integer(9)));
        assert!(!trailer.contains_key(&Name::new("Prev")));
        match trailer.get(&Name::new("ID")) {
            Some(Object::Array(id)) => match id.as_slice() {
                [Object::String(a), Object::String(b)] => {
                    assert_eq!(a.len(), 16);
                    assert_eq!(a, b);
                }
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
        // The stream now has a direct, exact /Length; object 5 survives.
        match again
            .get(ObjRef { num: 4, gen: 0 })
            .expect("get")
            .expect("listed")
        {
            Object::Stream { dict, data } => {
                assert_eq!(dict.get(&Name::new("Length")), Some(&Object::Integer(5)));
                assert_eq!(data, b"BT ET");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            again.get(ObjRef { num: 5, gen: 0 }),
            Ok(Some(Object::Integer(5)))
        );
        assert_eq!(again.page_count(), Ok(1));
    }

    #[test]
    fn xref_stream_layout_and_version_floor() {
        let src = source();
        let doc = Document::open(&src).expect("open source");
        let writer = Writer::new(version(1, 4)).xref_style(XrefStyle::Stream);
        assert_eq!(writer.version(), version(1, 5));
        let out = writer.write(&doc).expect("write");
        assert!(out.starts_with(b"%PDF-1.5\n"));
        let again = Document::open(&out).expect("reopen");
        assert_eq!(again.reconstructed(), None);
        assert_eq!(again.xref().kind(), SectionKind::Stream);
        // The stream is object 9, listed at its own offset; 6 and 7 are free.
        let trailer = again.trailer();
        assert_eq!(trailer.get(&Name::new("Size")), Some(&Object::Integer(10)));
        assert_eq!(
            trailer.get(&Name::new("Index")),
            Some(&Object::Array(
                [0, 6, 8, 2].iter().map(|&i| Object::Integer(i)).collect()
            ))
        );
        assert_eq!(again.xref().get(6), None);
        let Some(XrefEntry::InUse { offset, gen: 0 }) = again.xref().get(9) else {
            panic!("{:?}", again.xref().get(9));
        };
        assert!(out[offset..].starts_with(b"9 0 obj\n"));
        assert_eq!(again.xref().object_count(), 7);
        assert_eq!(again.page_count(), Ok(1));
        assert_eq!(
            Writer::new(version(1, 7))
                .xref_style(XrefStyle::Stream)
                .version(),
            version(1, 7)
        );
    }

    #[test]
    fn rewriting_is_stable_and_keeps_the_permanent_id() {
        let src = source();
        let doc = Document::open(&src).expect("open");
        let first = Writer::new(version(1, 4)).write(&doc).expect("first");
        let doc2 = Document::open(&first).expect("reopen");
        let second = Writer::new(version(1, 4)).write(&doc2).expect("second");
        assert_eq!(first, second);

        // A source with its own /ID keeps the first string, refreshes the second.
        let with_id = String::from_utf8_lossy(&src).replace(
            "/Root 1 0 R",
            "/Root 1 0 R /ID [<00112233445566778899AABBCCDDEEFF> (old)]",
        );
        let doc3 = Document::open(with_id.as_bytes()).expect("open with id");
        let out = Writer::new(version(1, 4)).write(&doc3).expect("write");
        let again = Document::open(&out).expect("reopen");
        match again.trailer().get(&Name::new("ID")) {
            Some(Object::Array(id)) => match id.as_slice() {
                [Object::String(a), Object::String(b)] => {
                    assert_eq!(
                        a,
                        &[
                            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB,
                            0xCC, 0xDD, 0xEE, 0xFF
                        ]
                    );
                    assert_ne!(b, b"old");
                    assert_eq!(b.len(), 16);
                }
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn refusals() {
        // Sparse numbering: object 5000000 forces five million filler entries.
        let sparse = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog /Pages 5000000 0 R >>\nendobj\n5000000 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\ntrailer\n<< /Root 1 0 R >>\n";
        let doc = Document::open(sparse).expect("open by scan");
        assert!(matches!(
            Writer::new(version(1, 4)).write(&doc),
            Err(Error::Unwritable { message }) if message.contains("sparse")
        ));
        let out = Writer::new(version(1, 4))
            .xref_style(XrefStyle::Stream)
            .write(&doc)
            .expect("stream style handles gaps");
        let again = Document::open(&out).expect("reopen");
        assert_eq!(again.reconstructed(), None);
        assert_eq!(again.xref().object_count(), 3);
        assert_eq!(again.page_count(), Ok(0));
        // /Root leading nowhere sends the file to the scan, which finds the
        // catalog by its /Type: written fine.
        let no_root = String::from_utf8_lossy(&source()).replace("/Root 1 0 R", "/Root 7 0 R");
        let doc = Document::open(no_root.as_bytes()).expect("open");
        assert!(doc.reconstructed().is_some());
        assert!(Writer::new(version(1, 4)).write(&doc).is_ok());
        // Without a /Type /Catalog anywhere, nothing can serve as /Root.
        let no_catalog = no_root.replace("/Type /Catalog", "/Type /Catalox");
        let doc = Document::open(no_catalog.as_bytes()).expect("open");
        assert!(doc.reconstructed().is_some());
        assert!(matches!(
            Writer::new(version(1, 4)).write(&doc),
            Err(Error::Unwritable { message }) if message.contains("/Root")
        ));
    }

    #[test]
    fn helpers() {
        assert_eq!(bytes_needed(0), 1);
        assert_eq!(bytes_needed(255), 1);
        assert_eq!(bytes_needed(256), 2);
        assert_eq!(bytes_needed(1 << 24), 4);
        assert_eq!(file_id(b"a"), file_id(b"a"));
        assert_ne!(file_id(b"a"), file_id(b"b"));
        assert_ne!(file_id(b"a")[..8], file_id(b"a")[8..]);
    }
}
