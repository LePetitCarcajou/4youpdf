//! PDF header detection (`%PDF-x.y`) and quick file facts, without parsing
//! the whole document. Used by `fyp info` and as the first sanity check when
//! opening a file.

use crate::parser::find;
use crate::{Error, Result};

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
    /// readers tolerate up to 1024 bytes).
    pub header_offset: usize,
    /// Offset announced by the last `startxref`, if found.
    pub startxref: Option<usize>,
    /// Whether the file has an `/Encrypt` entry somewhere in its trailer area.
    pub looks_encrypted: bool,
    /// Whether an `%%EOF` marker was found.
    pub has_eof_marker: bool,
}

/// Locate and parse the `%PDF-x.y` header.
pub fn detect_version(input: &[u8]) -> Result<(PdfVersion, usize)> {
    let window = &input[..input.len().min(1024)];
    let off = find(window, b"%PDF-").ok_or(Error::BadHeader)?;
    let rest = &input[off + 5..];
    let major = rest.first().and_then(digit).ok_or(Error::BadHeader)?;
    if rest.get(1) != Some(&b'.') {
        return Err(Error::BadHeader);
    }
    let minor = rest.get(2).and_then(digit).ok_or(Error::BadHeader)?;
    Ok((PdfVersion { major, minor }, off))
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
    let (version, header_offset) = detect_version(input)?;
    // Look at the tail of the file for startxref / trailer / %%EOF.
    let tail_start = input.len().saturating_sub(2048);
    let tail = &input[tail_start..];
    let startxref = rfind(tail, b"startxref").and_then(|i| {
        let after = &tail[i + b"startxref".len()..];
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
    fn quick_facts() {
        let file = b"%PDF-1.4\n1 0 obj << >> endobj\ntrailer\n<< /Root 1 0 R /Encrypt 2 0 R >>\nstartxref\n42\n%%EOF\n";
        let info = quick_info(file).expect("info");
        assert_eq!(info.startxref, Some(42));
        assert!(info.looks_encrypted);
        assert!(info.has_eof_marker);
    }
}
