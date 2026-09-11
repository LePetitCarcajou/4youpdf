//! `fyp-core` — the 4YouPDF document engine.
//!
//! Scope of this crate: the PDF *syntax* and *document structure* layers of
//! ISO 32000-2:2020 (lexing, object model, cross-reference tables and streams,
//! filters, incremental updates, writing). It knows nothing about rendering,
//! fonts, OCR or conformance rules — those live in other crates or plugins.
//!
//! Design rules (see `docs/architecture.md`):
//! - `#![forbid(unsafe_code)]` — the parser is pure safe Rust.
//! - Never panic on malformed input. Every failure is an [`Error`].
//! - Be tolerant when reading (recover broken xref tables), strict when writing.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod lexer;
pub mod object;
pub mod parser;
pub mod version;

use std::fmt;

/// Every failure `fyp-core` can report. The engine never panics on input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Missing or malformed `%PDF-x.y` header.
    BadHeader,
    /// Unexpected byte or token at the given offset.
    Syntax {
        /// Byte offset in the input.
        offset: usize,
        /// Human-readable explanation.
        message: String,
    },
    /// Input ended while a token or object was still open.
    UnexpectedEof,
    /// Nesting deeper than [`parser::MAX_DEPTH`] (protection against hostile files).
    TooDeep,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::BadHeader => write!(f, "missing or malformed %PDF header"),
            Error::Syntax { offset, message } => write!(f, "syntax error at byte {offset}: {message}"),
            Error::UnexpectedEof => write!(f, "unexpected end of input"),
            Error::TooDeep => write!(f, "object nesting too deep"),
        }
    }
}

impl std::error::Error for Error {}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;
