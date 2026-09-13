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
    /// (directories), then, except on Windows, in the system library path.
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

/// Where the PDFium library may be, in this order: the directory named by
/// `FYP_PDFIUM_DIR`; the directory of the executable, where the installer
/// and the portable archive put it; and `development`, the `app/pdfium/`
/// of the checkout a development build was compiled from
/// (`tools/fetch_pdfium.py`), which a packaged build does not give.
pub fn library_candidates(development: Option<PathBuf>) -> Vec<PathBuf> {
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
    dirs.extend(development);
    dirs
}

/// The only place that knows about `pdfium-render`. A request takes three
/// steps: [`Renderer::open`], once per document, [`Loaded::draw`] and
/// [`encode_png`]. The worker takes them in turn; the fidelity bench
/// (`tools/render_bench`) takes them one by one, to time drawing apart from
/// encoding.
pub mod pdfium {
    use super::*;
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::DynamicImage;
    use pdfium_render::prelude::*;

    /// Bind to the first library found, then serve requests until the
    /// sender is dropped.
    pub(super) fn worker(candidates: &[PathBuf], ready: &Sender<Status>, rx: &Receiver<Request>) {
        let (renderer, detail) = match Renderer::bind(candidates) {
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
        let mut loaded: Option<(u64, Loaded<'_>)> = None;
        for request in rx {
            let result = serve(&renderer, &mut loaded, &request);
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
        // Windows has no system copy of PDFium, and its search for a bare
        // library name goes through the current directory and `PATH`: a
        // `pdfium.dll` left there must never be loaded in place of ours.
        #[cfg(not(windows))]
        if let Ok(bindings) = Pdfium::bind_to_system_library() {
            return Ok((
                Pdfium::new(bindings),
                "PDFium chargé depuis le système".into(),
            ));
        }
        Err(format!(
            "bibliothèque PDFium introuvable (à côté de l'exécutable une fois installé, dans app/pdfium/ après tools/fetch_pdfium.py en développement) ; cherchée : {}",
            tried.join(", ")
        ))
    }

    fn serve<'a>(
        renderer: &'a Renderer,
        loaded: &mut Option<(u64, Loaded<'a>)>,
        request: &Request,
    ) -> Result<Vec<u8>, String> {
        if loaded.as_ref().map(|(id, _)| *id) != Some(request.document_id) {
            *loaded = None;
            let document = renderer.open(&request.bytes, &request.password)?;
            *loaded = Some((request.document_id, document));
        }
        let Some((_, document)) = loaded.as_ref() else {
            return Err("document non chargé".into());
        };
        encode_png(&document.draw(request.page, request.width)?)
    }

    /// PDFium bound to its library. Not thread-safe: the thread that binds
    /// it is the only one to use it.
    pub struct Renderer(Pdfium);

    /// A document loaded by PDFium, from its own copy of the bytes.
    pub struct Loaded<'a>(PdfDocument<'a>);

    impl Renderer {
        /// Bind to the library in the first of `candidates` that holds one
        /// (see `bind`); the text says where it was found, or where it was
        /// looked for.
        pub fn bind(candidates: &[PathBuf]) -> Result<(Renderer, String), String> {
            bind(candidates).map(|(pdfium, detail)| (Renderer(pdfium), detail))
        }

        /// Load `bytes`, deciphered with `password` unless it is empty.
        pub fn open(&self, bytes: &[u8], password: &str) -> Result<Loaded<'_>, String> {
            let password = (!password.is_empty()).then_some(password);
            self.0
                .load_pdf_from_byte_vec(bytes.to_vec(), password)
                .map(Loaded)
                .map_err(|e| format!("PDFium ne peut pas ouvrir ce fichier : {e:?}"))
        }
    }

    impl Loaded<'_> {
        /// Draw page `page` (0-based), `width` pixels wide, in the
        /// proportions of the page as its `/Rotate` turns it.
        pub fn draw(&self, page: usize, width: u32) -> Result<DynamicImage, String> {
            let index =
                PdfPageIndex::try_from(page).map_err(|_| format!("page {page} hors de portée"))?;
            let pdf_page = self
                .0
                .pages()
                .get(index)
                .map_err(|e| format!("page {page} : {e:?}"))?;
            let width = i32::try_from(width).unwrap_or(i32::MAX);
            let bitmap = pdf_page
                .render_with_config(&PdfRenderConfig::new().set_target_width(width))
                .map_err(|e| format!("rendu de la page {page} : {e:?}"))?;
            bitmap
                .as_image()
                .map_err(|e| format!("image de la page {page} : {e:?}"))
        }
    }

    /// The PNG the interface receives for `image`.
    pub fn encode_png(image: &DynamicImage) -> Result<Vec<u8>, String> {
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

    /// A packaged build looks next to its executable, never in the checkout
    /// it was built from; a development build looks there last.
    #[test]
    fn only_a_development_build_looks_in_the_checkout() {
        let exe_dir = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("pdfium");
        let packaged = library_candidates(None);
        assert!(packaged.contains(&exe_dir));
        assert!(!packaged.contains(&checkout));
        let development = library_candidates(Some(checkout.clone()));
        assert_eq!(development.last(), Some(&checkout));
        assert_eq!(development[..development.len() - 1], packaged[..]);
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
        let large = service
            .render(7, Arc::clone(&bytes), "", 0, 1400)
            .expect("render");
        let image = image::load_from_memory(&large).expect("decode");
        assert_eq!(image.width(), 1400);
        let ratio = f64::from(image.height()) / f64::from(image.width());
        assert!((ratio - 842.0 / 595.0).abs() < 0.01, "A4, got {ratio}");
        // Once turned (`session::rotate`), the page is drawn on its side:
        // the renderer follows the `/Rotate` of the rewrite.
        let turned = crate::session::rotate(&bytes, "", &[0], 90).expect("rotate");
        let png = service
            .render(8, Arc::new(turned.bytes), "", 0, 120)
            .expect("render");
        let image = image::load_from_memory(&png).expect("decode");
        assert_eq!(image.width(), 120);
        let ratio = f64::from(image.height()) / f64::from(image.width());
        assert!(
            (ratio - 595.0 / 842.0).abs() < 0.01,
            "A4 on its side, got {ratio}"
        );
        // Out-of-range page: an error, not a panic.
        assert!(service.render(7, Arc::new(Vec::new()), "", 9, 60).is_err());
    }
}
