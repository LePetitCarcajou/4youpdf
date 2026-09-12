//! Cross-reference tables (ISO 32000-2, 7.5.4), cross-reference streams
//! (7.5.8), file trailers (7.5.5), the `/Prev` chain of incremental updates
//! (7.5.6) and the `/XRefStm` entry of hybrid-reference files (7.5.8.4).
//!
//! Lookup order for one object number (7.5.8.4): the newest section's
//! table, then the stream named by that section's `/XRefStm`, then the
//! section at `/Prev`, and so on. Within a hybrid section, an entry of the
//! `/XRefStm` stream replaces a *free* entry of the table: writers hide
//! compressed objects from pre-1.5 readers that way. The `/Prev` entry of a
//! stream reached through `/XRefStm` is ignored; the table's own `/Prev`
//! already continues the chain.

use std::collections::{BTreeMap, BTreeSet};

use crate::filters::{self, DecodeLimits};
use crate::lexer::{Lexer, Token};
use crate::object::{Dict, Name, Object};
use crate::parser::Parser;
use crate::{Error, Result};

/// One cross-reference entry (ISO 32000-2, 7.5.4 and table 18).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XrefEntry {
    /// `f` entry, or type 0: the object number is free.
    Free {
        /// Generation number for the next reuse of this object number.
        gen: u16,
    },
    /// `n` entry, or type 1: the object is stored in the file.
    InUse {
        /// Byte offset of its `n g obj` header.
        offset: usize,
        /// Generation number.
        gen: u16,
    },
    /// Type 2: the object is compressed in an object stream (7.5.7). Such
    /// objects always have generation 0.
    InStream {
        /// Object number of the object stream holding it.
        stream_num: u32,
        /// Position of the object within that stream, counted from 0.
        index: u32,
    },
}

/// How a cross-reference section is stored in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    /// Classic `xref` table followed by a `trailer` dictionary
    /// (ISO 32000-2, 7.5.4).
    Table,
    /// Cross-reference stream (ISO 32000-2, 7.5.8).
    Stream,
    /// Classic table whose trailer points to a cross-reference stream
    /// through `/XRefStm` (ISO 32000-2, 7.5.8.4).
    Hybrid,
    /// No usable section in the file: the table was rebuilt by scanning
    /// for `n g obj` headers (see [`crate::recover`]).
    Reconstructed,
}

/// Cross-reference table merged across incremental updates, with the
/// newest trailer.
#[derive(Debug, Clone, PartialEq)]
pub struct Xref {
    entries: BTreeMap<u32, XrefEntry>,
    trailer: Dict,
    kind: SectionKind,
}

/// Largest field width in a cross-reference stream's `/W` array. A field
/// is read into a `u64`, so 8 bytes is the natural bound; anything larger
/// is a hostile or broken file.
const MAX_FIELD_WIDTH: usize = 8;

impl Xref {
    /// Read the section at `startxref`, then each older section reached
    /// through `/Prev`. For every object number the newest entry wins
    /// (ISO 32000-2, 7.5.6).
    ///
    /// Nothing is sized from values found in the file (`/Size`, subsection
    /// counts, `/Index`): only entries actually present are stored.
    pub fn parse(input: &[u8], startxref: usize) -> Result<Xref> {
        Xref::parse_with_limits(input, startxref, DecodeLimits::default())
    }

    /// Same as [`Xref::parse`], with the limits applied when decoding
    /// cross-reference streams.
    pub fn parse_with_limits(input: &[u8], startxref: usize, limits: DecodeLimits) -> Result<Xref> {
        let mut visited = BTreeSet::new();
        let (newest, kind) = load_section(input, startxref, limits, &mut visited)?;
        let mut next = prev_offset(&newest.trailer, startxref)?;
        let mut entries = newest.entries;
        while let Some(offset) = next {
            let (older, _) = load_section(input, offset, limits, &mut visited)?;
            for (num, entry) in older.entries {
                // Sections are read newest first: keep what is already there.
                entries.entry(num).or_insert(entry);
            }
            next = prev_offset(&older.trailer, offset)?;
        }
        free_object_zero(&mut entries);
        Ok(Xref {
            entries,
            trailer: newest.trailer,
            kind,
        })
    }

    /// A table rebuilt by [`crate::recover`], kind
    /// [`SectionKind::Reconstructed`].
    pub(crate) fn reconstructed(entries: BTreeMap<u32, XrefEntry>, trailer: Dict) -> Xref {
        Xref {
            entries,
            trailer,
            kind: SectionKind::Reconstructed,
        }
    }

    /// Trailer of the newest section (ISO 32000-2, 7.5.5). For a
    /// cross-reference stream this is the stream dictionary (7.5.8.2).
    pub fn trailer(&self) -> &Dict {
        &self.trailer
    }

    /// How the newest section, the one `startxref` points to, is stored.
    /// Older sections reached through `/Prev` may differ: an incremental
    /// update can append a stream to a file written with a table.
    pub fn kind(&self) -> SectionKind {
        self.kind
    }

    /// Entry for object number `num`, if any section lists it.
    pub fn get(&self, num: u32) -> Option<XrefEntry> {
        self.entries.get(&num).copied()
    }

    /// All entries, free ones included, by increasing object number.
    pub fn entries(&self) -> impl Iterator<Item = (u32, XrefEntry)> + '_ {
        self.entries.iter().map(|(&num, &entry)| (num, entry))
    }

    /// Number of entries that denote a stored object: `n` entries and
    /// objects inside object streams.
    pub fn object_count(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| !matches!(entry, XrefEntry::Free { .. }))
            .count()
    }
}

/// One cross-reference section, table or stream, as read from the file.
struct Section {
    entries: BTreeMap<u32, XrefEntry>,
    trailer: Dict,
    /// `/XRefStm` of a hybrid-reference file's trailer (7.5.8.4).
    xref_stm: Option<usize>,
}

/// Read the section at `offset` and, for a hybrid file, merge in the
/// entries of its `/XRefStm` stream (7.5.8.4). `visited` holds every offset
/// already read along the chain, so that loops are errors, not hangs.
fn load_section(
    input: &[u8],
    offset: usize,
    limits: DecodeLimits,
    visited: &mut BTreeSet<usize>,
) -> Result<(Section, SectionKind)> {
    if !visited.insert(offset) {
        return Err(Error::XrefLoop { offset });
    }
    let (mut section, kind) = parse_section(input, offset, limits)?;
    if let Some(stm) = section.xref_stm {
        if !visited.insert(stm) {
            return Err(Error::XrefLoop { offset: stm });
        }
        let hybrid = parse_stream_section(input, stm, limits)?;
        for (num, entry) in hybrid.entries {
            match section.entries.get(&num) {
                None | Some(XrefEntry::Free { .. }) => {
                    section.entries.insert(num, entry);
                }
                Some(_) => {}
            }
        }
    }
    Ok((section, kind))
}

/// Parse the section at `offset`: a classic `xref` table or a
/// cross-reference stream, whichever is there.
fn parse_section(
    input: &[u8],
    offset: usize,
    limits: DecodeLimits,
) -> Result<(Section, SectionKind)> {
    if offset >= input.len() {
        return Err(bad(offset, "offset beyond end of file"));
    }
    let mut lexer = Lexer::at(input, offset);
    match next(&mut lexer)? {
        (Token::Keyword(k), _) if k == b"xref" => {
            let section = parse_table_section(input, &mut lexer)?;
            let kind = if section.xref_stm.is_some() {
                SectionKind::Hybrid
            } else {
                SectionKind::Table
            };
            Ok((section, kind))
        }
        (Token::Integer(_), _) => Ok((
            parse_stream_section(input, offset, limits)?,
            SectionKind::Stream,
        )),
        (_, at) => Err(bad(at, "expected `xref` or a cross-reference stream")),
    }
}

/// Parse one `xref` ... `trailer << >>` section; the lexer is just past
/// the `xref` keyword.
fn parse_table_section(input: &[u8], lexer: &mut Lexer<'_>) -> Result<Section> {
    let mut entries = BTreeMap::new();
    loop {
        match next(lexer)? {
            (Token::Keyword(k), _) if k == b"trailer" => break,
            (Token::Integer(first), first_at) => {
                let first = u32::try_from(first)
                    .map_err(|_| bad(first_at, "subsection start out of range"))?;
                let count = match next(lexer)? {
                    (Token::Integer(n), count_at) => u32::try_from(n)
                        .map_err(|_| bad(count_at, "subsection count out of range"))?,
                    (_, at) => return Err(bad(at, "expected subsection entry count")),
                };
                // `count` comes from the file: never allocate from it. The
                // loop stops at the first token that is not an entry, so its
                // cost is bounded by the input size.
                for i in 0..count {
                    let num = first
                        .checked_add(i)
                        .ok_or_else(|| bad(first_at, "object number out of range"))?;
                    entries.insert(num, parse_entry(lexer)?);
                }
            }
            (Token::Eof, _) => return Err(Error::UnexpectedEof),
            (_, at) => return Err(bad(at, "expected subsection header or `trailer`")),
        }
    }
    let trailer_at = lexer.pos();
    let trailer = match Parser::at(input, trailer_at).parse_object()? {
        Object::Dict(trailer) => trailer,
        _ => return Err(bad(trailer_at, "trailer is not a dictionary")),
    };
    let xref_stm = match trailer.get(&Name::new("XRefStm")) {
        None => None,
        Some(obj) => Some(
            obj.as_i64()
                .and_then(|p| usize::try_from(p).ok())
                .ok_or_else(|| bad(trailer_at, "trailer /XRefStm is not a valid offset"))?,
        ),
    };
    Ok(Section {
        entries,
        trailer,
        xref_stm,
    })
}

/// One entry, `nnnnnnnnnn ggggg n` or `... f` (ISO 32000-2, 7.5.4). The
/// standard fixes a 20-byte layout; reading tokens instead tolerates the
/// common deviations in spacing and end-of-line markers.
fn parse_entry(lexer: &mut Lexer<'_>) -> Result<XrefEntry> {
    let (t1, at) = next(lexer)?;
    let (t2, _) = next(lexer)?;
    let (kind, _) = next(lexer)?;
    let (Token::Integer(field), Token::Integer(gen)) = (t1, t2) else {
        return Err(bad(at, "malformed cross-reference entry"));
    };
    match kind {
        // An `n` entry at offset 0 is a free entry written by a careless
        // writer: the header sits at offset 0, no object can (same
        // tolerance as in `decode_row`).
        Token::Keyword(k) if k == b"n" && field == 0 => Ok(XrefEntry::Free {
            gen: u16::try_from(gen).unwrap_or(u16::MAX),
        }),
        Token::Keyword(k) if k == b"n" => {
            let gen = u16::try_from(gen).map_err(|_| bad(at, "generation out of range"))?;
            let offset = usize::try_from(field).map_err(|_| bad(at, "negative object offset"))?;
            Ok(XrefEntry::InUse { offset, gen })
        }
        // Tolerance: many writers put `65536` on the head of the free list
        // instead of the `65535` of 7.5.4. The generation of a free entry
        // only says what the next reuse would get, so an out-of-range value
        // is clamped to "never reused" rather than condemning the table.
        Token::Keyword(k) if k == b"f" => Ok(XrefEntry::Free {
            gen: u16::try_from(gen).unwrap_or(u16::MAX),
        }),
        _ => Err(bad(at, "entry type must be `n` or `f`")),
    }
}

/// Parse a cross-reference stream, `n g obj << /Type /XRef ... >> stream`,
/// at `offset` (ISO 32000-2, 7.5.8.2 and 7.5.8.3).
fn parse_stream_section(input: &[u8], offset: usize, limits: DecodeLimits) -> Result<Section> {
    if offset >= input.len() {
        return Err(bad(offset, "offset beyond end of file"));
    }
    let (_, obj) = Parser::at(input, offset)
        .parse_indirect()
        .map_err(|_| bad(offset, "expected `xref` or a cross-reference stream"))?;
    let Object::Stream { dict, data } = obj else {
        return Err(bad(offset, "cross-reference object is not a stream"));
    };
    if dict.get(&Name::new("Type")).and_then(Object::as_name) != Some(&Name::new("XRef")) {
        return Err(bad(offset, "stream is not of /Type /XRef"));
    }
    // Entries of a cross-reference stream dictionary are direct objects
    // (7.5.8.2): references are not followed, there is no table yet.
    let decoded = filters::decode_stream_with(&dict, &data, |o| Ok(o.clone()), limits)?;
    let widths = field_widths(&dict, offset)?;
    let row_len: usize = widths.iter().sum();
    let mut rows = decoded.chunks_exact(row_len.max(1));
    let mut entries = BTreeMap::new();
    // `/Index` and `/Size` are counts from the file: the loops below stop
    // at the last row actually present, whatever they claim.
    'subsections: for (first, count) in index_pairs(&dict, offset)? {
        for i in 0..count {
            let Some(row) = rows.next() else {
                break 'subsections;
            };
            let num = u32::try_from(u64::from(first) + i)
                .map_err(|_| bad(offset, "object number out of range"))?;
            if let Some(entry) = decode_row(row, widths, offset)? {
                entries.insert(num, entry);
            }
        }
    }
    Ok(Section {
        entries,
        trailer: dict,
        xref_stm: None,
    })
}

/// `/W`: the byte width of the three fields of every row (7.5.8.2, table
/// 17). Widths above [`MAX_FIELD_WIDTH`] or an all-zero `/W` are refused.
fn field_widths(dict: &Dict, offset: usize) -> Result<[usize; 3]> {
    let Some(Object::Array(w)) = dict.get(&Name::new("W")) else {
        return Err(bad(offset, "cross-reference stream has no /W array"));
    };
    let mut widths = [0usize; 3];
    let mut items = w.iter();
    for slot in &mut widths {
        let width = items
            .next()
            .and_then(Object::as_i64)
            .and_then(|v| usize::try_from(v).ok())
            .filter(|&v| v <= MAX_FIELD_WIDTH)
            .ok_or_else(|| bad(offset, "/W must hold three widths between 0 and 8"))?;
        *slot = width;
    }
    if widths.iter().all(|&w| w == 0) {
        return Err(bad(offset, "/W declares empty rows"));
    }
    Ok(widths)
}

/// `/Index`: pairs `first count` (7.5.8.2). Defaults to `[0 /Size]`; when
/// `/Size` is unusable too, every row is taken from object 0 on.
fn index_pairs(dict: &Dict, offset: usize) -> Result<Vec<(u32, u64)>> {
    match dict.get(&Name::new("Index")) {
        None => {
            let size = dict
                .get(&Name::new("Size"))
                .and_then(Object::as_i64)
                .and_then(|s| u64::try_from(s).ok())
                .unwrap_or(u64::MAX);
            Ok(vec![(0, size)])
        }
        Some(Object::Array(items)) => {
            if items.len() % 2 != 0 {
                return Err(bad(offset, "/Index must hold pairs of integers"));
            }
            items
                .chunks_exact(2)
                .map(|pair| {
                    let first = pair
                        .first()
                        .and_then(Object::as_i64)
                        .and_then(|v| u32::try_from(v).ok());
                    let count = pair
                        .get(1)
                        .and_then(Object::as_i64)
                        .and_then(|v| u64::try_from(v).ok());
                    match (first, count) {
                        (Some(f), Some(c)) => Ok((f, c)),
                        _ => Err(bad(offset, "/Index must hold non-negative integers")),
                    }
                })
                .collect()
        }
        Some(_) => Err(bad(offset, "/Index is not an array")),
    }
}

/// One row of a cross-reference stream (7.5.8.3, table 18). Types other
/// than 0, 1 and 2 denote the null object: `None`.
fn decode_row(row: &[u8], widths: [usize; 3], offset: usize) -> Result<Option<XrefEntry>> {
    let [w1, w2, w3] = widths;
    let (type_bytes, rest) = row.split_at(w1.min(row.len()));
    let (f2_bytes, f3_bytes) = rest.split_at(w2.min(rest.len()));
    // A missing type field means type 1 (table 17).
    let kind = if w1 == 0 { 1 } else { big_endian(type_bytes) };
    let f2 = big_endian(f2_bytes);
    let f3 = big_endian(f3_bytes.get(..w3).unwrap_or(f3_bytes));
    Ok(match kind {
        0 => Some(XrefEntry::Free {
            gen: u16::try_from(f3).unwrap_or(u16::MAX),
        }),
        // Tolerance: some writers list unused numbers as "in use at offset
        // 0" instead of type 0. No object can start at offset 0, where the
        // header is, so readers treat such rows as free.
        1 if f2 == 0 => Some(XrefEntry::Free {
            gen: u16::try_from(f3).unwrap_or(u16::MAX),
        }),
        1 => Some(XrefEntry::InUse {
            offset: usize::try_from(f2).map_err(|_| bad(offset, "object offset out of range"))?,
            gen: u16::try_from(f3).map_err(|_| bad(offset, "generation out of range"))?,
        }),
        2 => Some(XrefEntry::InStream {
            stream_num: u32::try_from(f2)
                .map_err(|_| bad(offset, "object stream number out of range"))?,
            index: u32::try_from(f3)
                .map_err(|_| bad(offset, "index in object stream out of range"))?,
        }),
        _ => None,
    })
}

/// Big-endian unsigned integer of at most 8 bytes (guaranteed by
/// [`field_widths`]).
fn big_endian(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0u64, |acc, &b| (acc << 8) | u64::from(b))
}

/// Next token with its start offset.
fn next(lexer: &mut Lexer<'_>) -> Result<(Token, usize)> {
    lexer.skip_whitespace_and_comments();
    let at = lexer.pos();
    Ok((lexer.next_token()?, at))
}

/// Object number 0 is the head of the free list and never a stored object
/// (ISO 32000-2, 7.5.4). A table listing it in use describes a writer's
/// junk object (corpus: qpdf `obj0.pdf`), which readers ignore.
fn free_object_zero(entries: &mut BTreeMap<u32, XrefEntry>) {
    if let Some(entry) = entries.get_mut(&0) {
        if !matches!(entry, XrefEntry::Free { .. }) {
            *entry = XrefEntry::Free { gen: u16::MAX };
        }
    }
}

/// `/Prev` of a trailer: offset of the previous section, if any.
fn prev_offset(trailer: &Dict, section: usize) -> Result<Option<usize>> {
    let Some(prev) = trailer.get(&Name::new("Prev")) else {
        return Ok(None);
    };
    prev.as_i64()
        .and_then(|p| usize::try_from(p).ok())
        .map(Some)
        .ok_or_else(|| bad(section, "trailer /Prev is not a valid offset"))
}

fn bad(offset: usize, message: &str) -> Error {
    Error::BadXref {
        offset,
        message: message.into(),
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const SIMPLE: &[u8] = b"xref\n0 1\n0000000000 65535 f \n3 2\n0000000017 00000 n \n0000000081 00002 n \ntrailer\n<< /Size 5 >>\n";

    fn is_bad_xref<T>(result: Result<T>) -> bool {
        matches!(result, Err(Error::BadXref { .. }))
    }

    /// An uncompressed cross-reference stream as object 9, `/W [1 2 1]`.
    fn xref_stream(extra: &str, rows: &[u8]) -> Vec<u8> {
        let mut out = format!(
            "9 0 obj\n<< /Type /XRef /W [1 2 1] {extra} /Length {} >>\nstream\n",
            rows.len()
        )
        .into_bytes();
        out.extend_from_slice(rows);
        out.extend_from_slice(b"\nendstream\nendobj\n");
        out
    }

    #[test]
    fn subsections_and_entry_types() {
        let xref = Xref::parse(SIMPLE, 0).expect("xref");
        assert_eq!(xref.get(0), Some(XrefEntry::Free { gen: 65535 }));
        assert_eq!(xref.get(1), None);
        assert_eq!(xref.get(3), Some(XrefEntry::InUse { offset: 17, gen: 0 }));
        assert_eq!(xref.get(4), Some(XrefEntry::InUse { offset: 81, gen: 2 }));
        assert_eq!(xref.object_count(), 2);
        assert_eq!(xref.entries().count(), 3);
        assert_eq!(
            xref.trailer()
                .get(&Name::new("Size"))
                .and_then(Object::as_i64),
            Some(5)
        );
    }

    #[test]
    fn free_entry_generation_out_of_range_is_clamped() {
        // `65536 f` on the free-list head is a common writer slip: the
        // table stays usable, the value reads as "never reused".
        let file =
            b"xref\n0 2\n0000000000 65536 f \n0000000009 00000 n \ntrailer\n<< /Root 1 0 R >>\n";
        let xref = Xref::parse(file, 0).expect("parse");
        assert_eq!(xref.get(0), Some(XrefEntry::Free { gen: 65535 }));
        assert_eq!(xref.get(1), Some(XrefEntry::InUse { offset: 9, gen: 0 }));
        // On an in-use entry the generation is meaningful: still refused.
        let in_use = b"xref\n0 2\n0000000000 65535 f \n0000000009 65536 n \ntrailer\n<< >>\n";
        assert!(is_bad_xref(Xref::parse(in_use, 0)));
    }

    #[test]
    fn in_use_entry_at_offset_zero_is_free() {
        // Classic table: `0000000000 00000 n` for an unused number.
        let table = b"xref\n0 3\n0000000000 65535 f \n0000000009 00000 n \n0000000000 00000 n \ntrailer\n<< >>\n";
        let xref = Xref::parse(table, 0).expect("parse");
        assert_eq!(xref.get(1), Some(XrefEntry::InUse { offset: 9, gen: 0 }));
        assert_eq!(xref.get(2), Some(XrefEntry::Free { gen: 0 }));
        assert_eq!(xref.object_count(), 1);
        // Cross-reference stream (`/W [1 2 1]`): a type 1 row with field 2
        // at zero.
        let rows = [
            0, 0, 0, 0xFF, // 0: free
            1, 0, 42, 0, // 1: at offset 42
            1, 0, 0, 7, // 2: "in use" at offset 0, generation 7
        ];
        let stream = xref_stream("/Size 3", &rows);
        let xref = Xref::parse(&stream, 0).expect("parse");
        assert_eq!(xref.get(1), Some(XrefEntry::InUse { offset: 42, gen: 0 }));
        assert_eq!(xref.get(2), Some(XrefEntry::Free { gen: 7 }));
        assert_eq!(xref.object_count(), 1);
    }

    #[test]
    fn tolerates_one_byte_eol_in_entries() {
        let file = b"xref\n0 2\n0000000000 65535 f\n0000000009 00000 n\ntrailer\n<< >>\n";
        let xref = Xref::parse(file, 0).expect("xref");
        assert_eq!(xref.get(1), Some(XrefEntry::InUse { offset: 9, gen: 0 }));
    }

    #[test]
    fn newest_entry_wins_along_prev_chain() {
        let old = "xref\n0 2\n0000000000 65535 f \n0000000100 00000 n \ntrailer\n<< /Size 2 >>\n";
        let new =
            "xref\n1 2\n0000000200 00000 n \n0000000300 00000 n \ntrailer\n<< /Size 3 /Prev 0 >>\n";
        let file = format!("{old}{new}");
        let xref = Xref::parse(file.as_bytes(), old.len()).expect("xref");
        assert_eq!(xref.get(0), Some(XrefEntry::Free { gen: 65535 }));
        assert_eq!(
            xref.get(1),
            Some(XrefEntry::InUse {
                offset: 200,
                gen: 0
            })
        );
        assert_eq!(
            xref.get(2),
            Some(XrefEntry::InUse {
                offset: 300,
                gen: 0
            })
        );
        assert_eq!(
            xref.trailer()
                .get(&Name::new("Size"))
                .and_then(Object::as_i64),
            Some(3)
        );
    }

    #[test]
    fn prev_cycle_between_two_sections_is_an_error() {
        // Fixed-width offsets so that each section's length is known up front.
        let section = |prev: usize| format!("xref\n0 0\ntrailer\n<< /Prev {prev:010} >>\n");
        let len = section(0).len();
        let file = section(len) + &section(0);
        assert_eq!(
            Xref::parse(file.as_bytes(), 0),
            Err(Error::XrefLoop { offset: 0 })
        );
    }

    #[test]
    fn hostile_offsets_are_errors() {
        assert!(is_bad_xref(Xref::parse(SIMPLE, SIMPLE.len())));
        assert!(is_bad_xref(Xref::parse(SIMPLE, usize::MAX)));
        // Lands in the middle of the table.
        assert!(is_bad_xref(Xref::parse(SIMPLE, 5)));
        let prev_beyond_eof = b"xref\n0 0\ntrailer\n<< /Prev 99999999 >>\n";
        assert!(is_bad_xref(Xref::parse(prev_beyond_eof, 0)));
        let negative_prev = b"xref\n0 0\ntrailer\n<< /Prev -5 >>\n";
        assert!(is_bad_xref(Xref::parse(negative_prev, 0)));
        let negative_offset = b"xref\n0 1\n-000000001 00000 n \ntrailer\n<< >>\n";
        assert!(is_bad_xref(Xref::parse(negative_offset, 0)));
    }

    #[test]
    fn absurd_size_and_counts_neither_allocate_nor_hang() {
        let huge_size = b"xref\n0 1\n0000000000 65535 f \ntrailer\n<< /Size 99999999999999 >>\n";
        assert_eq!(
            Xref::parse(huge_size, 0).expect("xref").entries().count(),
            1
        );
        let negative_size = b"xref\n0 1\n0000000000 65535 f \ntrailer\n<< /Size -3 >>\n";
        assert!(Xref::parse(negative_size, 0).is_ok());
        let huge_count = b"xref\n0 4000000000\n0000000000 65535 f \ntrailer\n<< >>\n";
        assert!(is_bad_xref(Xref::parse(huge_count, 0)));
        let past_u32 =
            b"xref\n4294967295 2\n0000000000 65535 f \n0000000000 65535 f \ntrailer\n<< >>\n";
        assert!(is_bad_xref(Xref::parse(past_u32, 0)));
    }

    #[test]
    fn malformed_sections() {
        let bad_type = b"xref\n0 1\n0000000000 65535 x \ntrailer\n<< >>\n";
        assert!(is_bad_xref(Xref::parse(bad_type, 0)));
        // A free entry's generation is clamped (see
        // `free_entry_generation_out_of_range_is_clamped`); an in-use one is not.
        let bad_gen = b"xref\n0 1\n0000000009 70000 n \ntrailer\n<< >>\n";
        assert!(is_bad_xref(Xref::parse(bad_gen, 0)));
        let trailer_not_dict = b"xref\n0 1\n0000000000 65535 f \ntrailer\n[1 2]\n";
        assert!(is_bad_xref(Xref::parse(trailer_not_dict, 0)));
        let no_trailer = b"xref\n0 1\n0000000000 65535 f \n";
        assert_eq!(Xref::parse(no_trailer, 0), Err(Error::UnexpectedEof));
        assert!(is_bad_xref(Xref::parse(b"<< /Size 1 >>", 0)));
    }

    // --- Cross-reference streams (7.5.8) ---

    #[test]
    fn stream_entries_of_every_type() {
        let rows =
            b"\x00\x00\x00\xff\x01\x00\x2a\x00\x02\x00\x07\x03\x05\x00\x00\x00\x01\x01\x00\x02";
        let file = xref_stream("/Size 5 /Root 1 0 R", rows);
        let xref = Xref::parse(&file, 0).expect("xref");
        assert_eq!(xref.get(0), Some(XrefEntry::Free { gen: 255 }));
        assert_eq!(xref.get(1), Some(XrefEntry::InUse { offset: 42, gen: 0 }));
        assert_eq!(
            xref.get(2),
            Some(XrefEntry::InStream {
                stream_num: 7,
                index: 3
            })
        );
        // Type 5 is unknown: the null object, not an entry.
        assert_eq!(xref.get(3), None);
        assert_eq!(
            xref.get(4),
            Some(XrefEntry::InUse {
                offset: 256,
                gen: 2
            })
        );
        assert_eq!(xref.object_count(), 3);
        assert!(xref.trailer().contains_key(&Name::new("Root")));
        assert_eq!(
            xref.trailer()
                .get(&Name::new("Type"))
                .and_then(Object::as_name),
            Some(&Name::new("XRef"))
        );
    }

    #[test]
    fn stream_index_and_defaulted_type_field() {
        let mut out =
            b"9 0 obj\n<< /Type /XRef /Size 30 /W [0 2 1] /Index [5 2 20 1] /Length 9 >>\nstream\n"
                .to_vec();
        out.extend_from_slice(b"\x00\x10\x00\x00\x20\x01\x01\x00\x00");
        out.extend_from_slice(b"\nendstream\nendobj\n");
        let xref = Xref::parse(&out, 0).expect("xref");
        assert_eq!(xref.get(5), Some(XrefEntry::InUse { offset: 16, gen: 0 }));
        assert_eq!(xref.get(6), Some(XrefEntry::InUse { offset: 32, gen: 1 }));
        assert_eq!(
            xref.get(20),
            Some(XrefEntry::InUse {
                offset: 256,
                gen: 0
            })
        );
        assert_eq!(xref.entries().count(), 3);
    }

    #[test]
    fn stream_prev_chain_mixes_with_tables() {
        // Old classic table at 0, newer xref stream with /Prev 0.
        let old = b"xref\n0 2\n0000000000 65535 f \n0000000100 00000 n \ntrailer\n<< /Size 2 >>\n";
        let rows = b"\x01\x00\xc8\x00\x01\x01\x2c\x00";
        let mut file = old.to_vec();
        let start = file.len();
        file.extend_from_slice(&xref_stream("/Size 3 /Index [1 2] /Prev 0", rows));
        let xref = Xref::parse(&file, start).expect("xref");
        assert_eq!(xref.get(0), Some(XrefEntry::Free { gen: 65535 }));
        assert_eq!(
            xref.get(1),
            Some(XrefEntry::InUse {
                offset: 200,
                gen: 0
            })
        );
        assert_eq!(
            xref.get(2),
            Some(XrefEntry::InUse {
                offset: 300,
                gen: 0
            })
        );
        // Loop through /Prev pointing at itself.
        let looping = xref_stream("/Size 1 /Prev 0", b"\x00\x00\x00\x00");
        assert_eq!(Xref::parse(&looping, 0), Err(Error::XrefLoop { offset: 0 }));
    }

    #[test]
    fn hybrid_xrefstm_fills_gaps_and_free_entries() {
        // The stream lists objects 2 (compressed) and 3; the table lists 1
        // and marks 2 free to hide it from old readers.
        let rows = b"\x02\x00\x07\x00\x01\x01\x00\x00";
        let stream = xref_stream("/Size 4 /Index [2 2]", rows);
        let mut file = stream.clone();
        let table_at = file.len();
        file.extend_from_slice(
            b"xref\n0 3\n0000000000 65535 f \n0000000050 00000 n \n0000000000 00001 f \ntrailer\n<< /Size 4 /XRefStm 0 /Root 1 0 R >>\n",
        );
        let xref = Xref::parse(&file, table_at).expect("xref");
        assert_eq!(xref.get(1), Some(XrefEntry::InUse { offset: 50, gen: 0 }));
        assert_eq!(
            xref.get(2),
            Some(XrefEntry::InStream {
                stream_num: 7,
                index: 0
            })
        );
        assert_eq!(
            xref.get(3),
            Some(XrefEntry::InUse {
                offset: 256,
                gen: 0
            })
        );
        // The trailer is the table's, not the stream's.
        assert!(xref.trailer().contains_key(&Name::new("XRefStm")));
        assert!(!xref.trailer().contains_key(&Name::new("W")));
        // /XRefStm pointing back to the table itself: a loop.
        let self_ref = b"xref\n0 0\ntrailer\n<< /XRefStm 0 >>\n";
        assert_eq!(Xref::parse(self_ref, 0), Err(Error::XrefLoop { offset: 0 }));
    }

    #[test]
    fn hostile_stream_dictionaries() {
        let rows = b"\x01\x00\x10\x00";
        // Absurd field widths.
        for w in [
            "[1 99999999 1]",
            "[1 2 -1]",
            "[0 0 0]",
            "[1 2]",
            "[9 0 0]",
            "[1 /a 1]",
        ] {
            let mut out = format!("9 0 obj\n<< /Type /XRef /Size 2 /W {w} /Length 4 >>\nstream\n")
                .into_bytes();
            out.extend_from_slice(rows);
            out.extend_from_slice(b"\nendstream\nendobj\n");
            assert!(is_bad_xref(Xref::parse(&out, 0)), "/W {w}");
        }
        let no_w = b"9 0 obj\n<< /Type /XRef /Size 2 /Length 0 >>\nstream\n\nendstream\nendobj\n";
        assert!(is_bad_xref(Xref::parse(no_w, 0)));
        // Enormous /Size and /Index counts: only the rows present are read.
        let huge = xref_stream("/Size 99999999999999", rows);
        assert_eq!(Xref::parse(&huge, 0).expect("xref").entries().count(), 1);
        let huge_index = xref_stream("/Size 2 /Index [0 4000000000000]", rows);
        assert_eq!(
            Xref::parse(&huge_index, 0).expect("xref").entries().count(),
            1
        );
        let past_u32 = xref_stream(
            "/Size 2 /Index [4294967295 2]",
            b"\x01\x00\x10\x00\x01\x00\x20\x00",
        );
        assert!(is_bad_xref(Xref::parse(&past_u32, 0)));
        let odd_index = xref_stream("/Size 2 /Index [0 1 2]", rows);
        assert!(is_bad_xref(Xref::parse(&odd_index, 0)));
        let negative_index = xref_stream("/Size 2 /Index [-1 1]", rows);
        assert!(is_bad_xref(Xref::parse(&negative_index, 0)));
        // Row cut short: the partial row is dropped, not misread.
        let short = xref_stream("/Size 2", b"\x01\x00\x10\x00\x01\x00");
        assert_eq!(Xref::parse(&short, 0).expect("xref").entries().count(), 1);
        // Not a stream, or not an XRef stream.
        let not_stream = b"9 0 obj\n<< /Type /XRef >>\nendobj\n";
        assert!(is_bad_xref(Xref::parse(not_stream, 0)));
        let wrong_type = b"9 0 obj\n<< /Type /ObjStm /W [1 1 1] /Length 3 >>\nstream\n\x01\x02\x03\nendstream\nendobj\n";
        assert!(is_bad_xref(Xref::parse(wrong_type, 0)));
        // Widths of 8 bytes are the maximum and work.
        let wide = b"9 0 obj\n<< /Type /XRef /Size 2 /Index [1 1] /W [1 8 8] /Length 17 >>\nstream\n\x01\x00\x00\x00\x00\x00\x00\x00\x2a\x00\x00\x00\x00\x00\x00\x00\x01\nendstream\nendobj\n";
        assert_eq!(
            Xref::parse(wide, 0).expect("xref").get(1),
            Some(XrefEntry::InUse { offset: 42, gen: 1 })
        );
        // Object 0 listed in use, in a stream or a table: free (7.5.4).
        let zero = b"9 0 obj\n<< /Type /XRef /Size 1 /W [1 1 1] /Length 3 >>\nstream\n\x01\x2a\x00\nendstream\nendobj\n";
        assert_eq!(
            Xref::parse(zero, 0).expect("xref").get(0),
            Some(XrefEntry::Free { gen: u16::MAX })
        );
        let zero_table =
            b"xref\n0 2\n0000000015 00000 n \n0000000030 00000 n \ntrailer\n<< /Size 2 >>\n";
        let xref = Xref::parse(zero_table, 0).expect("xref");
        assert_eq!(xref.get(0), Some(XrefEntry::Free { gen: u16::MAX }));
        assert_eq!(xref.get(1), Some(XrefEntry::InUse { offset: 30, gen: 0 }));
        // Generation beyond u16.
        let big_gen = b"9 0 obj\n<< /Type /XRef /Size 1 /W [1 1 3] /Length 5 >>\nstream\n\x01\x10\x01\x00\x00\nendstream\nendobj\n";
        assert!(is_bad_xref(Xref::parse(big_gen, 0)));
    }

    #[test]
    fn stream_that_inflates_past_the_limit() {
        use std::io::Write;
        let zeros = vec![0u8; 1 << 20];
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        enc.write_all(&zeros).unwrap();
        let bomb = enc.finish().unwrap();
        let mut out = format!(
            "9 0 obj\n<< /Type /XRef /Size 1 /W [1 2 1] /Filter /FlateDecode /Length {} >>\nstream\n",
            bomb.len()
        )
        .into_bytes();
        out.extend_from_slice(&bomb);
        out.extend_from_slice(b"\nendstream\nendobj\n");
        let limits = DecodeLimits { max_output: 4096 };
        assert!(matches!(
            Xref::parse_with_limits(&out, 0, limits),
            Err(Error::LimitExceeded { limit: 4096, .. })
        ));
        // Under the default limit the same file is fine (1 MiB of rows).
        assert!(Xref::parse(&out, 0).is_ok());
    }

    #[test]
    fn kind_is_that_of_the_newest_section() {
        let kind = |file: &[u8], at: usize| Xref::parse(file, at).expect("xref").kind();
        assert_eq!(kind(SIMPLE, 0), SectionKind::Table);
        let stream = xref_stream("/Size 1", b"\x01\x00\x10\x00");
        assert_eq!(kind(&stream, 0), SectionKind::Stream);
        // Table whose trailer names the stream at offset 0.
        let mut hybrid = stream.clone();
        let table_at = hybrid.len();
        hybrid.extend_from_slice(
            b"xref\n0 1\n0000000000 65535 f \ntrailer\n<< /Size 2 /XRefStm 0 >>\n",
        );
        assert_eq!(kind(&hybrid, table_at), SectionKind::Hybrid);
        // Stream appended to a file written with a table: the newest decides.
        let mut update = b"xref\n0 1\n0000000000 65535 f \ntrailer\n<< /Size 1 >>\n".to_vec();
        let stream_at = update.len();
        update.extend_from_slice(&xref_stream("/Size 2 /Prev 0", b"\x01\x00\x10\x00"));
        assert_eq!(kind(&update, stream_at), SectionKind::Stream);
        // And the reverse: table appended to a file written with a stream.
        let mut update = stream.clone();
        let table_at = update.len();
        update
            .extend_from_slice(b"xref\n0 1\n0000000000 65535 f \ntrailer\n<< /Size 2 /Prev 0 >>\n");
        assert_eq!(kind(&update, table_at), SectionKind::Table);
    }
}
