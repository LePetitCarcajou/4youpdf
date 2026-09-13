//! How far apart two images are. The bench ranks pages by a [`Metric`] chosen
//! by name ([`by_name`]): another metric changes the number in the reports,
//! neither the engines nor the rest of the bench.
//!
//! The default is [`Ssim`], `1 − SSIM`, computed the way the reference code of
//! its authors computes it: the image is first reduced so that its smaller
//! side is about 256 pixels, a view of the whole page, then compared through a
//! Gaussian window. Two renderers never draw the edge of a glyph or a thin
//! line with the same anti-aliasing, nor place it at the same fraction of a
//! pixel; at full resolution these differences weigh as much as missing
//! content, reduced they weigh little, while a missing glyph run, image or
//! form field, or a wrong colour, weigh in proportion to their area
//! (`docs/banc-rendu.md`, « La métrique », measures both). Each channel is
//! compared on its own, so that two colours of the same luminance still
//! differ. [`Pixels`] counts the pixels that differ at all, at full
//! resolution and without tolerance.

use std::borrow::Cow;

use image::{Rgb, RgbImage};

/// A distance between two images of the same size.
pub trait Metric: Send + Sync {
    /// Its name, on the command line and in the reports.
    fn name(&self) -> &'static str;

    /// What the number means, in one sentence of the report.
    fn description(&self) -> &'static str;

    /// How far `a` is from `b`, two images of the same size: 0 when they are
    /// identical, 1 at most, larger when further apart.
    fn distance(&self, a: &RgbImage, b: &RgbImage) -> f64;
}

/// The metrics the bench knows; the first is the default.
pub const NAMES: [&str; 2] = ["ssim", "pixels"];

/// The metric called `name`, one of [`NAMES`].
pub fn by_name(name: &str) -> Option<Box<dyn Metric>> {
    match name {
        "ssim" => Some(Box::new(Ssim)),
        "pixels" => Some(Box::new(Pixels)),
        _ => None,
    }
}

/// Smaller side, in samples, of the image SSIM compares: the reference code
/// of Wang et al. (`ssim.m`) reduces an image by
/// `max(1, round(min(width, height) / 256))` before comparing.
const VIEW: f64 = 256.0;
/// Radius of the Gaussian window: 11 × 11 samples.
const RADIUS: usize = 5;
/// Standard deviation of the Gaussian window, in samples.
const SIGMA: f64 = 1.5;
/// Stabilizing constants of SSIM for 8-bit channels (Wang, Bovik, Sheikh and
/// Simoncelli, 2004): (0.01 × 255)² and (0.03 × 255)².
const C1: f64 = 6.5025;
const C2: f64 = 58.5225;

/// `1 − mean SSIM` of Wang et al. (2004) as their reference code computes
/// it: the image reduced by [`reduction`], an 11 × 11 Gaussian window of
/// standard deviation 1.5 at every position where it fits, K1 = 0.01,
/// K2 = 0.03; for the red, green and blue channels in turn, averaged. A
/// window whose SSIM is negative (inverted structure) counts as 0: nothing
/// in common.
pub struct Ssim;

impl Metric for Ssim {
    fn name(&self) -> &'static str {
        "ssim"
    }

    fn description(&self) -> &'static str {
        "1 − similarité structurelle moyenne (SSIM de Wang et al., 2004, calculée comme leur code de référence : image réduite par moyenne de blocs pour que son petit côté fasse environ 256 pixels, fenêtre gaussienne 11 × 11 d'écart type 1,5 ; moyenne des canaux rouge, vert et bleu, une similarité négative comptée pour 0) : 0 pour deux images identiques, 1 pour deux images sans structure commune. Un anticrénelage ou un placement au sous-pixel différents pèsent peu ; un objet manquant ou d'une autre couleur pèse en proportion de sa surface."
    }

    fn distance(&self, a: &RgbImage, b: &RgbImage) -> f64 {
        if a.dimensions() != b.dimensions() {
            return 1.0;
        }
        if a.width() == 0 || a.height() == 0 {
            return 0.0;
        }
        let factor = reduction(a.width(), a.height());
        let (a, b) = (reduce(a, factor), reduce(b, factor));
        let (width, height) = (a.width() as usize, a.height() as usize);
        let similarity: f64 = (0..3)
            .map(|channel| mean_ssim(&plane(&a, channel), &plane(&b, channel), width, height))
            .sum();
        (1.0 - similarity / 3.0).clamp(0.0, 1.0)
    }
}

/// The factor by which [`Ssim`] reduces an image of `width` × `height`
/// pixels: 5 for a portrait page 1400 pixels wide.
pub fn reduction(width: u32, height: u32) -> u32 {
    let factor = (f64::from(width.min(height)) / VIEW).round();
    if factor < 1.0 {
        1
    } else {
        factor as u32
    }
}

/// `image` reduced by `factor`: each pixel the rounded mean of a block of
/// `factor` × `factor`, the incomplete blocks of the last row and column left
/// out.
fn reduce(image: &RgbImage, factor: u32) -> Cow<'_, RgbImage> {
    let (width, height) = (
        image.width() / factor.max(1),
        image.height() / factor.max(1),
    );
    if factor <= 1 || width == 0 || height == 0 {
        return Cow::Borrowed(image);
    }
    let samples = factor * factor;
    Cow::Owned(RgbImage::from_fn(width, height, |x, y| {
        let mut sums = [0u32; 3];
        for dy in 0..factor {
            for dx in 0..factor {
                let pixel = image.get_pixel(x * factor + dx, y * factor + dy);
                for (sum, value) in sums.iter_mut().zip(pixel.0) {
                    *sum += u32::from(value);
                }
            }
        }
        Rgb(sums.map(|sum| u8::try_from((sum + samples / 2) / samples).unwrap_or(u8::MAX)))
    }))
}

/// One channel of `image`, as numbers.
fn plane(image: &RgbImage, channel: usize) -> Vec<f64> {
    image
        .as_raw()
        .as_chunks::<3>()
        .0
        .iter()
        .map(|pixel| f64::from(pixel[channel]))
        .collect()
}

/// Normalized weights of a Gaussian window of `radius` samples each side.
fn gaussian(radius: usize) -> Vec<f64> {
    let weights: Vec<f64> = (0..=2 * radius)
        .map(|i| {
            let d = i as f64 - radius as f64;
            (-d * d / (2.0 * SIGMA * SIGMA)).exp()
        })
        .collect();
    let total: f64 = weights.iter().sum();
    weights.into_iter().map(|weight| weight / total).collect()
}

/// `plane` of `width` × `height` samples filtered by `kernel` across then
/// down, at the positions where the whole window fits.
fn filter(plane: &[f64], width: usize, height: usize, kernel: &[f64]) -> Vec<f64> {
    let span = kernel.len();
    let (across, down) = (width + 1 - span, height + 1 - span);
    let mut rows = vec![0.0; across * height];
    for (y, line) in plane.chunks(width).enumerate().take(height) {
        for x in 0..across {
            rows[y * across + x] = kernel
                .iter()
                .zip(&line[x..x + span])
                .map(|(k, v)| k * v)
                .sum();
        }
    }
    let mut out = vec![0.0; across * down];
    for y in 0..down {
        for x in 0..across {
            out[y * across + x] = kernel
                .iter()
                .enumerate()
                .map(|(i, k)| k * rows[(y + i) * across + x])
                .sum();
        }
    }
    out
}

/// Mean SSIM of two planes of `width` × `height` samples, over every
/// position of the window, negative values counted as 0. The window is
/// narrower on a plane smaller than it. For two identical planes, every
/// numerator is the same floating-point number as its denominator, and the
/// result exactly 1.
fn mean_ssim(a: &[f64], b: &[f64], width: usize, height: usize) -> f64 {
    let radius = RADIUS.min((width.min(height).max(1) - 1) / 2);
    let kernel = gaussian(radius);
    let blur = |plane: &[f64]| filter(plane, width, height, &kernel);
    let product =
        |x: &[f64], y: &[f64]| -> Vec<f64> { x.iter().zip(y).map(|(p, q)| p * q).collect() };
    let (mean_a, mean_b) = (blur(a), blur(b));
    let (square_a, square_b, cross) = (
        blur(&product(a, a)),
        blur(&product(b, b)),
        blur(&product(a, b)),
    );
    if mean_a.is_empty() {
        return 1.0;
    }
    let mut total = 0.0;
    for i in 0..mean_a.len() {
        let (ma, mb) = (mean_a[i], mean_b[i]);
        let variance_a = square_a[i] - ma * ma;
        let variance_b = square_b[i] - mb * mb;
        let covariance = cross[i] - ma * mb;
        let ssim = ((2.0 * ma * mb + C1) * (2.0 * covariance + C2))
            / ((ma * ma + mb * mb + C1) * (variance_a + variance_b + C2));
        total += ssim.clamp(0.0, 1.0);
    }
    total / mean_a.len() as f64
}

/// Share of the pixels whose colour differs, however little.
pub struct Pixels;

impl Metric for Pixels {
    fn name(&self) -> &'static str {
        "pixels"
    }

    fn description(&self) -> &'static str {
        "Part des pixels dont la couleur diffère, si peu que ce soit, en pleine résolution : 0 pour deux images identiques, 1 quand tous diffèrent. Sans aucune tolérance : l'anticrénelage d'un autre moteur y compte autant qu'un objet manquant."
    }

    fn distance(&self, a: &RgbImage, b: &RgbImage) -> f64 {
        if a.dimensions() != b.dimensions() {
            return 1.0;
        }
        let total = a.as_raw().len() / 3;
        if total == 0 {
            return 0.0;
        }
        let differing = a
            .as_raw()
            .as_chunks::<3>()
            .0
            .iter()
            .zip(b.as_raw().as_chunks::<3>().0.iter())
            .filter(|(pa, pb)| pa != pb)
            .count();
        differing as f64 / total as f64
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// A page of text: strokes 3 pixels high every 17 rows, broken every 7
    /// columns. `soft` lightens their top and bottom rows, as the
    /// anti-aliasing of another rasterizer would.
    fn text(width: u32, height: u32, soft: u8) -> RgbImage {
        RgbImage::from_fn(width, height, |x, y| {
            let row = y % 17;
            if x % 7 == 0 || row > 2 {
                Rgb([255, 255, 255])
            } else if row == 1 {
                Rgb([20, 20, 20])
            } else {
                let value = 20 + soft;
                Rgb([value, value, value])
            }
        })
    }

    /// Deterministic noise: structure everywhere.
    fn noise(width: u32, height: u32, seed: u32) -> RgbImage {
        let mut state = seed;
        RgbImage::from_fn(width, height, |_, _| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let [r, g, b, _] = state.to_le_bytes();
            Rgb([r, g, b])
        })
    }

    #[test]
    fn identical_images_are_at_distance_exactly_zero() {
        for name in NAMES {
            let metric = by_name(name).unwrap();
            for image in [
                text(768, 1024, 0),
                noise(900, 700, 9),
                noise(64, 40, 7),
                noise(1, 1, 3),
                noise(3, 5, 4),
            ] {
                assert_eq!(metric.distance(&image, &image.clone()), 0.0, "{name}");
            }
        }
        assert!(by_name("psnr").is_none());
    }

    #[test]
    fn the_whole_page_is_seen() {
        assert_eq!(reduction(1400, 1980), 5);
        assert_eq!(reduction(1400, 990), 4);
        assert_eq!(reduction(120, 170), 1);
        assert_eq!(reduction(1, 1), 1);
        assert_eq!(gaussian(5).len(), 11);
        assert!((gaussian(5).iter().sum::<f64>() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn anti_aliasing_weighs_less_than_missing_content() {
        let ssim = Ssim;
        let page = text(768, 1024, 0);
        let softer = text(768, 1024, 90);
        let mut missing = page.clone();
        for y in 300..600 {
            for x in 200..500 {
                missing.put_pixel(x, y, Rgb([255, 255, 255]));
            }
        }
        let (soft, gone) = (
            ssim.distance(&page, &softer),
            ssim.distance(&page, &missing),
        );
        assert!(soft > 0.0 && soft < gone, "{soft} vs {gone}");
        // Symmetric, up to rounding.
        assert!((gone - ssim.distance(&missing, &page)).abs() < 1e-12);
        // Two flat colours of close luminance: far apart.
        let red = RgbImage::from_pixel(300, 300, Rgb([200, 40, 40]));
        let green = RgbImage::from_pixel(300, 300, Rgb([40, 130, 40]));
        assert!(ssim.distance(&red, &green) > 0.3);
        // Inverted structure: nothing in common.
        let inverted = RgbImage::from_fn(768, 1024, |x, y| {
            let Rgb([r, g, b]) = *page.get_pixel(x, y);
            Rgb([255 - r, 255 - g, 255 - b])
        });
        assert!(ssim.distance(&page, &inverted) > 0.9);
        // Another size is as far as can be.
        assert_eq!(ssim.distance(&page, &text(768, 1025, 0)), 1.0);
    }

    #[test]
    fn pixels_counts_every_difference() {
        let a = RgbImage::from_pixel(10, 10, Rgb([255, 255, 255]));
        let mut b = a.clone();
        b.put_pixel(3, 4, Rgb([254, 255, 255]));
        assert_eq!(Pixels.distance(&a, &b), 0.01);
        assert_eq!(Pixels.distance(&a, &RgbImage::new(10, 9)), 1.0);
    }
}
