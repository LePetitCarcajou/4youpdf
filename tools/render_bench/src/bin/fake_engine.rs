//! A fake engine for the tests of the bench. It follows the protocol
//! (`fyp_render_bench::protocol`) without reading the document: every page is
//! a synthetic drawing. On demand it misbehaves as a real engine may: it
//! crashes, hangs, answers nonsense, fails to open or fails a page.
//!
//! ```text
//! fyp-render-engine-fake [--name NAME] [--shift-page INDEX] [--fail-page INDEX]
//!     [--crash-after N] [--hang-after N] [--garbage] [--fail-open] [--protocol P]
//! ```

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use fyp_render_bench::protocol::{Reply, Request, PROTOCOL};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder, Rgb, RgbImage};

/// What the engine does wrong, from its arguments.
struct Behaviour {
    name: String,
    shift_page: Option<usize>,
    fail_page: Option<usize>,
    crash_after: Option<usize>,
    hang_after: Option<usize>,
    garbage: bool,
    fail_open: bool,
    protocol: u32,
}

fn main() -> ExitCode {
    let behaviour = match parse(std::env::args().skip(1)) {
        Ok(behaviour) => behaviour,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let mut out = std::io::stdout().lock();
    let mut input = String::new();
    let request = std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| e.to_string())
        .and_then(|_| serde_json::from_str::<Request>(&input).map_err(|e| e.to_string()));
    let request = match request {
        Ok(request) => request,
        Err(error) => {
            send(&mut out, &Reply::Fatal { error });
            return ExitCode::FAILURE;
        }
    };
    send(
        &mut out,
        &Reply::Engine {
            protocol: behaviour.protocol,
            name: behaviour.name.clone(),
            version: "0".to_string(),
            detail: "moteur factice des tests du banc".to_string(),
        },
    );
    if behaviour.garbage {
        let _ = writeln!(out, "ceci n'est pas une réponse");
        return ExitCode::SUCCESS;
    }
    if behaviour.fail_open {
        send(
            &mut out,
            &Reply::OpenFailed {
                error: "ouverture refusée par le moteur factice".to_string(),
            },
        );
        return ExitCode::SUCCESS;
    }
    send(&mut out, &Reply::Opened { ms: 0.25 });
    for (answered, page) in request.pages.iter().enumerate() {
        if behaviour.crash_after == Some(answered) {
            eprintln!("le moteur factice s'effondre");
            std::process::exit(101);
        }
        if behaviour.hang_after == Some(answered) {
            loop {
                std::thread::sleep(Duration::from_secs(60));
            }
        }
        if behaviour.fail_page == Some(page.index) {
            send(
                &mut out,
                &Reply::PageFailed {
                    index: page.index,
                    error: "page refusée par le moteur factice".to_string(),
                },
            );
            continue;
        }
        let (mut render_ms, mut encode_ms) = (Vec::new(), Vec::new());
        let mut first = None;
        for _ in 0..request.repeat.max(1) {
            let started = Instant::now();
            let image = draw(
                request.width,
                page.index,
                behaviour.shift_page == Some(page.index),
            );
            render_ms.push(started.elapsed().as_secs_f64() * 1000.0);
            let started = Instant::now();
            let png = encode(&image);
            encode_ms.push(started.elapsed().as_secs_f64() * 1000.0);
            first.get_or_insert((image, png));
        }
        let reply = match first {
            Some((image, Ok(png))) => match std::fs::write(&page.output, png) {
                Ok(()) => Reply::Page {
                    index: page.index,
                    width: image.width(),
                    height: image.height(),
                    render_ms,
                    encode_ms,
                    identical_repeats: true,
                },
                Err(e) => Reply::PageFailed {
                    index: page.index,
                    error: format!("{} : {e}", page.output.display()),
                },
            },
            Some((_, Err(error))) => Reply::PageFailed {
                index: page.index,
                error,
            },
            None => Reply::PageFailed {
                index: page.index,
                error: "aucune répétition".to_string(),
            },
        };
        send(&mut out, &reply);
    }
    ExitCode::SUCCESS
}

fn send(out: &mut impl Write, reply: &Reply) {
    let _ = writeln!(out, "{}", reply.to_line());
    let _ = out.flush();
}

/// A page `width` pixels wide, in the proportions of A4: white, with a black
/// block whose place depends on the page, moved 5 pixels right by `shift`.
fn draw(width: u32, index: usize, shift: bool) -> RgbImage {
    let height = u32::try_from((u64::from(width) * 842).div_ceil(595)).unwrap_or(width);
    let column = u32::try_from(index % 5).unwrap_or(0) * width / 10 + if shift { 5 } else { 0 };
    let (left, right) = (column, column + width / 4);
    let (top, bottom) = (height / 10, height / 3);
    RgbImage::from_fn(width, height, |x, y| {
        if (left..right).contains(&x) && (top..bottom).contains(&y) {
            Rgb([0, 0, 0])
        } else {
            Rgb([255, 255, 255])
        }
    })
}

/// A PNG with the settings of the application's page service.
fn encode(image: &RgbImage) -> Result<Vec<u8>, String> {
    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::Up)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgb8,
        )
        .map_err(|e| format!("encodage PNG : {e}"))?;
    Ok(png)
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Behaviour, String> {
    let mut behaviour = Behaviour {
        name: "fake".to_string(),
        shift_page: None,
        fail_page: None,
        crash_after: None,
        hang_after: None,
        garbage: false,
        fail_open: false,
        protocol: PROTOCOL,
    };
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{flag} : valeur manquante"))
        };
        let number = |text: String| text.parse::<usize>().map_err(|e| format!("{text} : {e}"));
        match flag.as_str() {
            "--name" => behaviour.name = value()?,
            "--shift-page" => behaviour.shift_page = Some(number(value()?)?),
            "--fail-page" => behaviour.fail_page = Some(number(value()?)?),
            "--crash-after" => behaviour.crash_after = Some(number(value()?)?),
            "--hang-after" => behaviour.hang_after = Some(number(value()?)?),
            "--protocol" => {
                behaviour.protocol = u32::try_from(number(value()?)?).map_err(|e| e.to_string())?;
            }
            "--garbage" => behaviour.garbage = true,
            "--fail-open" => behaviour.fail_open = true,
            other => return Err(format!("argument inconnu : {other}")),
        }
    }
    Ok(behaviour)
}
