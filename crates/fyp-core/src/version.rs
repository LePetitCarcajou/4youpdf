//! PDF header detection (`%PDF-x.y`) and quick file facts, without parsing
//! the whole document. Used by `fyp info` and as the first sanity check when
//! opening a file.

use crate::lexer::is_whitespace;
use crate::parser::find;
use crate::{Error, Result};

/// Version assumed when a file has no `%PDF-x.y` header at all but is
/// otherwise a PDF (see [`quick_info`]). 1.4 is what such files, produced
/// by old or careless tools, turn out to be in practice.
pub const ASSUMED_VERSION: PdfVersion = PdfVersion { major: 1, minor: 4 };

/// Junk before the header is tolerated up to this many bytes, like the
/// major readers do.
const HEADER_WINDOW: usize = 1024;

/// PDF version declared in the file header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PdfVersion {
    /// Major version (1 or 2).
    pub major: u8,
    /// Minor version.
    pub minor: u8,
}

impl std::fmt::Display for PdfVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// Facts obtainable by scanning the file, before any object is parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickInfo {
    /// Header version.
    pub version: PdfVersion,
    /// Byte offset of the header (ISO 32000-2 allows junk before `%PDF`;
    /// readers tolerate up to 1024 bytes). 0 when the header is missing.
    pub header_offset: usize,
    /// False when the file has no `%PDF-x.y` header and `version` is
    /// [`ASSUMED_VERSION`]. Such a file is not sound; callers that report
    /// on a file should say so.
    pub header_present: bool,
    /// Offset announced by the last `startxref`, if found.
    pub startxref: Option<usize>,
    /// Whether the file has an `/Encrypt` entry somewhere in its trailer area.
    pub looks_encrypted: bool,
    /// Whether an `%%EOF` marker was found.
    pub has_eof_marker: bool,
}

/// Locate and parse the `%PDF-x.y` header. Tolerance: `%PDF-1.` without a
/// minor digit, or `%PDF-1` alone, reads as minor 0 (corpus: pdf.js
/// `issue9105_other.pdf`).
pub fn detect_version(input: &[u8]) -> Result<(PdfVersion, usize)> {
    let window = input.get(..HEADER_WINDOW).unwrap_or(input);
    let off = find(window, b"%PDF-").ok_or(Error::BadHeader)?;
    let rest = input.get(off + 5..).unwrap_or_default();
    let major = rest.first().and_then(digit).ok_or(Error::BadHeader)?;
    let minor = match rest.get(1) {
        Some(b'.') => match rest.get(2) {
            Some(d) if d.is_ascii_digit() => d - b'0',
            Some(&b) if is_whitespace(b) => 0,
            None => 0,
            Some(_) => return Err(Error::BadHeader),
        },
        Some(&b) if is_whitespace(b) => 0,
        None => 0,
        Some(_) => return Err(Error::BadHeader),
    };
    Ok((PdfVersion { major, minor }, off))
}

/// A file with no header that is still worth trying: it starts with a
/// comment line, as `%PDF` files do (some writers emit only the binary
/// comment), and holds an object header (corpus: pdf.js `bug1606566.pdf`).
fn looks_like_headerless_pdf(input: &[u8]) -> bool {
    let window = input.get(..HEADER_WINDOW).unwrap_or(input);
    let first = window.iter().find(|&&b| !is_whitespace(b));
    first == Some(&b'%') && find(window, b"obj").is_some()
}

fn digit(b: &u8) -> Option<u8> {
    if b.is_ascii_digit() {
        Some(b - b'0')
    } else {
        None
    }
}

/// Scan a file for quick facts without building the object graph.
pub fn quick_info(input: &[u8]) -> Result<QuickInfo> {
    let (version, header_offset, header_present) = match detect_version(input) {
        Ok((version, offset)) => (version, offset, true),
        Err(Error::BadHeader) if looks_like_headerless_pdf(input) => (ASSUMED_VERSION, 0, false),
        Err(e) => return Err(e),
    };
    // Look at the tail of the file for startxref / trailer / %%EOF.
    let tail_start = input.len().saturating_sub(2048);
    let tail = input.get(tail_start..).unwrap_or_default();
    let startxref = rfind(tail, b"startxref").and_then(|i| {
        let after = tail.get(i + b"startxref".len()..).unwrap_or_default();
        let digits: Vec<u8> = after
            .iter()
            .copied()
            .skip_while(|b| b.is_ascii_whitespace())
            .take_while(u8::is_ascii_digit)
            .collect();
        std::str::from_utf8(&digits).ok()?.parse::<usize>().ok()
    });
    let looks_encrypted = find(tail, b"/Encrypt").is_some();
    let has_eof_marker = rfind(tail, b"%%EOF").is_some();
    Ok(QuickInfo {
        version,
        header_offset,
        header_present,
        startxref,
        looks_encrypted,
        has_eof_marker,
    })
}

/// Reverse byte-slice search.
pub fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).rposition(|w| w == needle)
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn header() {
        let (v, off) = detect_version(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n").expect("header");
        assert_eq!(v, PdfVersion { major: 1, minor: 7 });
        assert_eq!(off, 0);
        assert_eq!(
            detect_version(b"junk\n%PDF-2.0")
                .expect("header")
                .0
                .to_string(),
            "2.0"
        );
        assert_eq!(detect_version(b"not a pdf"), Err(Error::BadHeader));
        assert_eq!(detect_version(b"%PDF-x.y"), Err(Error::BadHeader));
    }

    #[test]
    fn header_without_minor_digit_reads_as_minor_zero() {
        for input in [&b"%PDF-1.\n1 0 obj"[..], b"%PDF-1\n", b"%PDF-1.", b"%PDF-1"] {
            let (v, off) = detect_version(input).unwrap_or_else(|e| panic!("{input:?}: {e}"));
            assert_eq!(v, PdfVersion { major: 1, minor: 0 }, "{input:?}");
            assert_eq!(off, 0);
        }
        assert_eq!(detect_version(b"%PDF-1.x"), Err(Error::BadHeader));
        assert_eq!(detect_version(b"%PDF-1x"), Err(Error::BadHeader));
    }

    #[test]
    fn headerless_file_starting_with_a_comment_is_tried() {
        let file =
            b"%\xE2\xE3\xCF\xD3\n1 0 obj\n<< >>\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let info = quick_info(file).expect("info");
        assert!(!info.header_present);
        assert_eq!(info.version, ASSUMED_VERSION);
        assert_eq!(info.header_offset, 0);
        assert!(info.has_eof_marker);
        // A real header is still preferred and reported as present.
        assert!(
            quick_info(b"%PDF-1.7\n1 0 obj\n")
                .expect("info")
                .header_present
        );
        // Not PDFs: no comment first, or a comment with no object at all.
        assert_eq!(quick_info(b"oops\n"), Err(Error::BadHeader));
        assert_eq!(
            quick_info(b"1/Catalogier\n]<\ntrailer"),
            Err(Error::BadHeader)
        );
        assert_eq!(quick_info(b"% just a comment\n"), Err(Error::BadHeader));
    }

    #[test]
    fn quick_facts() {
        let file = b"%PDF-1.4\n1 0 obj << >> endobj\ntrailer\n<< /Root 1 0 R /Encrypt 2 0 R >>\nstartxref\n42\n%%EOF\n";
        let info = quick_info(file).expect("info");
        assert_eq!(info.startxref, Some(42));
        assert!(info.looks_encrypted);
        assert!(info.has_eof_marker);
    }
}
