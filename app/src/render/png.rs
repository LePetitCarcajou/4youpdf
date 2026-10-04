//! The PNG the interface receives for a page. Encoding is done by the
//! process of the window, from pixels it has checked: WebView2 never
//! decodes compressed bytes made by the rendering worker (ADR 0008).
//!
//! The fidelity bench (`tools/render_bench`) compiles this file as it is,
//! to time encoding apart from drawing.

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::DynamicImage;

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
