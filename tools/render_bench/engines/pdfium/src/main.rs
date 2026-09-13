//! The PDFium engine of the rendering fidelity bench (`tools/render_bench`;
//! `docs/banc-rendu.md`, « Protocole des moteurs »). It draws pages with the
//! application's own module, `app/src/render.rs`, compiled here as it is:
//! the same binding, the same drawing, the same PNG encoding, taken step by
//! step so that each step can be timed.
//!
//! PDFium is looked for in the directory `FYP_PDFIUM_DIR` names, then in
//! `app/pdfium/` of the checkout, where `tools/fetch_pdfium.py` puts the
//! pinned release: never in a copy left next to an executable.

#![forbid(unsafe_code)]

// The application's renderer. This engine calls its PDFium steps on its own
// thread; the worker thread, the service and the library search serve the
// application only.
#[allow(dead_code)]
#[path = "../../../../../app/src/render.rs"]
mod render;

use std::env::consts::{DLL_PREFIX, DLL_SUFFIX};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use fyp_render_bench::protocol::{PageRequest, Reply, Request, PROTOCOL};
use fyp_render_bench::system;
use render::pdfium::{encode_png, Loaded, Renderer};

fn main() -> ExitCode {
    let mut out = std::io::stdout().lock();
    match serve(&mut out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            send(&mut out, &Reply::Fatal { error });
            ExitCode::FAILURE
        }
    }
}

/// Answer the request on standard input. `Err` is fatal: no library, or a
/// request this engine does not understand.
fn serve(out: &mut impl Write) -> Result<(), String> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| format!("demande illisible : {e}"))?;
    let request: Request =
        serde_json::from_str(&input).map_err(|e| format!("demande illisible : {e}"))?;
    if request.protocol != PROTOCOL {
        return Err(format!(
            "demande au protocole {}, ce moteur parle le protocole {PROTOCOL}",
            request.protocol
        ));
    }
    let directories = library_directories();
    let (renderer, detail) = Renderer::bind(&directories)?;
    send(
        out,
        &Reply::Engine {
            protocol: PROTOCOL,
            name: "pdfium".to_string(),
            version: version(&directories, &detail),
            detail,
        },
    );
    let bytes = match std::fs::read(&request.document) {
        Ok(bytes) => bytes,
        Err(e) => {
            let error = format!("{} : {e}", request.document.display());
            send(out, &Reply::OpenFailed { error });
            return Ok(());
        }
    };
    let started = Instant::now();
    let document = match renderer.open(&bytes, &request.password) {
        Ok(document) => document,
        Err(error) => {
            send(out, &Reply::OpenFailed { error });
            return Ok(());
        }
    };
    send(
        out,
        &Reply::Opened {
            ms: millis(started),
        },
    );
    for page in &request.pages {
        send(out, &draw(&document, page, request.width, request.repeat));
    }
    Ok(())
}

/// Draw and encode `page` `repeat` times, timing each step, and write the
/// first image.
fn draw(document: &Loaded<'_>, page: &PageRequest, width: u32, repeat: u32) -> Reply {
    let failed = |error: String| Reply::PageFailed {
        index: page.index,
        error,
    };
    let (mut render_ms, mut encode_ms) = (Vec::new(), Vec::new());
    let mut first = None;
    let mut identical = true;
    for _ in 0..repeat.max(1) {
        let started = Instant::now();
        let image = match document.draw(page.index, width) {
            Ok(image) => image,
            Err(error) => return failed(error),
        };
        render_ms.push(millis(started));
        let started = Instant::now();
        let png = match encode_png(&image) {
            Ok(png) => png,
            Err(error) => return failed(error),
        };
        encode_ms.push(millis(started));
        match &first {
            None => first = Some((image, png)),
            Some((kept, _)) => {
                identical &= kept.width() == image.width()
                    && kept.height() == image.height()
                    && kept.as_bytes() == image.as_bytes();
            }
        }
    }
    let Some((image, png)) = first else {
        return failed("aucune répétition".to_string());
    };
    if let Err(e) = std::fs::write(&page.output, png) {
        return failed(format!("{} : {e}", page.output.display()));
    }
    Reply::Page {
        index: page.index,
        width: image.width(),
        height: image.height(),
        render_ms,
        encode_ms,
        identical_repeats: identical,
    }
}

/// Where to look for PDFium: `FYP_PDFIUM_DIR`, then `app/pdfium/` of this
/// checkout.
fn library_directories() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    std::env::var_os("FYP_PDFIUM_DIR")
        .map(PathBuf::from)
        .into_iter()
        .chain([root.join("app").join("pdfium")])
        .collect()
}

/// The release of the library `detail` says was loaded, from the `RELEASE`
/// file `tools/fetch_pdfium.py` writes next to it, and the start of the
/// library's SHA-256.
fn version(directories: &[PathBuf], detail: &str) -> String {
    let name = format!("{DLL_PREFIX}pdfium{DLL_SUFFIX}");
    for directory in directories {
        let library = directory.join(&name);
        if !detail.contains(&library.display().to_string()) {
            continue;
        }
        let release = std::fs::read_to_string(directory.join("RELEASE"))
            .map(|text| text.trim().to_string())
            .unwrap_or_else(|_| "release inconnue".to_string());
        let digest = std::fs::read(&library)
            .map(|bytes| system::sha256_hex(&bytes))
            .unwrap_or_default();
        return format!(
            "{release}, {name} sha256 {}",
            digest.get(..12).unwrap_or("?")
        );
    }
    "version inconnue".to_string()
}

fn millis(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn send(out: &mut impl Write, reply: &Reply) {
    let _ = writeln!(out, "{}", reply.to_line());
    let _ = out.flush();
}
