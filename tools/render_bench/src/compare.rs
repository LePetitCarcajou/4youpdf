//! The two images of a page, side by side: whether they are identical, how
//! many pixels differ, how far apart the metric puts them, and a picture of
//! where they differ.

use std::borrow::Cow;
use std::path::Path;

use image::{Rgb, RgbImage, RgbaImage};
use serde::Serialize;

use crate::metric::Metric;

/// Largest gap in width or height put down to rounding the height of the
/// page to whole pixels: one pixel. Beyond it, the engines do not agree on
/// the size of the page, and the page is as far apart as can be.
pub const ROUNDING_GAP: u32 = 1;

/// A PNG image as an engine wrote it, 8 bits per channel with alpha.
pub fn load_png(path: &Path) -> Result<RgbaImage, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{} : {e}", path.display()))?;
    image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .map(|image| image.to_rgba8())
        .map_err(|e| format!("{} : image PNG illisible : {e}", path.display()))
}

/// `image` over a white page, as a reader sees transparent pixels.
pub fn over_white(image: &RgbaImage) -> RgbImage {
    let mut out = RgbImage::new(image.width(), image.height());
    for (source, target) in image.pixels().zip(out.pixels_mut()) {
        let [r, g, b, alpha] = source.0;
        let alpha = u32::from(alpha);
        let blend = |channel: u8| {
            let value = (u32::from(channel) * alpha + 255 * (255 - alpha) + 127) / 255;
            u8::try_from(value).unwrap_or(u8::MAX)
        };
        *target = Rgb([blend(r), blend(g), blend(b)]);
    }
    out
}

/// What comparing the images of a page found.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Comparison {
    /// Width and height of engine A's image, in pixels.
    pub a_size: (u32, u32),
    /// Width and height of engine B's image, in pixels.
    pub b_size: (u32, u32),
    /// Same size, and the same bytes, alpha included.
    pub identical: bool,
    /// Pixels whose colour differs, over white, in the area both images
    /// cover.
    pub differing_pixels: u64,
    /// Pixels of that area.
    pub compared_pixels: u64,
    /// The metric's distance over that area; 1 when the sizes differ by more
    /// than [`ROUNDING_GAP`].
    pub distance: f64,
    /// Why the distance is not the metric's alone, when it is not.
    pub note: Option<String>,
}

/// Compare `a` and `b` with `metric`, and draw where they differ
/// ([`diff_picture`]).
pub fn compare(a: &RgbaImage, b: &RgbaImage, metric: &dyn Metric) -> (Comparison, RgbImage) {
    let identical = a.dimensions() == b.dimensions() && a.as_raw() == b.as_raw();
    let (a_white, b_white) = (over_white(a), over_white(b));
    let (width, height) = (a.width().min(b.width()), a.height().min(b.height()));
    let common = |image: &RgbImage| -> RgbImage {
        if image.dimensions() == (width, height) {
            image.clone()
        } else {
            image::imageops::crop_imm(image, 0, 0, width, height).to_image()
        }
    };
    let (a_common, b_common) = if a.dimensions() == b.dimensions() {
        (Cow::Borrowed(&a_white), Cow::Borrowed(&b_white))
    } else {
        (Cow::Owned(common(&a_white)), Cow::Owned(common(&b_white)))
    };
    let differing_pixels = a_common
        .as_raw()
        .as_chunks::<3>()
        .0
        .iter()
        .zip(b_common.as_raw().as_chunks::<3>().0.iter())
        .filter(|(pa, pb)| pa != pb)
        .count() as u64;
    let gap = a
        .width()
        .abs_diff(b.width())
        .max(a.height().abs_diff(b.height()));
    let sizes = format!(
        "{} × {} et {} × {} pixels",
        a.width(),
        a.height(),
        b.width(),
        b.height()
    );
    let (distance, note) = if gap > ROUNDING_GAP {
        (1.0, Some(format!("tailles différentes : {sizes}")))
    } else if gap > 0 {
        (
            metric.distance(&a_common, &b_common),
            Some(format!(
                "tailles différentes d'un pixel ({sizes}), comparées sur {width} × {height}"
            )),
        )
    } else {
        (metric.distance(&a_common, &b_common), None)
    };
    let comparison = Comparison {
        a_size: a.dimensions(),
        b_size: b.dimensions(),
        identical,
        differing_pixels,
        compared_pixels: u64::from(width) * u64::from(height),
        distance,
        note,
    };
    (comparison, diff_picture(&a_white, &b_white))
}

/// Where two images differ: `a` in light grey, each differing pixel in red,
/// the more saturated the larger its largest channel difference, even a
/// difference of one level; magenta where only one image has pixels.
pub fn diff_picture(a: &RgbImage, b: &RgbImage) -> RgbImage {
    let (width, height) = (a.width().max(b.width()), a.height().max(b.height()));
    RgbImage::from_fn(width, height, |x, y| {
        match (a.get_pixel_checked(x, y), b.get_pixel_checked(x, y)) {
            (Some(pa), Some(pb)) => {
                let [r, g, b_] = pa.0.map(u32::from);
                let grey = 200 + (299 * r + 587 * g + 114 * b_) / 1000 * 55 / 255;
                let delta =
                    pa.0.iter()
                        .zip(pb.0)
                        .map(|(ca, cb)| u32::from(ca.abs_diff(cb)))
                        .max()
                        .unwrap_or(0);
                if delta == 0 {
                    let grey = u8::try_from(grey).unwrap_or(u8::MAX);
                    Rgb([grey, grey, grey])
                } else {
                    // From 25 % of red for a difference of one level to
                    // pure red from 64 levels on.
                    let strength = (64 + delta * 3).min(255);
                    let mix = |from: u32, to: u32| {
                        u8::try_from((from * (255 - strength) + to * strength) / 255)
                            .unwrap_or(u8::MAX)
                    };
                    Rgb([mix(grey, 220), mix(grey, 0), mix(grey, 0)])
                }
            }
            _ => Rgb([255, 0, 255]),
        }
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::metric::Ssim;
    use image::Rgba;

    fn white(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 255]))
    }

    #[test]
    fn identical_images() {
        let (comparison, picture) = compare(&white(30, 40), &white(30, 40), &Ssim);
        assert!(comparison.identical);
        assert_eq!(comparison.differing_pixels, 0);
        assert_eq!(comparison.compared_pixels, 1200);
        assert_eq!(comparison.distance, 0.0);
        assert_eq!(comparison.note, None);
        assert_eq!(picture.dimensions(), (30, 40));
        assert!(picture.pixels().all(|p| p.0[0] == p.0[1]), "grey only");
    }

    #[test]
    fn one_level_of_one_pixel_is_a_difference() {
        let a = white(30, 40);
        let mut b = a.clone();
        b.put_pixel(5, 6, Rgba([255, 254, 255, 255]));
        let (comparison, picture) = compare(&a, &b, &Ssim);
        assert!(!comparison.identical);
        assert_eq!(comparison.differing_pixels, 1);
        assert!(comparison.distance > 0.0);
        let Rgb([r, g, _]) = *picture.get_pixel(5, 6);
        assert!(r > g, "the pixel is marked");
    }

    #[test]
    fn transparency_is_seen_over_white() {
        let a = white(4, 4);
        let b = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 0]));
        let (comparison, _) = compare(&a, &b, &Ssim);
        assert!(!comparison.identical, "not the same bytes");
        assert_eq!(comparison.differing_pixels, 0, "the same page to a reader");
        assert_eq!(comparison.distance, 0.0);
    }

    #[test]
    fn sizes_that_differ() {
        let (rounded, picture) = compare(&white(30, 40), &white(30, 41), &Ssim);
        assert_eq!(rounded.distance, 0.0);
        assert_eq!(rounded.compared_pixels, 1200);
        assert!(rounded.note.unwrap().contains("d'un pixel"));
        assert_eq!(picture.dimensions(), (30, 41));
        assert_eq!(*picture.get_pixel(0, 40), Rgb([255, 0, 255]));
        let (apart, _) = compare(&white(30, 40), &white(40, 30), &Ssim);
        assert_eq!(apart.distance, 1.0);
        assert!(apart.note.unwrap().starts_with("tailles différentes :"));
    }

    #[test]
    fn a_png_is_read_back() {
        let dir = std::env::temp_dir().join(format!("fyp-render-bench-png-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("page.png");
        white(3, 2).save(&path).unwrap();
        assert_eq!(load_png(&path).unwrap(), white(3, 2));
        std::fs::write(&path, b"not a png").unwrap();
        assert!(load_png(&path).unwrap_err().contains("illisible"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
