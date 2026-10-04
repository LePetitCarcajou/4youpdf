//! The part of the 4YouPDF desktop application that is a library: the
//! page images ([`render`]), in the process of the window and in the
//! rendering worker, which is this same executable in another mode
//! (ADR 0008). A library so that the tests of `tests/` run the service of
//! the window against the real `fyp-app` executable; the window itself is
//! `main.rs`.

#![forbid(unsafe_code)]

pub mod render;
