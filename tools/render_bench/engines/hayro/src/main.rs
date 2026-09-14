//! The hayro engine of the rendering fidelity bench (`tools/render_bench`;
//! `docs/banc-rendu.md`, « Les moteurs »). It draws pages with hayro, a PDF
//! rasterizer written in Rust, in the steps of the application's page service
//! ([`render`]), taken one by one so that each can be timed, and answers the
//! protocol as the PDFium engine does.
//!
//! Documents are opened by `fyp-core`: the password goes to the core, and an
//! encrypted file reaches hayro as the core rewrites it, in the clear.

#![forbid(unsafe_code)]

mod render;

use std::io::{Read, Write};
use std::process::ExitCode;
use std::time::Instant;

use fyp_render_bench::protocol::{PageRequest, Reply, Request, PROTOCOL};
use hayro::RenderCache;
use render::{encode_png, Loaded};

/// The crates that draw, at the versions `Cargo.lock` resolves
/// (tests/engine.rs checks them).
const VERSION: &str = "hayro 0.7.1, hayro-interpret 0.7.0, hayro-syntax 0.7.2";

/// How documents reach hayro, for the report.
const DETAIL: &str = "hayro compilé dans le moteur ; le document est ouvert par fyp-core, et un fichier chiffré est lu par hayro tel que le noyau le réécrit en clair : le mot de passe ne va jamais à hayro";

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

/// Answer the request on standard input. `Err` is fatal: a request this
/// engine does not understand.
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
    send(
        out,
        &Reply::Engine {
            protocol: PROTOCOL,
            name: "hayro".to_string(),
            version: VERSION.to_string(),
            detail: DETAIL.to_string(),
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
    let document = match render::open(&bytes, &request.password) {
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
    let cache = document.cache();
    for page in &request.pages {
        send(
            out,
            &draw(&document, &cache, page, request.width, request.repeat),
        );
    }
    Ok(())
}

/// Draw and encode `page` `repeat` times, timing each step, and write the
/// first image.
fn draw<'a>(
    document: &'a Loaded,
    cache: &RenderCache<'a>,
    page: &PageRequest,
    width: u32,
    repeat: u32,
) -> Reply {
    let failed = |error: String| Reply::PageFailed {
        index: page.index,
        error,
    };
    let (mut render_ms, mut encode_ms) = (Vec::new(), Vec::new());
    let mut first = None;
    let mut identical = true;
    for _ in 0..repeat.max(1) {
        let started = Instant::now();
        let image = match document.draw(cache, page.index, width) {
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

fn millis(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn send(out: &mut impl Write, reply: &Reply) {
    let _ = writeln!(out, "{}", reply.to_line());
    let _ = out.flush();
}
