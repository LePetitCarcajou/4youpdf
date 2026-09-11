//! `fyp-crypto` — the standard security handler (ISO 32000-2, clause 7.6).
//!
//! Planned: RC4 (revisions 2-4), AES-128 (revision 4), AES-256 (revisions
//! 5 and 6, the PDF 2.0 one). Primitives come from audited crates
//! (`aes`, `sha2`, `cbc`); this crate only wires the PDF key derivation.
//!
//! Milestone 0.1: placeholder so the workspace shape is final.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Security handler revision, as found in `/R` of the `/Encrypt` dictionary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Revision {
    /// RC4 40-bit.
    R2,
    /// RC4 up to 128-bit.
    R3,
    /// RC4 or AES-128 via crypt filters.
    R4,
    /// AES-256 (Adobe extension level 3, deprecated).
    R5,
    /// AES-256, ISO 32000-2. The only revision new files should use.
    R6,
}
