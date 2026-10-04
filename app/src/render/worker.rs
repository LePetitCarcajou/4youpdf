//! The rendering worker: `fyp-app` started with [`ARGUMENT`], by the
//! window, which talks to it over its standard input and output
//! ([`protocol`](super::protocol), ADR 0008). It binds PDFium and answers
//! requests one by one until its standard input closes; it starts nothing
//! else of the application. A crash, a stack overflow or an endless
//! drawing of the engine are the worker's, not the window's.

use std::ffi::OsString;
use std::io::{Read, Write};

use super::pdfium::{Loaded, Renderer};
use super::protocol::{
    self, Reply, Request, MAX_HEIGHT, MAX_PIXELS, MAX_WIDTH, MIN_WIDTH, VERSION,
};

/// The argument that makes `fyp-app` a rendering worker, when it is the
/// first and only one. Reserved: a file of that name is not opened.
pub const ARGUMENT: &str = "--fyp-render-worker";

/// Exit status of a worker whose window closed the channel, or that has no
/// PDFium to bind.
const DONE: i32 = 0;

/// Exit status of a worker that was told something it does not
/// understand, or that could not answer.
const BROKEN: i32 = 2;

/// Whether `arguments`, those of the process with its name first, ask for
/// a worker.
pub fn asked(arguments: impl IntoIterator<Item = OsString>) -> bool {
    let mut arguments = arguments.into_iter().skip(1);
    arguments.next().is_some_and(|first| first == ARGUMENT) && arguments.next().is_none()
}

/// Serve the window on the standard input and output of this process until
/// the input closes; the exit status to end the process with.
pub fn run() -> i32 {
    serve(&mut std::io::stdin().lock(), &mut std::io::stdout().lock())
}

/// Serve the requests of `input` on `output`; the exit status.
fn serve(input: &mut impl Read, output: &mut impl Write) -> i32 {
    let candidates = match protocol::read_request(input) {
        Ok(Some(Request::Hello {
            version: VERSION,
            candidates,
        })) => candidates,
        Ok(Some(Request::Hello { .. })) => {
            let _ = Reply::Refused { version: VERSION }.write_to(output);
            return BROKEN;
        }
        Ok(None) => return DONE,
        Ok(Some(_)) | Err(_) => return BROKEN,
    };
    let renderer = match Renderer::bind(&candidates) {
        Ok((renderer, detail)) => {
            let ready = Reply::Ready {
                version: VERSION,
                available: true,
                detail,
            };
            if ready.write_to(output).is_err() {
                return BROKEN;
            }
            renderer
        }
        Err(detail) => {
            let _ = Reply::Ready {
                version: VERSION,
                available: false,
                detail,
            }
            .write_to(output);
            return DONE;
        }
    };
    // The loaded document, kept while requests concern the same one.
    let mut loaded: Option<(u64, Loaded<'_>)> = None;
    loop {
        let reply = match protocol::read_request(input) {
            Ok(Some(Request::Document {
                sequence,
                id,
                password,
                bytes,
            })) => {
                // The one before goes first: never two documents at once.
                loaded = None;
                match renderer.open(&bytes, &password) {
                    Ok(document) => {
                        loaded = Some((id, document));
                        Reply::Opened { sequence }
                    }
                    Err(message) => Reply::Failed { sequence, message },
                }
            }
            Ok(Some(Request::Draw {
                sequence,
                id,
                page,
                width,
            })) => match draw(loaded.as_ref(), id, page, width) {
                Ok((width, height, pixels)) => Reply::Bitmap {
                    sequence,
                    width,
                    height,
                    pixels,
                },
                Err(message) => Reply::Failed { sequence, message },
            },
            Ok(None) => return DONE,
            Ok(Some(Request::Hello { .. })) | Err(_) => return BROKEN,
        };
        if reply.write_to(output).is_err() {
            return BROKEN;
        }
    }
}

/// Page `page` (0-based) of the document `id`, `width` pixels wide: the
/// width, the height and the RGBA pixels of an image the window will
/// accept.
fn draw(
    loaded: Option<&(u64, Loaded<'_>)>,
    id: u64,
    page: u32,
    width: u32,
) -> Result<(u32, u32, Vec<u8>), String> {
    let Some((_, document)) = loaded.filter(|(held, _)| *held == id) else {
        return Err("document non chargé".into());
    };
    if !(MIN_WIDTH..=MAX_WIDTH).contains(&width) {
        return Err(format!("largeur de {width} pixels hors limites"));
    }
    let number = u64::from(page) + 1;
    let page = usize::try_from(page).map_err(|_| format!("page {number} hors de portée"))?;
    let image = document.draw(page, width)?.into_rgba8();
    let (drawn, height) = image.dimensions();
    if drawn != width {
        return Err(format!(
            "page {number} dessinée sur {drawn} pixels de large au lieu de {width}"
        ));
    }
    if height > MAX_HEIGHT || u64::from(drawn) * u64::from(height) > MAX_PIXELS {
        return Err(format!(
            "la page {number} est trop haute pour être dessinée à cette largeur ({drawn} × {height} pixels)"
        ));
    }
    Ok((drawn, height, image.into_raw()))
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn arguments(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    /// The worker mode takes its argument first and alone: a file to open
    /// is never mistaken for it, nor the argument for a file.
    #[test]
    fn the_worker_argument_is_the_first_and_only_one() {
        assert!(asked(arguments(&["fyp-app", ARGUMENT])));
        assert!(!asked(arguments(&["fyp-app"])));
        assert!(!asked(arguments(&[])));
        assert!(!asked(arguments(&["fyp-app", "a.pdf"])));
        assert!(!asked(arguments(&["fyp-app", "a.pdf", ARGUMENT])));
        assert!(!asked(arguments(&["fyp-app", ARGUMENT, "a.pdf"])));
        assert!(!asked(arguments(&[ARGUMENT])));
        assert_eq!(ARGUMENT, "--fyp-render-worker");
    }

    fn served(requests: &[Request]) -> (i32, Vec<Reply>) {
        let mut input = Vec::new();
        for request in requests {
            request.write_to(&mut input).unwrap();
        }
        served_bytes(&input)
    }

    fn served_bytes(input: &[u8]) -> (i32, Vec<Reply>) {
        let mut output = Vec::new();
        let status = serve(&mut &input[..], &mut output);
        let mut replies = Vec::new();
        let mut output = &output[..];
        while let Ok(reply) = protocol::read_reply(&mut output) {
            replies.push(reply);
        }
        (status, replies)
    }

    /// A window of another version is refused, and told which version the
    /// worker speaks.
    #[test]
    fn an_unknown_version_is_refused() {
        let (status, replies) = served(&[Request::Hello {
            version: VERSION + 1,
            candidates: Vec::new(),
        }]);
        assert_eq!(status, BROKEN);
        assert_eq!(replies, [Reply::Refused { version: VERSION }]);
    }

    /// Anything but a hello first ends the worker without an answer, and an
    /// input closed before the first frame ends it quietly.
    #[test]
    fn the_first_frame_is_a_hello_or_the_worker_stops() {
        let draw = Request::Draw {
            sequence: 1,
            id: 1,
            page: 0,
            width: 100,
        };
        assert_eq!(served(&[draw]), (BROKEN, Vec::new()));
        assert_eq!(
            served_bytes(b"GET / HTTP/1.1\r\n\r\n"),
            (BROKEN, Vec::new())
        );
        assert_eq!(served_bytes(&[]), (DONE, Vec::new()));
    }

    /// Without a library where it is told to look, the worker says so and
    /// stops (unless the system has a PDFium of its own, which Windows
    /// never does).
    #[test]
    fn a_worker_without_pdfium_says_so_and_stops() {
        let (status, replies) = served(&[Request::Hello {
            version: VERSION,
            candidates: vec![std::path::PathBuf::from("Z:/nowhere")],
        }]);
        let [Reply::Ready {
            version: VERSION,
            available,
            detail,
        }] = &replies[..]
        else {
            panic!("{replies:?}");
        };
        if !available {
            assert_eq!(status, DONE);
            assert!(detail.contains("introuvable"), "{detail}");
        }
    }
}
