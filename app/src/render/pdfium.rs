//! The only file that knows about `pdfium-render` (ADR 0005). It runs in
//! the rendering worker, never in the process of the window (ADR 0008):
//! [`Renderer::bind`] once, [`Renderer::open`] once per document, then
//! [`Loaded::draw`] for each page.
//!
//! The fidelity bench (`tools/render_bench`) compiles this file as it is
//! and takes the same steps one by one, to time them.

use std::path::PathBuf;

use image::DynamicImage;
use pdfium_render::prelude::*;

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

/// PDFium bound to its library. Not thread-safe: the thread that binds
/// it is the only one to use it.
pub struct Renderer(Pdfium);

/// A document loaded by PDFium, from its own copy of the bytes.
pub struct Loaded<'a>(PdfDocument<'a>);

impl Renderer {
    /// Bind to the library in the first of `candidates` (directories) that
    /// holds one, then, except on Windows, to the one of the system; the
    /// text says where it was found, or where it was looked for.
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
    /// proportions of the page as its `/Rotate` turns it. A failure names
    /// the page as the interface does, from 1.
    pub fn draw(&self, page: usize, width: u32) -> Result<DynamicImage, String> {
        let number = page.saturating_add(1);
        let index =
            PdfPageIndex::try_from(page).map_err(|_| format!("page {number} hors de portée"))?;
        let pdf_page = self
            .0
            .pages()
            .get(index)
            .map_err(|e| format!("page {number} : {e:?}"))?;
        let width = i32::try_from(width).unwrap_or(i32::MAX);
        let bitmap = pdf_page
            .render_with_config(&PdfRenderConfig::new().set_target_width(width))
            .map_err(|e| format!("rendu de la page {number} : {e:?}"))?;
        bitmap
            .as_image()
            .map_err(|e| format!("image de la page {number} : {e:?}"))
    }
}
