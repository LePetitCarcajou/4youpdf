//! The rendering fidelity bench (`docs/banc-rendu.md`). Two engines draw the
//! same reference pages at the same width; the bench measures how far apart
//! their images are and how long each engine took, drawing and PNG encoding
//! apart. It exists for the exit criterion of ADR 0005: PDFium goes once our
//! engine renders the fixtures and the public corpus with a comparable
//! fidelity.
//!
//! The bench knows an engine only through [`protocol`]: a program named in
//! `engines.toml` ([`engine`]), run once per document of the page set
//! ([`pageset`]). The distance between two images is a [`metric::Metric`],
//! chosen by name. [`run`] puts it together, [`report`] writes what it found
//! and [`timings`] the reference times; [`select`] chose the page set.

#![forbid(unsafe_code)]

pub mod compare;
pub mod engine;
pub mod metric;
pub mod pageset;
pub mod protocol;
pub mod report;
pub mod run;
pub mod select;
pub mod stats;
pub mod system;
pub mod timings;
