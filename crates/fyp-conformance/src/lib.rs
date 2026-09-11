//! `fyp-conformance` — declarative rule engine for the PDF sub-standards.
//!
//! One engine, several rule sets: PDF/A (ISO 19005), PDF/X (ISO 15930),
//! PDF/E (ISO 24517), PDF/UA (ISO 14289), PDF/VT (ISO 16612). Each rule has
//! an identifier, a clause reference, a severity and, when possible, an
//! automatic fix. The UI's conformance panel is a direct view of this.
//!
//! Milestone 0.1: types only.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// A family of conformance profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    /// Long-term archiving.
    PdfA,
    /// Print production.
    PdfX,
    /// Engineering documents.
    PdfE,
    /// Accessibility.
    PdfUa,
    /// Variable data printing.
    PdfVt,
}

/// A concrete profile, e.g. PDF/A-2b.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Profile {
    /// Family.
    pub family: Family,
    /// Part number (1, 2, 3, 4 for PDF/A).
    pub part: u8,
    /// Conformance level (`a`, `b`, `u`, `e`, `f`) when the family has one.
    pub level: Option<char>,
}

/// Outcome of one rule against one document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Rule identifier, stable across releases.
    pub rule: String,
    /// Clause of the standard, e.g. `6.2.11.4.1`.
    pub clause: String,
    /// Human explanation (French default).
    pub message: String,
    /// Pages concerned, if applicable.
    pub pages: Vec<u32>,
    /// Whether an automatic fix exists.
    pub fixable: bool,
}
