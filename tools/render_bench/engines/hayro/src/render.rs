//! Pages drawn by hayro (crates `hayro`, `hayro-interpret`, `hayro-syntax`),
//! in the three steps of the application's page service
//! (`app/src/render.rs`): [`open`], once per document, [`Loaded::draw`] and
//! [`encode_png`].
//!
//! The password never reaches hayro. `fyp-core` opens the document first, as
//! the application does before anything is drawn; hayro then reads the
//! bytes of the file, or, when the file is encrypted, the core's rewrite of
//! it in the clear (`fyp_core::writer`).
//!
//! hayro returns no error while drawing, and may panic on a file it does not
//! expect: every call into it is made under [`std::panic::catch_unwind`], so
//! that a panic fails one page, not the document.

use std::panic::{self, AssertUnwindSafe};

use fyp_core::document::Document;
use fyp_core::writer::{Writer, XrefStyle};
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::vello_cpu::Pixmap;
use hayro::{RenderCache, RenderSettings};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{DynamicImage, RgbaImage};

/// A document loaded by hayro, from its own copy of the bytes.
pub struct Loaded {
    pdf: Pdf,
    settings: InterpreterSettings,
}

/// Open `bytes` with `password` (empty for most files): by `fyp-core`, then
/// by hayro, from the bytes of the file or, for an encrypted file, from the
/// core's rewrite in the clear.
pub fn open(bytes: &[u8], password: &str) -> Result<Loaded, String> {
    let document = Document::open_with_password(bytes, password.as_bytes())
        .map_err(|e| format!("le noyau ne peut pas ouvrir ce fichier : {e}"))?;
    let source = match document.encryption() {
        Some(_) => in_the_clear(&document)?,
        None => bytes.to_vec(),
    };
    let pdf = caught(|| Pdf::new(source))
        .map_err(|e| format!("hayro a paniqué à l'ouverture : {e}"))?
        .map_err(|e| format!("hayro ne peut pas ouvrir ce fichier : {e:?}"))?;
    Ok(Loaded {
        pdf,
        settings: InterpreterSettings::default(),
    })
}

/// `document` written again by the core, in the clear: with a classic table,
/// or with a cross-reference stream when its numbering is too sparse for one,
/// as `fyp-host` rewrites what a module returns.
fn in_the_clear(document: &Document<'_>) -> Result<Vec<u8>, String> {
    let version = document.version();
    Writer::new(version)
        .write(document)
        .or_else(|first| {
            Writer::new(version)
                .xref_style(XrefStyle::Stream)
                .write(document)
                .map_err(|_| first)
        })
        .map_err(|e| format!("le noyau ne peut pas réécrire ce fichier en clair : {e}"))
}

impl Loaded {
    /// A cache for drawing the pages of this document: fonts and glyph
    /// outlines are kept from one drawing to the next, as PDFium keeps them
    /// for a loaded document.
    pub fn cache(&self) -> RenderCache<'_> {
        RenderCache::new()
    }

    /// Draw page `page` (0-based) on white, `width` pixels wide, in the
    /// proportions of the page as its `/Rotate` turns it.
    pub fn draw<'a>(
        &'a self,
        cache: &RenderCache<'a>,
        page: usize,
        width: u32,
    ) -> Result<DynamicImage, String> {
        let pdf_page = self
            .pdf
            .pages()
            .get(page)
            .ok_or_else(|| format!("page {page} hors de portée"))?;
        let (page_width, page_height) = pdf_page.render_dimensions();
        let (wide, high) = image_size(page_width, page_height, width).ok_or_else(|| {
            format!("page {page} : pas d'image de {width} pixels de large pour {page_width} × {page_height} points")
        })?;
        let scale = f32::from(wide) / page_width;
        let settings = RenderSettings {
            x_scale: scale,
            y_scale: scale,
            width: Some(wide),
            height: Some(high),
            bg_color: WHITE,
        };
        let pixmap = caught(|| hayro::render(pdf_page, cache, &self.settings, &settings))
            .map_err(|e| format!("hayro a paniqué en dessinant la page {page} : {e}"))?;
        to_image(pixmap).ok_or_else(|| format!("image de la page {page} : tampon incomplet"))
    }
}

/// Width and height in pixels of the image of a page of `page_width` ×
/// `page_height` points, drawn `width` pixels wide. The height is rounded as
/// PDFium rounds it (`PdfRenderConfig` of `pdfium-render`), so that both
/// engines give the same size to a page they measure alike. `None` beyond
/// what hayro draws, 65 535 pixels a side.
fn image_size(page_width: f32, page_height: f32, width: u32) -> Option<(u16, u16)> {
    let wide = u16::try_from(width).ok().filter(|wide| *wide > 0)?;
    let high = (page_height * (f32::from(wide) / page_width)).round();
    (1.0..=f32::from(u16::MAX))
        .contains(&high)
        .then_some((wide, high as u16))
}

/// The pixels of `pixmap`, premultiplied RGBA, as an image in straight RGBA.
/// On a white page every pixel is opaque and stays as it is.
fn to_image(pixmap: Pixmap) -> Option<DynamicImage> {
    let (width, height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
    let mut data = pixmap.data_as_u8_slice().to_vec();
    for [r, g, b, alpha] in data.as_chunks_mut::<4>().0 {
        if *alpha == 0 || *alpha == u8::MAX {
            continue;
        }
        let alpha = u16::from(*alpha);
        for channel in [r, g, b] {
            let straight = (u16::from(*channel) * 255 + alpha / 2) / alpha;
            *channel = u8::try_from(straight).unwrap_or(u8::MAX);
        }
    }
    RgbaImage::from_raw(width, height, data).map(DynamicImage::ImageRgba8)
}

/// The PNG the page service makes of `image` (`encode_png` of
/// `app/src/render.rs`): fast compression and the `Up` filter.
pub fn encode_png(image: &DynamicImage) -> Result<Vec<u8>, String> {
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

/// What `call` returns, or what its panic says. After a panic, the caches of
/// the document keep what hayro had put in them; they belong to that document
/// alone.
fn caught<T>(call: impl FnOnce() -> T) -> Result<T, String> {
    panic::catch_unwind(AssertUnwindSafe(call)).map_err(|payload| {
        payload
            .downcast_ref::<&str>()
            .map(|text| (*text).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "sans message".to_string())
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use hayro::vello_cpu::color::PremulRgba8;

    #[test]
    fn images_have_the_size_pdfium_gives_them() {
        // The heights of tools/render_bench/timings/pdfium.toml: A4 and
        // Letter pages 1400 pixels wide.
        assert_eq!(image_size(595.0, 842.0, 1400), Some((1400, 1981)));
        assert_eq!(image_size(612.0, 792.0, 1400), Some((1400, 1812)));
        assert_eq!(image_size(595.0, 842.0, 0), None);
        assert_eq!(image_size(595.0, 842.0, 70_000), None);
        assert_eq!(image_size(1.0, 100_000.0, 1400), None);
        assert_eq!(image_size(595.0, f32::NAN, 1400), None);
    }

    #[test]
    fn premultiplied_pixels_are_made_straight() {
        let mut pixmap = Pixmap::new(3, 1);
        let pixel = |r, g, b, a| PremulRgba8 { r, g, b, a };
        pixmap.set_pixel(0, 0, pixel(255, 128, 0, 255));
        pixmap.set_pixel(1, 0, pixel(64, 32, 0, 128));
        pixmap.set_pixel(2, 0, pixel(0, 0, 0, 0));
        let image = to_image(pixmap).unwrap().to_rgba8();
        assert_eq!(image.get_pixel(0, 0).0, [255, 128, 0, 255]);
        assert_eq!(image.get_pixel(1, 0).0, [128, 64, 0, 128]);
        assert_eq!(image.get_pixel(2, 0).0, [0, 0, 0, 0]);
    }

    #[test]
    fn a_panic_is_a_message() {
        assert_eq!(caught(|| 7), Ok(7));
        let silent = panic::take_hook();
        panic::set_hook(Box::new(|_| {}));
        let text = caught(|| -> u8 { panic!("glyphe introuvable") });
        let formatted = caught(|| -> u8 { panic!("page {}", 3) });
        panic::set_hook(silent);
        assert_eq!(text, Err("glyphe introuvable".to_string()));
        assert_eq!(formatted, Err("page 3".to_string()));
    }
}
