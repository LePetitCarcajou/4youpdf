//! Cross-reference reconstruction by scanning the file for `n g obj`
//! headers. This is what lets 4YouPDF open files other readers give up on.
//!
//! It is a fallback, never the normal path: [`crate::document::Document`]
//! reads the declared table first and comes here when that table is
//! missing, unreadable, loops, or does not match the objects actually in
//! the file. A document rebuilt this way says so
//! ([`crate::document::Document::reconstructed`]).
//!
//! Rules:
//! - Of several definitions of one object number, the last in the file
//!   wins: incremental updates append (ISO 32000-2, 7.5.6).
//! - Object streams (7.5.7) found by the scan are opened and their objects
//!   indexed. They take part in the "last wins" rule at the position of the
//!   stream that holds them.
//! - The trailer is rebuilt from every `trailer` dictionary and every
//!   cross-reference stream dictionary found, later ones overriding earlier
//!   ones key by key. If the resulting `/Root` points nowhere, the last
//!   `/Type /Catalog` object serves.
//! - Work is linear in the file size. A candidate object is parsed on a
//!   slice cut at its own `endobj`, so a runaway parse never reads past it,
//!   and a global budget of parsed bytes stops the scan on files built so
//!   that every candidate is expensive. An object stream is read again on
//!   the slice it was accepted on, for the same reason. Each of its objects
//!   is looked at on the data cut at the next one, and one per stream, at
//!   most, a second time on all the data.
//!
//! Candidates that do not parse as an object are dropped: `n g obj` inside
//! a string of an accepted object is never examined (the scan resumes after
//! that object), inside stream data it is skipped along with the data, and
//! on a `%` comment line it is ignored.

use std::collections::{BTreeMap, BTreeSet};

use crate::document::ObjectStream;
use crate::filters::{self, DecodeLimits};
use crate::lexer::{is_delimiter, is_whitespace};
use crate::object::{Dict, Name, ObjRef, Object};
use crate::parser::{find, Parser};
use crate::xref::{Xref, XrefEntry};

/// How many bytes of the file a candidate may make the parser read, in
/// total, relative to the file size. Sound files stay near 1: accepted
/// objects partition the file. The margin absorbs a few false starts.
const BUDGET_FACTOR: usize = 8;

/// Floor for the budget, so that tiny files still get a fair scan.
const BUDGET_FLOOR: usize = 1 << 20;

/// How far back from a candidate header to look for the `%` of a comment
/// on the same line. Longer lines are treated as not being comments.
const COMMENT_LOOKBACK: usize = 1024;

/// Rebuild a cross-reference table from the objects present in `input`.
/// `None` when no object at all was found: there is nothing to open.
pub fn reconstruct(input: &[u8], limits: DecodeLimits) -> Option<Xref> {
    let mut scan = Scan {
        input,
        budget: input.len().saturating_mul(BUDGET_FACTOR).max(BUDGET_FLOOR),
        next_obj: find_from(input, 0, b"obj"),
        next_trailer: find_from(input, 0, b"trailer"),
        next_startxref: find_from(input, 0, b"startxref"),
        found: BTreeMap::new(),
        object_streams: Vec::new(),
        catalog: None,
        trailers: Vec::new(),
    };
    scan.run();
    if scan.found.is_empty() {
        return None;
    }
    scan.index_object_streams(limits);
    let trailer = scan.trailer();
    let entries = scan
        .found
        .into_iter()
        .map(|(num, f)| (num, f.entry))
        .collect();
    Some(Xref::reconstructed(entries, trailer))
}

/// Where an object was last defined, and how to reach it.
struct Found {
    entry: XrefEntry,
    /// Byte position of the definition, for the "last wins" rule. For an
    /// object inside an object stream, the position of that stream.
    at: usize,
}

struct Scan<'a> {
    input: &'a [u8],
    budget: usize,
    /// Positions of the next `obj`, `trailer` and `startxref` keywords at
    /// or after the scan position. Cached so that a keyword absent from the
    /// rest of the file is searched for once, not once per candidate.
    next_obj: Option<usize>,
    next_trailer: Option<usize>,
    next_startxref: Option<usize>,
    found: BTreeMap<u32, Found>,
    /// `/Type /ObjStm` objects, in file order: `(object number, offset,
    /// end of the slice the object was parsed on)`.
    object_streams: Vec<(u32, usize, usize)>,
    /// Last `/Type /Catalog` object seen: `(position, reference)`.
    catalog: Option<(usize, ObjRef)>,
    /// Trailer-like dictionaries in file order, already stripped of the
    /// keys that make no sense once the table is rebuilt.
    trailers: Vec<Dict>,
}

impl Scan<'_> {
    /// Walk the file from start to end, alternating between the next `obj`
    /// and the next `trailer` keyword, whichever comes first.
    fn run(&mut self) {
        let mut pos = 0;
        while self.budget > 0 {
            self.refresh(pos);
            pos = match (self.next_obj, self.next_trailer) {
                (None, None) => break,
                (Some(o), None) => self.candidate_object(o),
                (None, Some(t)) => self.candidate_trailer(t),
                (Some(o), Some(t)) if o < t => self.candidate_object(o),
                (Some(_), Some(t)) => self.candidate_trailer(t),
            };
        }
    }

    /// Bring the cached keyword positions to `pos` or after.
    fn refresh(&mut self, pos: usize) {
        let input = self.input;
        let refresh = |cache: &mut Option<usize>, needle: &[u8]| {
            if cache.is_some_and(|at| at < pos) {
                *cache = find_from(input, pos, needle);
            }
        };
        refresh(&mut self.next_obj, b"obj");
        refresh(&mut self.next_trailer, b"trailer");
        refresh(&mut self.next_startxref, b"startxref");
    }

    /// `kw` is the position of an `obj` keyword. Accept the object if the
    /// bytes before form a header `n g` and the bytes after parse as an
    /// object. Returns where the scan resumes.
    fn candidate_object(&mut self, kw: usize) -> usize {
        let input = self.input;
        let after = kw + 3;
        if !boundary_after(input, after) {
            return after;
        }
        let Some((start, r)) = header_before(input, kw) else {
            return after;
        };
        // Object 0 is the head of the free list, never an object (7.5.4):
        // `0 0 obj` is junk (corpus: qpdf `obj0.pdf`, `issue-99.pdf`).
        if r.num == 0 || on_comment_line(input, start) {
            return after;
        }
        // Tight cut first: `endobj` if one comes before the next header,
        // else that header. Cheap to find, handles a missing `endobj`, and
        // keeps hostile files linear. If that parse fails, cut at the real
        // `endobj` instead: this handles `n g obj` inside a string or inside
        // stream data, at the price of a search that may run far.
        let next_header = next_header_start(input, after);
        let tight_end =
            find_between(input, after, next_header, b"endobj").map_or(next_header, |e| e + 6);
        if let Some(resume) = self.try_parse(start, r, tight_end) {
            return resume;
        }
        let Some(loose_end) = self.object_end(after) else {
            return input.len();
        };
        if loose_end > tight_end {
            if let Some(resume) = self.try_parse(start, r, loose_end) {
                return resume;
            }
        }
        after
    }

    /// Debit `cost` bytes of work from the budget. `false` when the budget
    /// does not cover them: it is then spent, which stops the whole scan.
    fn spend(&mut self, cost: usize) -> bool {
        if cost > self.budget {
            self.budget = 0;
            return false;
        }
        self.budget -= cost;
        true
    }

    /// End of the object whose header ends at `after`: just past its
    /// `endobj`, skipping stream data first when a `stream` keyword comes
    /// before that `endobj`. The end of input when the file is cut short.
    /// `None` when the budget ran out. Only the search for `stream` is
    /// debited here. The other searches are paid for by a parse: when they
    /// run past the tight cut, the loose parse that follows is debited for
    /// at least the bytes they went through; when they do not, the tight
    /// parse already was.
    fn object_end(&mut self, after: usize) -> Option<usize> {
        let input = self.input;
        let len = input.len();
        let endobj = find_from(input, after, b"endobj");
        let limit = endobj.unwrap_or(len);
        let (stream, searched) = stream_keyword_between(input, after, limit);
        if !self.spend(searched) {
            return None;
        }
        if let Some(s) = stream {
            let Some(es) = find_from(input, s + 6, b"endstream") else {
                return Some(len);
            };
            return Some(find_from(input, es + 9, b"endobj").map_or(len, |e| e + 6));
        }
        Some(endobj.map_or(len, |e| e + 6))
    }

    /// Parse the object whose header starts at `start` on the input cut at
    /// `end`. On success, record it and return where the parser stopped.
    fn try_parse(&mut self, start: usize, r: ObjRef, end: usize) -> Option<usize> {
        let cost = end.saturating_sub(start);
        if !self.spend(cost) {
            return Some(self.input.len());
        }
        examined(cost);
        let slice = self.input.get(..end).unwrap_or(self.input);
        let mut parser = Parser::at(slice, start);
        match parser.parse_indirect() {
            Ok((found, obj)) if found == r => {
                self.record(r, start, end, &obj);
                Some(parser.pos().max(start + 1))
            }
            _ => None,
        }
    }

    /// Record the object `r` found at `at`, parsed on the input cut at `end`.
    fn record(&mut self, r: ObjRef, at: usize, end: usize, obj: &Object) {
        self.found.insert(
            r.num,
            Found {
                entry: XrefEntry::InUse {
                    offset: at,
                    gen: r.gen,
                },
                at,
            },
        );
        let Some(dict) = obj.as_dict() else {
            return;
        };
        let is_stream = matches!(obj, Object::Stream { .. });
        match dict
            .get(&Name::new("Type"))
            .and_then(Object::as_name)
            .map(|n| n.0.as_slice())
        {
            Some(b"ObjStm") if is_stream => self.object_streams.push((r.num, at, end)),
            Some(b"XRef") if is_stream => self.trailers.push(xref_stream_trailer(dict)),
            Some(b"Catalog") => self.catalog = Some((at, r)),
            _ => {}
        }
    }

    /// `kw` is the position of a `trailer` keyword between objects. Parse
    /// the dictionary that follows.
    fn candidate_trailer(&mut self, kw: usize) -> usize {
        let input = self.input;
        let after = kw + 7;
        if !boundary_before(input, kw) || !boundary_after(input, after) {
            return after;
        }
        // A trailer dictionary ends before `startxref`, the next `obj` or
        // the next `trailer`. This keyword is consumed: look for the next.
        self.next_trailer = find_from(input, after, b"trailer");
        self.refresh(after);
        let end = [self.next_startxref, self.next_trailer, self.next_obj]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(input.len());
        let cost = end.saturating_sub(kw);
        if !self.spend(cost) {
            return input.len();
        }
        examined(cost);
        let slice = input.get(..end).unwrap_or(input);
        let mut parser = Parser::at(slice, after);
        match parser.parse_object() {
            Ok(Object::Dict(mut dict)) => {
                dict.remove(&Name::new("Prev"));
                dict.remove(&Name::new("XRefStm"));
                self.trailers.push(dict);
                parser.pos().max(after)
            }
            _ => after,
        }
    }

    /// Open every object stream found and index its objects, subject to the
    /// "last wins" rule. A stream that cannot be decoded or parsed is
    /// skipped: its objects are simply not found.
    fn index_object_streams(&mut self, limits: DecodeLimits) {
        let streams = std::mem::take(&mut self.object_streams);
        for (num, at, end) in streams {
            let Some(stream) = self.load_object_stream(num, at, end, limits) else {
                continue;
            };
            let has_catalog = find_counted(&stream.data, b"/Catalog").is_some();
            // Where the objects of the stream start, in order, and the
            // starts already looked at: the header may name one offset any
            // number of times, in any order.
            let mut starts: Vec<usize> = Vec::new();
            if has_catalog {
                starts.extend(stream.objects.iter().map(|&(_, offset)| offset));
                starts.sort_unstable();
                starts.dedup();
            }
            let mut probed = BTreeSet::new();
            let mut whole_data_left = true;
            for (index, &(inner, offset)) in stream.objects.iter().enumerate() {
                let Ok(index) = u32::try_from(index) else {
                    break;
                };
                let older = self.found.get(&inner).is_none_or(|f| f.at < at);
                if !older {
                    continue;
                }
                self.found.insert(
                    inner,
                    Found {
                        entry: XrefEntry::InStream {
                            stream_num: num,
                            index,
                        },
                        at,
                    },
                );
                if has_catalog
                    && self.catalog.is_none_or(|(pos, _)| pos < at)
                    && probed.insert(offset)
                    && is_catalog_at(&stream.data, &starts, offset, &mut whole_data_left)
                {
                    self.catalog = Some((at, ObjRef { num: inner, gen: 0 }));
                }
            }
        }
    }

    /// Re-read and decode the object stream at `at`, on the input cut at
    /// `end` as when the scan accepted it: on the whole input, a `/Length`
    /// aimed at a far `endstream` would make every object stream of a
    /// hostile file read to that point. Its `/N` and `/First` must be
    /// direct: with the table being rebuilt, references are not followed
    /// here.
    fn load_object_stream(
        &self,
        num: u32,
        at: usize,
        end: usize,
        limits: DecodeLimits,
    ) -> Option<ObjectStream> {
        let slice = self.input.get(..end).unwrap_or(self.input);
        examined(slice.len().saturating_sub(at));
        let (found, obj) = Parser::at(slice, at).parse_indirect().ok()?;
        if found.num != num {
            return None;
        }
        let Object::Stream { dict, data } = obj else {
            return None;
        };
        let int = |key: &str| {
            dict.get(&Name::new(key))
                .and_then(Object::as_i64)
                .and_then(|v| usize::try_from(v).ok())
        };
        let count = int("N")?;
        let first = int("First")?;
        let decoded = filters::decode_stream_with(&dict, &data, |o| Ok(o.clone()), limits).ok()?;
        ObjectStream::from_decoded(num, decoded, count, first).ok()
    }

    /// Merge the trailer dictionaries, validate `/Root`, set `/Size`.
    fn trailer(&self) -> Dict {
        let mut trailer = Dict::new();
        for dict in &self.trailers {
            for (key, value) in dict {
                trailer.insert(key.clone(), value.clone());
            }
        }
        let root = Name::new("Root");
        // A catalog written directly in the trailer is kept (corpus:
        // pdf.js `issue9105_other.pdf`); the writer makes it indirect.
        let root_ok = match trailer.get(&root) {
            Some(Object::Reference(r)) => self.found.contains_key(&r.num),
            Some(Object::Dict(_)) => true,
            _ => false,
        };
        if !root_ok {
            match self.catalog {
                Some((_, r)) => {
                    trailer.insert(root, Object::Reference(r));
                }
                None => {
                    trailer.remove(&root);
                }
            }
        }
        let size = self
            .found
            .keys()
            .next_back()
            .map_or(0, |&max| i64::from(max) + 1);
        trailer.insert(Name::new("Size"), Object::Integer(size));
        trailer
    }
}

/// Is the object at `offset` of the decoded object stream `data` a catalog?
/// It is parsed on the data cut at the next of `starts`, the sorted offsets
/// of the stream's objects: an object ends where the next one starts
/// (ISO 32000-2, 7.5.7), and parsed that way the objects of one stream cost
/// its size in total, whatever its header says.
///
/// A header that declares another object inside this one cuts it short.
/// The first object of a stream that does not parse on its cut is parsed
/// again on all the data, and `whole_data_left` is cleared: one such
/// reading per stream costs no more than the size of the stream, and
/// finds a catalog cut that way unless an object named before it in the
/// header fails on its own cut and takes the reading.
fn is_catalog_at(data: &[u8], starts: &[usize], offset: usize, whole_data_left: &mut bool) -> bool {
    let is_catalog = |obj: &Object| {
        matches!(
            obj,
            Object::Dict(d)
                if d.get(&Name::new("Type")).and_then(Object::as_name)
                    == Some(&Name::new("Catalog"))
        )
    };
    let next = starts.partition_point(|&start| start <= offset);
    let cut = starts.get(next).copied().unwrap_or(data.len());
    let slice = data.get(..cut).unwrap_or(data);
    examined(slice.len().saturating_sub(offset));
    match Parser::at(slice, offset).parse_object() {
        Ok(obj) => is_catalog(&obj),
        Err(_) if *whole_data_left && slice.len() < data.len() => {
            *whole_data_left = false;
            examined(data.len().saturating_sub(offset));
            Parser::at(data, offset)
                .parse_object()
                .is_ok_and(|obj| is_catalog(&obj))
        }
        Err(_) => false,
    }
}

/// The trailer keys a cross-reference stream dictionary carries
/// (ISO 32000-2, 7.5.8.2), without the stream's own entries.
fn xref_stream_trailer(dict: &Dict) -> Dict {
    ["Root", "Info", "ID", "Encrypt"]
        .into_iter()
        .map(Name::new)
        .filter_map(|key| dict.get(&key).map(|v| (key, v.clone())))
        .collect()
}

// ---------------------------------------------------------------------------
// Byte-level helpers
// ---------------------------------------------------------------------------

/// Count `bytes` looked at by the scan. Tests read the total to check that
/// the work stays linear in the file size, which a stopwatch cannot do the
/// same way on every machine.
#[cfg(test)]
fn examined(bytes: usize) {
    tests::EXAMINED.with(|total| total.set(total.get().saturating_add(bytes)));
}

#[cfg(not(test))]
fn examined(_bytes: usize) {}

/// Search `haystack`, counting the bytes the search went through.
fn find_counted(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let found = find(haystack, needle);
    examined(found.map_or(haystack.len(), |i| i + needle.len()));
    found
}

/// First occurrence of `needle` at or after `from`.
fn find_from(input: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    find_counted(input.get(from..)?, needle).map(|i| from + i)
}

/// First occurrence of `needle` in `from..to`.
fn find_between(input: &[u8], from: usize, to: usize, needle: &[u8]) -> Option<usize> {
    find_counted(input.get(from..to)?, needle).map(|i| from + i)
}

/// Is the byte at `at` (or the end of input) a token boundary?
fn boundary_after(input: &[u8], at: usize) -> bool {
    input
        .get(at)
        .is_none_or(|&b| is_whitespace(b) || is_delimiter(b))
}

/// Is the byte before `at` (or the start of input) a token boundary?
fn boundary_before(input: &[u8], at: usize) -> bool {
    at.checked_sub(1)
        .and_then(|i| input.get(i))
        .is_none_or(|&b| is_whitespace(b) || is_delimiter(b))
}

/// Read `n g` backwards from an `obj` keyword at `kw`. Returns the offset
/// of `n` and the reference. Both numbers must fit their types.
fn header_before(input: &[u8], kw: usize) -> Option<(usize, ObjRef)> {
    let byte = |i: usize| input.get(i).copied();
    let skip_back = |mut i: usize, pred: fn(u8) -> bool| -> usize {
        while i > 0 && byte(i - 1).is_some_and(pred) {
            i -= 1;
        }
        i
    };
    let gen_end = skip_back(kw, is_whitespace);
    if gen_end == kw {
        return None;
    }
    let gen_start = skip_back(gen_end, |b| b.is_ascii_digit());
    if gen_start == gen_end {
        return None;
    }
    let num_end = skip_back(gen_start, is_whitespace);
    if num_end == gen_start {
        return None;
    }
    let num_start = skip_back(num_end, |b| b.is_ascii_digit());
    if num_start == num_end || !boundary_before(input, num_start) {
        return None;
    }
    let num = u32::try_from(parse_digits(input.get(num_start..num_end)?)?).ok()?;
    let gen = u16::try_from(parse_digits(input.get(gen_start..gen_end)?)?).ok()?;
    Some((num_start, ObjRef { num, gen }))
}

/// Decimal value of ASCII digits, `None` on overflow.
fn parse_digits(digits: &[u8]) -> Option<u64> {
    digits.iter().try_fold(0u64, |acc, &d| {
        acc.checked_mul(10)?.checked_add(u64::from(d - b'0'))
    })
}

/// Is there a `%` between the start of the line and `at`? Lines longer
/// than [`COMMENT_LOOKBACK`] are assumed not to be comments.
fn on_comment_line(input: &[u8], at: usize) -> bool {
    let from = at.saturating_sub(COMMENT_LOOKBACK);
    let line = input.get(from..at).unwrap_or_default();
    examined(line.len());
    let line_start = line
        .iter()
        .rposition(|&b| b == b'\n' || b == b'\r')
        .map_or(0, |i| i + 1);
    line.get(line_start..).unwrap_or_default().contains(&b'%')
}

/// Start of the next `n g obj` header at or after `from`, or the end of
/// input. Only the shape is checked, not whether an object follows.
fn next_header_start(input: &[u8], from: usize) -> usize {
    let mut pos = from;
    while let Some(kw) = find_from(input, pos, b"obj") {
        if boundary_after(input, kw + 3) {
            if let Some((start, _)) = header_before(input, kw) {
                if start >= from {
                    return start;
                }
            }
        }
        pos = kw + 3;
    }
    input.len()
}

/// A `stream` keyword in `from..to`: preceded by whitespace or `>`,
/// followed by whitespace (ISO 32000-2, 7.3.8.1 wants CR LF or LF; spaces
/// before that EOL are tolerated like the parser does). Also returns how
/// many bytes the search went through, never more than `to - from`.
fn stream_keyword_between(input: &[u8], from: usize, to: usize) -> (Option<usize>, usize) {
    let mut pos = from;
    while let Some(s) = find_between(input, pos, to, b"stream") {
        let before_ok = s
            .checked_sub(1)
            .and_then(|i| input.get(i))
            .is_none_or(|&b| is_whitespace(b) || b == b'>');
        let after_ok = input.get(s + 6).is_some_and(|&b| is_whitespace(b));
        pos = s + 6;
        if before_ok && after_ok {
            return (Some(s), pos.saturating_sub(from));
        }
    }
    (None, to.saturating_sub(from))
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::xref::SectionKind;
    use std::cell::Cell;
    use std::io::Write;
    use std::time::{Duration, Instant};

    thread_local! {
        /// Bytes looked at by the scan on this thread, see [`examined`].
        pub(super) static EXAMINED: Cell<usize> = const { Cell::new(0) };
    }

    /// Run [`reconstruct`] on `input`; also return the bytes it looked at.
    fn reconstruct_counted(input: &[u8]) -> (Option<Xref>, usize) {
        EXAMINED.with(|total| total.set(0));
        let xref = reconstruct(input, DecodeLimits::default());
        (xref, EXAMINED.with(Cell::get))
    }

    /// Bytes looked at per byte of input under which work counts as linear.
    const LINEAR: usize = 64;

    const CATALOG: &str = "<< /Type /Catalog /Pages 2 0 R >>";
    const PAGES: &str = "<< /Type /Pages /Kids [3 0 R] /Count 1 >>";
    const PAGE: &str = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] >>";

    fn rebuild(input: &[u8]) -> Xref {
        reconstruct(input, DecodeLimits::default()).expect("objects found")
    }

    fn objects(input: &str) -> Vec<u8> {
        format!("%PDF-1.7\n{input}").into_bytes()
    }

    fn root_of(xref: &Xref) -> Option<ObjRef> {
        match xref.trailer().get(&Name::new("Root")) {
            Some(Object::Reference(r)) => Some(*r),
            _ => None,
        }
    }

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        enc.write_all(data).expect("compress");
        enc.finish().expect("finish")
    }

    #[test]
    fn header_shapes() {
        let input = b"1 0 obj";
        assert_eq!(
            header_before(input, 4),
            Some((0, ObjRef { num: 1, gen: 0 }))
        );
        let input = b">>12 3 obj";
        assert_eq!(
            header_before(input, 7),
            Some((2, ObjRef { num: 12, gen: 3 }))
        );
        assert_eq!(header_before(b"x1 0 obj", 5), None);
        assert_eq!(header_before(b"1 obj", 2), None);
        assert_eq!(header_before(b"1 0obj", 3), None);
        assert_eq!(header_before(b"obj", 0), None);
        assert_eq!(header_before(b"99999999999 0 obj", 14), None);
        assert_eq!(header_before(b"1 70000 obj", 8), None);
        assert!(boundary_after(b"obj<<", 3));
        assert!(boundary_after(b"obj", 3));
        assert!(!boundary_after(b"objx", 3));
        assert!(on_comment_line(b"% see 1 0 obj", 6));
        assert!(!on_comment_line(b"%\n1 0 obj", 2));
    }

    #[test]
    fn last_definition_wins_and_trailer_is_found() {
        let file = objects(&format!(
            "1 0 obj\n{CATALOG}\nendobj\n2 0 obj\n{PAGES}\nendobj\n3 0 obj\n<< /Type /Page /MediaBox [0 0 1 1] >>\nendobj\n\
             trailer\n<< /Size 4 /Root 1 0 R /Info 9 0 R /Prev 5 >>\n\
             3 0 obj\n{PAGE}\nendobj\ntrailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n0\n%%EOF\n"
        ));
        let xref = rebuild(&file);
        assert_eq!(xref.kind(), SectionKind::Reconstructed);
        assert_eq!(xref.object_count(), 3);
        let last_page = find(&file, PAGE.as_bytes()).unwrap() - "3 0 obj\n".len();
        assert_eq!(
            xref.get(3),
            Some(XrefEntry::InUse {
                offset: last_page,
                gen: 0
            })
        );
        assert_eq!(root_of(&xref), Some(ObjRef { num: 1, gen: 0 }));
        // Keys merge across trailers; /Prev is dropped; /Size is recomputed.
        assert!(xref.trailer().contains_key(&Name::new("Info")));
        assert!(!xref.trailer().contains_key(&Name::new("Prev")));
        assert_eq!(
            xref.trailer().get(&Name::new("Size")),
            Some(&Object::Integer(4))
        );
    }

    #[test]
    fn catalog_fallback_when_no_trailer_or_root_is_dangling() {
        let file = objects(&format!(
            "2 0 obj\n{PAGES}\nendobj\n1 0 obj\n{CATALOG}\nendobj\n3 0 obj\n{PAGE}\nendobj\n"
        ));
        assert_eq!(root_of(&rebuild(&file)), Some(ObjRef { num: 1, gen: 0 }));
        let dangling = objects(&format!(
            "1 0 obj\n{CATALOG}\nendobj\ntrailer\n<< /Root 42 0 R >>\n"
        ));
        assert_eq!(
            root_of(&rebuild(&dangling)),
            Some(ObjRef { num: 1, gen: 0 })
        );
        let none = objects("5 0 obj\n<< /A 1 >>\nendobj\n");
        let xref = rebuild(&none);
        assert_eq!(root_of(&xref), None);
        assert_eq!(xref.get(5), Some(XrefEntry::InUse { offset: 9, gen: 0 }));
    }

    #[test]
    fn missing_endobj_and_generations() {
        let file = objects("1 2 obj\n<< /A 1 >>\n2 0 obj\n[1 2 3]\nendobj\n3 0 obj\n(str)\n");
        let xref = rebuild(&file);
        assert_eq!(xref.get(1), Some(XrefEntry::InUse { offset: 9, gen: 2 }));
        assert!(matches!(xref.get(2), Some(XrefEntry::InUse { gen: 0, .. })));
        assert!(matches!(xref.get(3), Some(XrefEntry::InUse { gen: 0, .. })));
        assert_eq!(xref.object_count(), 3);
    }

    #[test]
    fn headers_inside_strings_streams_and_comments_are_not_objects() {
        let data = b"9 0 obj\n<< /A 1 >>\nendobj\n8 0 obj";
        let file = objects(&format!(
            "1 0 obj\n<< /Type /Catalog /T (see 7 0 obj here) /U <41> >>\nendobj\n\
             % 6 0 obj is a comment\n\
             4 0 obj\n<< /Length {} >>\nstream\n{}\nendstream\nendobj\n\
             5 0 obj\n<< /Length 999 >>\nstream\n{}\nendstream\nendobj\n",
            data.len(),
            String::from_utf8_lossy(data),
            String::from_utf8_lossy(data),
        ));
        let xref = rebuild(&file);
        let nums: Vec<u32> = xref.entries().map(|(n, _)| n).collect();
        assert_eq!(nums, vec![1, 4, 5], "{nums:?}");
        assert_eq!(xref.get(7), None);
        assert_eq!(xref.get(6), None);
        assert_eq!(xref.get(8), None);
        assert_eq!(xref.get(9), None);
    }

    #[test]
    fn object_streams_are_opened_and_indexed() {
        // Objects 1 and 2 live in object stream 4; the file's xref stream
        // dictionary carries /Root. Object 2 is redefined later at top level.
        let content = format!("1 0 2 {} \n{CATALOG}\n{PAGES}\n", CATALOG.len() + 1);
        let first = content.find('\n').unwrap() + 1;
        let z = zlib(content.as_bytes());
        let mut file = objects(&format!("3 0 obj\n{PAGE}\nendobj\n"));
        let objstm_at = file.len();
        file.extend_from_slice(
            format!(
                "4 0 obj\n<< /Type /ObjStm /N 2 /First {first} /Filter /FlateDecode /Length {} >>\nstream\n",
                z.len()
            )
            .as_bytes(),
        );
        file.extend_from_slice(&z);
        file.extend_from_slice(b"\nendstream\nendobj\n");
        file.extend_from_slice(
            b"5 0 obj\n<< /Type /XRef /Size 6 /W [1 2 1] /Root 1 0 R /Info 3 0 R /Length 4 >>\nstream\n\x00\x00\x00\x00\nendstream\nendobj\n",
        );
        let redefined_at = file.len();
        file.extend_from_slice(format!("2 0 obj\n{PAGES}\nendobj\n").as_bytes());
        let xref = rebuild(&file);
        assert_eq!(
            xref.get(1),
            Some(XrefEntry::InStream {
                stream_num: 4,
                index: 0
            })
        );
        assert_eq!(
            xref.get(2),
            Some(XrefEntry::InUse {
                offset: redefined_at,
                gen: 0
            })
        );
        assert_eq!(
            xref.get(4),
            Some(XrefEntry::InUse {
                offset: objstm_at,
                gen: 0
            })
        );
        assert_eq!(xref.object_count(), 5);
        assert_eq!(root_of(&xref), Some(ObjRef { num: 1, gen: 0 }));
        assert!(xref.trailer().contains_key(&Name::new("Info")));
        assert!(!xref.trailer().contains_key(&Name::new("W")));

        // Without the xref stream, /Root comes from the catalog inside the
        // object stream.
        let cut = file.get(..find(&file, b"5 0 obj").unwrap()).unwrap();
        assert_eq!(root_of(&rebuild(cut)), Some(ObjRef { num: 1, gen: 0 }));

        // A stream that inflates past the limit is skipped, not fatal.
        let tight = DecodeLimits { max_output: 8 };
        let xref = reconstruct(&file, tight).expect("objects");
        assert_eq!(xref.get(1), None);
        assert!(matches!(xref.get(2), Some(XrefEntry::InUse { .. })));
    }

    #[test]
    fn truncated_file_keeps_what_is_complete() {
        let file = objects(&format!(
            "1 0 obj\n{CATALOG}\nendobj\n2 0 obj\n{PAGES}\nendobj\n3 0 obj\n<< /Type /Page /Parent 2 0 R /Med"
        ));
        let xref = rebuild(&file);
        assert_eq!(xref.object_count(), 2);
        assert_eq!(xref.get(3), None);
        assert_eq!(root_of(&xref), Some(ObjRef { num: 1, gen: 0 }));
        // A stream cut before its `endstream` is not an object.
        let tail_only = objects("2 0 obj\n<< /Length 5 >>\nstream\nhel");
        assert!(reconstruct(&tail_only, DecodeLimits::default()).is_none());
    }

    #[test]
    fn nothing_to_find() {
        assert!(reconstruct(b"", DecodeLimits::default()).is_none());
        assert!(reconstruct(b"%PDF-1.7\nhello obj world", DecodeLimits::default()).is_none());
        assert!(reconstruct(b"1 0 obj", DecodeLimits::default()).is_none());
        assert!(reconstruct(b"1 0 obj >>", DecodeLimits::default()).is_none());
    }

    /// Multi-megabyte files without one valid object must fail fast, even
    /// when every candidate is designed to make the parser run far. Fast is
    /// measured in bytes looked at, the same on every machine, not in
    /// seconds: these files cost 3 to 63.2 times their size, a scan gone
    /// quadratic costs thousands of times.
    #[test]
    fn hostile_megabytes_fail_fast() {
        let mut garbage = Vec::with_capacity(3 << 20);
        let mut x: u32 = 12345;
        while garbage.len() < 3 << 20 {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            garbage.push((x >> 16) as u8);
        }
        let mut unterminated = Vec::with_capacity(3 << 20);
        while unterminated.len() < 3 << 20 {
            unterminated.extend_from_slice(b"1 0 obj << ");
        }
        unterminated.extend_from_slice(b"endobj");
        let mut distinct = Vec::with_capacity(3 << 20);
        let mut n = 1;
        while distinct.len() < 3 << 20 {
            distinct.extend_from_slice(format!("{n} 0 obj << ").as_bytes());
            n += 1;
        }
        distinct.extend_from_slice(b"endobj");
        let mut trailers = Vec::with_capacity(3 << 20);
        while trailers.len() < 3 << 20 {
            trailers.extend_from_slice(b"trailer << ");
        }
        // Every candidate fails the tight parse, then looks for a `stream`
        // keyword that is nowhere in the file.
        let mut empty = Vec::with_capacity(3 << 20);
        while empty.len() < 3 << 20 {
            empty.extend_from_slice(b"1 0 obj << endobj ");
        }
        for (what, file) in [
            ("garbage", garbage),
            ("unterminated", unterminated),
            ("distinct", distinct),
            ("trailers", trailers),
            ("empty", empty),
        ] {
            let started = Instant::now();
            let (result, examined) = reconstruct_counted(&file);
            let elapsed = started.elapsed();
            assert!(result.is_none(), "{what}: found objects");
            assert!(
                examined <= LINEAR * file.len(),
                "{what}: looked at {examined} bytes of {}",
                file.len()
            );
            // Only catches a hang: the bound above is what fails fast means.
            assert!(
                elapsed < Duration::from_secs(60),
                "{what}: took {elapsed:?}"
            );
        }
    }

    /// Every object stream declares a `/Length` that lands on the one
    /// `endstream` closing the file. Read again on the whole input, each
    /// would hold the rest of the file: megabytes copied thousands of times.
    #[test]
    fn object_streams_aimed_at_a_far_endstream_stay_linear() {
        const UNIT: usize = 112;
        const BODY: &str = "8888888 0 1 endstream endobj\n";
        let count = (3 << 20) / UNIT;
        let total = count * UNIT;
        let mut file = Vec::with_capacity(total + 32);
        for i in 0..count {
            let head = format!(
                "{:07} 0 obj << /Type /ObjStm /N 1 /First 10 /Length ",
                i + 1
            );
            let tail = format!(" >> stream\n{BODY}");
            let data_start = file.len() + UNIT - BODY.len();
            let width = UNIT - head.len() - tail.len();
            let length = format!("{:>width$}", total - data_start);
            file.extend_from_slice(head.as_bytes());
            file.extend_from_slice(length.as_bytes());
            file.extend_from_slice(tail.as_bytes());
        }
        assert_eq!(file.len(), total);
        file.extend_from_slice(b"\nendstream endobj\n");
        // The premise: on the whole input, the parser trusts that `/Length`.
        let (_, whole) = Parser::at(&file, 0).parse_indirect().unwrap();
        let Object::Stream { data, .. } = whole else {
            panic!("not a stream");
        };
        assert!(data.len() > total - UNIT, "{}", data.len());

        let (xref, examined) = reconstruct_counted(&file);
        let xref = xref.expect("objects found");
        assert_eq!(xref.object_count(), count + 1);
        let last = u32::try_from(count).unwrap();
        assert_eq!(
            xref.get(8_888_888),
            Some(XrefEntry::InStream {
                stream_num: last,
                index: 0
            })
        );
        assert!(
            examined <= LINEAR * file.len(),
            "looked at {examined} bytes of {}",
            file.len()
        );
    }

    /// The bytes the search for `stream` goes through leave the budget, and
    /// a budget that does not cover them stops the scan.
    #[test]
    fn the_stream_search_is_debited_from_the_budget() {
        fn scan(input: &[u8], budget: usize) -> Scan<'_> {
            Scan {
                input,
                budget,
                next_obj: None,
                next_trailer: None,
                next_startxref: None,
                found: BTreeMap::new(),
                object_streams: Vec::new(),
                catalog: None,
                trailers: Vec::new(),
            }
        }
        let input = b"1 0 obj << /A 1 >> endobj";
        let after = 7;
        let endobj = find(input, b"endobj").unwrap();
        let mut rich = scan(input, 100);
        assert_eq!(rich.object_end(after), Some(input.len()));
        assert_eq!(rich.budget, 100 - (endobj - after));
        let mut exact = scan(input, endobj - after);
        assert_eq!(exact.object_end(after), Some(input.len()));
        assert_eq!(exact.budget, 0);
        let mut poor = scan(input, endobj - after - 1);
        assert_eq!(poor.object_end(after), None);
        assert_eq!(poor.budget, 0);
        // The search stops at the keyword: what follows it is not debited.
        let input = b"1 0 obj << >> stream\nabc\nendstream\nendobj";
        let keyword_end = find(input, b"stream").unwrap() + 6;
        let mut stream = scan(input, 100);
        assert_eq!(stream.object_end(after), Some(input.len()));
        assert_eq!(stream.budget, 100 - (keyword_end - after));
        // Out of budget in that search, the whole scan stops: the tight
        // parse takes 17 bytes of 20, the search wants 4 of the 3 left.
        let file = b"1 0 obj << endobj ";
        let mut stopped = scan(file, 20);
        stopped.next_obj = find_from(file, 0, b"obj");
        stopped.run();
        assert_eq!(stopped.budget, 0);
    }

    /// One object stream holding `/Catalog` whose header makes every probe
    /// for a catalog expensive: all objects at one long string, or each at
    /// the next of as many nested strings, declared forwards or backwards,
    /// or each at a catalog that never ends. Parsed on the whole decoded
    /// data, each is read once per object.
    #[test]
    fn object_stream_probed_for_a_catalog_stays_linear() {
        const SIZE: usize = 64 << 10;
        type Offset = fn(usize) -> usize;
        let forms: [(&str, &[u8], Offset); 4] = [
            ("same offset", b"a", |_| 0),
            ("nested strings", b"(", |n| n),
            ("nested strings, descending", b"(", |n| SIZE / 4 - n),
            ("cut catalogs", b"<</Type/Catalog/A(", |n| {
                (n - 1) % 1024 * 18 + 1
            }),
        ];
        for (what, fill, offset) in forms {
            let mut header = String::new();
            let mut count = 0;
            while header.len() < SIZE / 2 {
                count += 1;
                header.push_str(&format!("{count} {} ", offset(count)));
            }
            let first = header.len();
            let mut data = header.into_bytes();
            data.push(b'(');
            let filled = data.len() + SIZE / 2;
            while data.len() < filled {
                data.extend_from_slice(fill);
            }
            data.extend_from_slice(b" /Catalog)");
            let mut file = format!(
                "100000000 0 obj\n<< /Type /ObjStm /N {count} /First {first} /Length {} >>\nstream\n",
                data.len()
            )
            .into_bytes();
            file.extend_from_slice(&data);
            file.extend_from_slice(b"\nendstream\nendobj\n");
            let (xref, examined) = reconstruct_counted(&file);
            let xref = xref.expect("objects found");
            assert_eq!(xref.object_count(), count + 1, "{what}");
            assert_eq!(root_of(&xref), None, "{what}");
            assert!(
                examined <= LINEAR * file.len(),
                "{what}: looked at {examined} bytes of {}",
                file.len()
            );
        }
    }

    /// A catalog inside which the header declares another object is cut
    /// short by it, and still found when it is the first object of its
    /// stream that fails on its cut. Each stream has its own second reading.
    #[test]
    fn catalog_in_an_object_stream_with_an_offset_inside_it() {
        let stream = |num: u32, header: &str, body: &str| {
            let content = format!("{header}\n{body}\n");
            format!(
                "{num} 0 obj\n<< /Type /ObjStm /N 2 /First {} /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
                header.len() + 1,
                content.len()
            )
        };
        let two = objects(&format!(
            "{}{}",
            stream(8, "5 0 6 3 ", "<< /A (/Catalog) >>"),
            stream(9, "1 0 2 5 ", CATALOG)
        ));
        assert_eq!(root_of(&rebuild(&two)), Some(ObjRef { num: 1, gen: 0 }));
        for header in ["1 0 2 5 ", "2 5 1 0 ", "1 0 2 31 "] {
            let content = format!("{header}\n{CATALOG}\n");
            let first = header.len() + 1;
            let file = objects(&format!(
                "4 0 obj\n<< /Type /ObjStm /N 2 /First {first} /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
                content.len()
            ));
            assert_eq!(
                root_of(&rebuild(&file)),
                Some(ObjRef { num: 1, gen: 0 }),
                "{header}"
            );
        }
    }

    /// A catalog is still told apart when another object of its stream is
    /// declared at the same offset, before or after it in the header.
    #[test]
    fn catalog_in_an_object_stream_with_a_repeated_offset() {
        for header in ["1 0 2 0 ", "2 0 1 0 "] {
            let content = format!("{header}\n{CATALOG}\n");
            let first = header.len() + 1;
            let file = objects(&format!(
                "4 0 obj\n<< /Type /ObjStm /N 2 /First {first} /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
                content.len()
            ));
            let xref = rebuild(&file);
            let first_named = if header.starts_with('1') { 1 } else { 2 };
            assert_eq!(
                root_of(&xref),
                Some(ObjRef {
                    num: first_named,
                    gen: 0
                }),
                "{header}"
            );
        }
    }
}
