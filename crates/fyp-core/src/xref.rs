//! Classic cross-reference tables (ISO 32000-2, 7.5.4), file trailers
//! (ISO 32000-2, 7.5.5) and the `/Prev` chain of incremental updates
//! (ISO 32000-2, 7.5.6).
//!
//! Not handled yet (milestone 0.1, step 2): cross-reference streams
//! (ISO 32000-2, 7.5.8). A `startxref` pointing at one yields
//! [`Error::Unsupported`]. The `/XRefStm` entry of hybrid-reference files
//! (ISO 32000-2, 7.5.8.4) is ignored for now, so objects listed only in that
//! stream are missing from the table.

use std::collections::{BTreeMap, BTreeSet};

use crate::lexer::{Lexer, Token};
use crate::object::{Dict, Name, Object};
use crate::parser::Parser;
use crate::{Error, Result};

/// One cross-reference entry (ISO 32000-2, 7.5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XrefEntry {
    /// `f` entry: the object number is free.
    Free {
        /// Generation number for the next reuse of this object number.
        gen: u16,
    },
    /// `n` entry: the object is stored in the file.
    InUse {
        /// Byte offset of its `n g obj` header.
        offset: usize,
        /// Generation number.
        gen: u16,
    },
}

/// Cross-reference table merged across incremental updates, with the
/// newest trailer.
#[derive(Debug, Clone, PartialEq)]
pub struct Xref {
    entries: BTreeMap<u32, XrefEntry>,
    trailer: Dict,
}

impl Xref {
    /// Read the section at `startxref`, then each older section reached
    /// through `/Prev`. For every object number the newest entry wins
    /// (ISO 32000-2, 7.5.6).
    ///
    /// Nothing is sized from values found in the file (`/Size`, subsection
    /// counts): only entries actually present are stored.
    pub fn parse(input: &[u8], startxref: usize) -> Result<Xref> {
        let (mut entries, trailer) = parse_section(input, startxref)?;
        let mut visited = BTreeSet::from([startxref]);
        let mut prev = prev_offset(&trailer, startxref)?;
        while let Some(offset) = prev {
            if !visited.insert(offset) {
                return Err(Error::XrefLoop { offset });
            }
            let (older, older_trailer) = parse_section(input, offset)?;
            for (num, entry) in older {
                // Sections are read newest first: keep what is already there.
                entries.entry(num).or_insert(entry);
            }
            prev = prev_offset(&older_trailer, offset)?;
        }
        Ok(Xref { entries, trailer })
    }

    /// Trailer of the newest section (ISO 32000-2, 7.5.5).
    pub fn trailer(&self) -> &Dict {
        &self.trailer
    }

    /// Entry for object number `num`, if any section lists it.
    pub fn get(&self, num: u32) -> Option<XrefEntry> {
        self.entries.get(&num).copied()
    }

    /// All entries, free ones included, by increasing object number.
    pub fn entries(&self) -> impl Iterator<Item = (u32, XrefEntry)> + '_ {
        self.entries.iter().map(|(&num, &entry)| (num, entry))
    }

    /// Number of in-use (`n`) entries, i.e. objects stored in the file.
    pub fn object_count(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| matches!(entry, XrefEntry::InUse { .. }))
            .count()
    }
}

/// Parse one `xref` ... `trailer << >>` section starting at `offset`.
fn parse_section(input: &[u8], offset: usize) -> Result<(BTreeMap<u32, XrefEntry>, Dict)> {
    if offset >= input.len() {
        return Err(bad(offset, "offset beyond end of file"));
    }
    let mut lexer = Lexer::at(input, offset);
    match next(&mut lexer)? {
        (Token::Keyword(k), _) if k == b"xref" => {}
        (_, at) => return Err(not_a_table(input, offset, at)),
    }
    let mut entries = BTreeMap::new();
    loop {
        match next(&mut lexer)? {
            (Token::Keyword(k), _) if k == b"trailer" => break,
            (Token::Integer(first), first_at) => {
                let first = u32::try_from(first)
                    .map_err(|_| bad(first_at, "subsection start out of range"))?;
                let count = match next(&mut lexer)? {
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
                    entries.insert(num, parse_entry(&mut lexer)?);
                }
            }
            (Token::Eof, _) => return Err(Error::UnexpectedEof),
            (_, at) => return Err(bad(at, "expected subsection header or `trailer`")),
        }
    }
    let trailer_at = lexer.pos();
    match Parser::at(input, trailer_at).parse_object()? {
        Object::Dict(trailer) => Ok((entries, trailer)),
        _ => Err(bad(trailer_at, "trailer is not a dictionary")),
    }
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
    let gen = u16::try_from(gen).map_err(|_| bad(at, "generation out of range"))?;
    match kind {
        Token::Keyword(k) if k == b"n" => {
            let offset = usize::try_from(field).map_err(|_| bad(at, "negative object offset"))?;
            Ok(XrefEntry::InUse { offset, gen })
        }
        Token::Keyword(k) if k == b"f" => Ok(XrefEntry::Free { gen }),
        _ => Err(bad(at, "entry type must be `n` or `f`")),
    }
}

/// Next token with its start offset.
fn next(lexer: &mut Lexer<'_>) -> Result<(Token, usize)> {
    lexer.skip_whitespace_and_comments();
    let at = lexer.pos();
    Ok((lexer.next_token()?, at))
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

/// The offset did not lead to `xref`. A cross-reference stream there is a
/// valid file we cannot read yet, not a broken one: say so.
fn not_a_table(input: &[u8], offset: usize, at: usize) -> Error {
    let is_xref_stream = matches!(
        Parser::at(input, offset).parse_indirect(),
        Ok((_, Object::Stream { dict, .. }))
            if dict.get(&Name::new("Type")).and_then(Object::as_name) == Some(&Name::new("XRef"))
    );
    if is_xref_stream {
        Error::Unsupported {
            feature: "cross-reference streams (ISO 32000-2, 7.5.8)",
        }
    } else {
        bad(at, "expected `xref`")
    }
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
        let bad_gen = b"xref\n0 1\n0000000000 70000 f \ntrailer\n<< >>\n";
        assert!(is_bad_xref(Xref::parse(bad_gen, 0)));
        let trailer_not_dict = b"xref\n0 1\n0000000000 65535 f \ntrailer\n[1 2]\n";
        assert!(is_bad_xref(Xref::parse(trailer_not_dict, 0)));
        let no_trailer = b"xref\n0 1\n0000000000 65535 f \n";
        assert_eq!(Xref::parse(no_trailer, 0), Err(Error::UnexpectedEof));
    }

    #[test]
    fn cross_reference_stream_is_unsupported_not_broken() {
        let file = b"1 0 obj\n<< /Type /XRef /Size 1 /W [1 1 1] /Length 3 >>\nstream\n\x00\x00\x00\nendstream\nendobj\n";
        assert!(matches!(
            Xref::parse(file, 0),
            Err(Error::Unsupported { .. })
        ));
    }
}
