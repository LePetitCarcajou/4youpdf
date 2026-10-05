//! The timing harness of `tools/render_timing.py` for v0.5.0, where PDFium
//! ran on a thread of the window: copied to `app/examples/render_timing.rs`
//! of a `git archive v0.5.0` tree, it compiles the `render.rs` of that
//! version as it is and times the same requests as
//! `app/examples/render_timing.rs` does today (ADR 0008, « Mesures »).
//!
//! `render_timing <pdfium dir> <requests> <file>...` prints, for each file
//! and each width, one line: file, width, then the milliseconds of each
//! request, tab-separated.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "../src/render.rs"]
mod render;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

/// The widths asked for: a thumbnail of the grid, and a page of the view.
const WIDTHS: [u32; 2] = [160, 1400];

fn main() {
    let mut arguments = std::env::args_os().skip(1);
    let pdfium = PathBuf::from(arguments.next().expect("pdfium directory"));
    let requests: usize = arguments
        .next()
        .and_then(|n| n.to_str().and_then(|n| n.parse().ok()))
        .expect("number of requests");
    let service = render::RenderService::start(&[pdfium]);
    let status = service.status();
    assert!(status.available, "{}", status.detail);
    for (id, file) in arguments.enumerate() {
        let name = PathBuf::from(&file);
        let bytes = Arc::new(std::fs::read(&name).expect("file"));
        for width in WIDTHS {
            let mut line = format!("{}\t{width}", name.display());
            for _ in 0..requests {
                let started = Instant::now();
                let png = service
                    .render(id as u64 + 1, Arc::clone(&bytes), "", 0, width)
                    .expect("page");
                let elapsed = started.elapsed();
                assert!(!png.is_empty());
                line.push_str(&format!("\t{:.3}", elapsed.as_secs_f64() * 1000.0));
            }
            println!("{line}");
        }
    }
}
