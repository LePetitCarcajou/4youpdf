//! Times requests for pages from end to end, from the service of the window
//! to the PNG, through the rendering worker (ADR 0008, « Mesures »).
//! `tools/render_timing.py` runs it against the same loop compiled over
//! v0.5.0 (`tools/render_timing/baseline_v0_5_0.rs`), where PDFium ran on a
//! thread of the window.
//!
//! `render_timing <fyp-app> <pdfium dir> <requests> <file>...` prints, for
//! each file and each width, one line: file, width, then the milliseconds
//! of each request, tab-separated. `<fyp-app>` is the executable started as
//! the worker.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use fyp_app::render::{Executable, Limits, RenderService};

/// The widths asked for: a thumbnail of the grid, and a page of the view.
const WIDTHS: [u32; 2] = [160, 1400];

fn main() {
    let mut arguments = std::env::args_os().skip(1);
    let worker = PathBuf::from(arguments.next().expect("fyp-app executable"));
    let pdfium = PathBuf::from(arguments.next().expect("pdfium directory"));
    let requests: usize = arguments
        .next()
        .and_then(|n| n.to_str().and_then(|n| n.parse().ok()))
        .expect("number of requests");
    let service = RenderService::start_with(
        Box::new(Executable::new(worker)),
        &[pdfium],
        Limits::default(),
    );
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
                write!(line, "\t{:.3}", elapsed.as_secs_f64() * 1000.0).unwrap();
            }
            println!("{line}");
        }
    }
}
