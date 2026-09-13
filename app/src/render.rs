//! Page images: the thumbnails of the grid and the page view, sized to the
//! window. The interface asks for "page N of the open document, W pixels
//! wide" and gets a PNG; nothing else about the renderer reaches it. Today
//! the renderer is PDFium through `pdfium-render` (ADR 0005), loaded at run
//! time when its shared library is found; without it the application still
//! opens, reorders and saves, with blank placeholders.
//!
//! PDFium is not thread-safe, so one worker thread owns the library and
//! the loaded document and answers requests one by one through a channel.
//! A thumbnail is quick; a page as wide as the window is not, so the
//! interface puts nothing in front of the page on screen: thumbnails wait
//! while the page view is open, and the view sends one request at a time,
//! its current page first.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use serde::Serialize;

/// A rendering request: the document (shared bytes, identified for the
/// worker's cache), a page and a width.
pub struct Request {
    pub document_id: u64,
    pub bytes: Arc<Vec<u8>>,
    pub password: String,
    pub page: usize,
    pub width: u32,
    pub reply: SyncSender<Result<Vec<u8>, String>>,
}

/// Whether thumbnails can be drawn, and why not when they cannot.
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub available: bool,
    pub detail: String,
}

/// Handle to the worker thread.
pub struct RenderService {
    sender: Mutex<Sender<Request>>,
    status: Status,
}

impl RenderService {
    /// Start the worker. Looks for the PDFium library in `candidates`
    /// (directories), then in the system library path.
    pub fn start(candidates: &[PathBuf]) -> RenderService {
        let (ready_tx, ready_rx) = mpsc::channel();
        let (tx, rx) = mpsc::channel::<Request>();
        let candidates = candidates.to_vec();
        thread::Builder::new()
            .name("pdfium".into())
            .spawn(move || pdfium::worker(&candidates, &ready_tx, &rx))
            .ok();
        let status = ready_rx.recv().unwrap_or_else(|_| Status {
            available: false,
            detail: "le thread de rendu n'a pas démarré".into(),
        });
        RenderService {
            sender: Mutex::new(tx),
            status,
        }
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    /// Render one page as PNG. Blocks until the worker answers: call it
    /// off the interface thread.
    pub fn render(
        &self,
        document_id: u64,
        bytes: Arc<Vec<u8>>,
        password: &str,
        page: usize,
        width: u32,
    ) -> Result<Vec<u8>, String> {
        if !self.status.available {
            return Err(self.status.detail.clone());
        }
        let (reply, answer) = mpsc::sync_channel(1);
        let request = Request {
            document_id,
            bytes,
            password: password.to_string(),
            page,
            width: width.clamp(16, 4096),
            reply,
        };
        self.sender
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .send(request)
            .map_err(|_| "le thread de rendu s'est arrêté".to_string())?;
        answer
            .recv()
            .unwrap_or_else(|_| Err("le thread de rendu s'est arrêté".to_string()))
    }
}

/// Where the PDFium library may be: `FYP_PDFIUM_DIR`, next to the
/// executable, and `app/pdfium/` in a development checkout.
pub fn library_candidates() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("FYP_PDFIUM_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        dirs.push(dir);
    }
    dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("pdfium"));
    dirs
}

/// The only place that knows about `pdfium-render`.
mod pdfium {
    use super::*;
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use pdfium_render::prelude::*;

    /// Bind to the first library found, then serve requests until the
    /// sender is dropped.
    pub(super) fn worker(candidates: &[PathBuf], ready: &Sender<Status>, rx: &Receiver<Request>) {
        let (pdfium, detail) = match bind(candidates) {
            Ok(found) => found,
            Err(detail) => {
                let _ = ready.send(Status {
                    available: false,
                    detail,
                });
                return;
            }
        };
        let _ = ready.send(Status {
            available: true,
            detail,
        });
        // The loaded document, kept while requests concern the same one.
        let mut loaded: Option<(u64, PdfDocument<'_>)> = None;
        for request in rx {
            let result = serve(&pdfium, &mut loaded, &request);
            let _ = request.reply.send(result);
        }
    }

    fn bind(candidates: &[PathBuf]) -> Result<(Pdfium, String), String> {
        let mut tried = Vec::new();
        for dir in candidates {
            let path = Pdfium::pdfium_platform_library_name_at_path(dir);
            if !path.exists() {
                tried.push(path.display().to_string());
                continue;
            }
            match Pdfium::bind_to_library(&path) {
                Ok(bindings) => {
                    return Ok((
                        Pdfium::new(bindings),
                        format!("PDFium chargé depuis {}", path.display()),
                    ))
                }
                Err(e) => tried.push(format!("{} ({e})", path.display())),
            }
        }
        if let Ok(bindings) = Pdfium::bind_to_system_library() {
            return Ok((
                Pdfium::new(bindings),
                "PDFium chargé depuis le système".into(),
            ));
        }
        Err(format!(
            "bibliothèque PDFium introuvable (tools/fetch_pdfium.py la place dans app/pdfium/) ; cherchée : {}",
            tried.join(", ")
        ))
    }

    fn serve<'a>(
        pdfium: &'a Pdfium,
        loaded: &mut Option<(u64, PdfDocument<'a>)>,
        request: &Request,
    ) -> Result<Vec<u8>, String> {
        if loaded.as_ref().map(|(id, _)| *id) != Some(request.document_id) {
            *loaded = None;
            let password = (!request.password.is_empty()).then_some(request.password.as_str());
            let document = pdfium
                .load_pdf_from_byte_vec((*request.bytes).clone(), password)
                .map_err(|e| format!("PDFium ne peut pas ouvrir ce fichier : {e:?}"))?;
            *loaded = Some((request.document_id, document));
        }
        let Some((_, document)) = loaded.as_ref() else {
            return Err("document non chargé".into());
        };
        let index = PdfPageIndex::try_from(request.page)
            .map_err(|_| format!("page {} hors de portée", request.page))?;
        let page = document
            .pages()
            .get(index)
            .map_err(|e| format!("page {} : {e:?}", request.page))?;
        let width = i32::try_from(request.width).unwrap_or(i32::MAX);
        let bitmap = page
            .render_with_config(&PdfRenderConfig::new().set_target_width(width))
            .map_err(|e| format!("rendu de la page {} : {e:?}", request.page))?;
        let image = bitmap
            .as_image()
            .map_err(|e| format!("image de la page {} : {e:?}", request.page))?;
        // A large image spends its time in PNG encoding, not in PDFium. The
        // `Up` filter suits pages, whose rows are mostly alike: over four
        // times faster than the adaptive default, for files 12 to 14 %
        // larger (a page 1400 pixels wide in a debug build: 190 ms instead
        // of 810 ms).
        let mut png = Vec::new();
        image
            .write_with_encoder(PngEncoder::new_with_quality(
                &mut png,
                CompressionType::Fast,
                FilterType::Up,
            ))
            .map_err(|e| format!("encodage PNG : {e}"))?;
        Ok(png)
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn missing_library_is_reported_not_fatal() {
        let service = RenderService::start(&[PathBuf::from("Z:/nowhere")]);
        // Either a system PDFium is installed (available) or not: in both
        // cases the service answers.
        if !service.status().available {
            assert!(service.status().detail.contains("introuvable"));
            let err = service
                .render(1, Arc::new(Vec::new()), "", 0, 100)
                .unwrap_err();
            assert!(err.contains("introuvable"));
        }
    }

    /// With the library fetched (tools/fetch_pdfium.py), the first page
    /// of a fixture renders to a PNG of the requested width.
    #[test]
    fn renders_a_fixture_when_the_library_is_present() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("pdfium");
        let service = RenderService::start(&[dir]);
        if !service.status().available {
            eprintln!("PDFium not fetched: skipped ({})", service.status().detail);
            return;
        }
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/encrypted-rc4.pdf");
        let bytes = Arc::new(std::fs::read(path).unwrap());
        let png = service
            .render(7, Arc::clone(&bytes), "", 0, 120)
            .expect("render");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let image = image::load_from_memory(&png).expect("decode");
        assert_eq!(image.width(), 120);
        assert!(image.height() > 120, "A4 is taller than wide");
        // Second request on the same document: served from the cache.
        assert!(service.render(7, Arc::clone(&bytes), "", 0, 60).is_ok());
        // The page view asks for the width of the window: the image has
        // that width and the proportions of the page.
        let large = service.render(7, bytes, "", 0, 1400).expect("render");
        let image = image::load_from_memory(&large).expect("decode");
        assert_eq!(image.width(), 1400);
        let ratio = f64::from(image.height()) / f64::from(image.width());
        assert!((ratio - 842.0 / 595.0).abs() < 0.01, "A4, got {ratio}");
        // Out-of-range page: an error, not a panic.
        assert!(service.render(7, Arc::new(Vec::new()), "", 9, 60).is_err());
    }
}
