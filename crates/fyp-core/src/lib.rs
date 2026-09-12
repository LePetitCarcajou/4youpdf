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

pub mod document;
pub mod filters;
pub mod lexer;
pub mod object;
pub mod parser;
pub mod recover;
pub mod version;
pub mod xref;

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
    /// No `startxref` near the end of the file (ISO 32000-2, 7.5.5).
    MissingStartxref,
    /// Malformed cross-reference section, or an offset from it that leads
    /// nowhere (ISO 32000-2, 7.5.4).
    BadXref {
        /// Byte offset in the input.
        offset: usize,
        /// Human-readable explanation.
        message: String,
    },
    /// The `/Prev` chain of incremental updates comes back to a section
    /// already read (ISO 32000-2, 7.5.6). Protection against hostile files.
    XrefLoop {
        /// Offset of the section reached twice.
        offset: usize,
    },
    /// Valid PDF relying on a feature not implemented yet.
    Unsupported {
        /// The missing feature, with its clause of ISO 32000-2.
        feature: &'static str,
    },
    /// Objects parse but do not form the expected document structure
    /// (ISO 32000-2, 7.7), e.g. a trailer without `/Root`.
    BadStructure {
        /// Human-readable explanation.
        message: String,
    },
    /// Encoded stream data that its filter cannot decode (ISO 32000-2, 7.4).
    Filter {
        /// Filter name as written in the file, e.g. `ASCII85Decode`.
        filter: String,
        /// Human-readable explanation.
        message: String,
    },
    /// Decoding would produce more than the configured amount of data.
    /// Protection against decompression bombs (ISO 32000-2, 7.4.4 note).
    LimitExceeded {
        /// The limit that was hit, in bytes.
        limit: usize,
        /// What was being produced when the limit was hit.
        what: &'static str,
    },
    /// An object stream (ISO 32000-2, 7.5.7) that cannot be used: nested in
    /// another object stream, holding a stream, or malformed.
    BadObjectStream {
        /// Object number of the object stream.
        stream_num: u32,
        /// Human-readable explanation.
        message: String,
    },
    /// The declared cross-reference table is unusable and scanning the
    /// file found no `n g obj` to rebuild one from (see [`recover`]).
    Unrecoverable {
        /// Why the declared table could not be used.
        declared: Box<Error>,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::BadHeader => write!(f, "missing or malformed %PDF header"),
            Error::Syntax { offset, message } => {
                write!(f, "syntax error at byte {offset}: {message}")
            }
            Error::UnexpectedEof => write!(f, "unexpected end of input"),
            Error::TooDeep => write!(f, "object nesting too deep"),
            Error::MissingStartxref => write!(f, "no startxref found"),
            Error::BadXref { offset, message } => {
                write!(f, "cross-reference error at byte {offset}: {message}")
            }
            Error::XrefLoop { offset } => {
                write!(f, "/Prev chain loops back to the section at byte {offset}")
            }
            Error::Unsupported { feature } => write!(f, "not supported yet: {feature}"),
            Error::BadStructure { message } => {
                write!(f, "invalid document structure: {message}")
            }
            Error::Filter { filter, message } => write!(f, "{filter}: {message}"),
            Error::LimitExceeded { limit, what } => {
                write!(f, "{what} would exceed the limit of {limit} bytes")
            }
            Error::BadObjectStream {
                stream_num,
                message,
            } => write!(f, "object stream {stream_num}: {message}"),
            Error::Unrecoverable { declared } => write!(
                f,
                "cross-reference table unusable ({declared}) and no object found to rebuild it"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;
