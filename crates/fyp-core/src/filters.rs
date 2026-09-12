//! Standard stream filters (ISO 32000-2, 7.4).
//!
//! Implemented: `FlateDecode` (7.4.4) with the TIFF and PNG predictors
//! (7.4.4.4), `ASCIIHexDecode` (7.4.2), `ASCII85Decode` (7.4.3) and
//! `RunLengthDecode` (7.4.5). `LZWDecode` (7.4.4), the image filters
//! (7.4.6 to 7.4.9) and non-identity `Crypt` filters (7.4.10) report
//! [`Error::Unsupported`].
//!
//! Every decoder is bounded: the amount of decoded data never exceeds
//! [`DecodeLimits::max_output`], whatever the input claims, so a small
//! hostile stream cannot inflate into gigabytes ([`Error::LimitExceeded`]).
//!
//! Tolerance, in line with the rest of the reader: leading whitespace before
//! zlib data, corrupt or truncated tails after some data was produced, and
//! raw deflate data without the zlib header are all accepted, as the major
//! readers do. Truly undecodable data is [`Error::Filter`].

use std::io::Read;

use flate2::read::{DeflateDecoder, ZlibDecoder};

use crate::lexer::is_whitespace;
use crate::object::{Dict, Name, Object};
use crate::{Error, Result};

/// Default ceiling on decoded data: 256 MiB.
pub const DEFAULT_MAX_OUTPUT: usize = 256 * 1024 * 1024;

/// Bounds applied while decoding (protection against hostile files).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeLimits {
    /// Maximum size, in bytes, of the output of any single filter stage.
    pub max_output: usize,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        DecodeLimits {
            max_output: DEFAULT_MAX_OUTPUT,
        }
    }
}

/// Apply the `/Filter` chain of a stream dictionary to its raw `data`, with
/// the matching `/DecodeParms`, under the default [`DecodeLimits`].
///
/// `resolve` follows indirect references found in the dictionary (the
/// document layer passes [`crate::document::Document::resolve`]; callers
/// without a document pass `|o| Ok(o.clone())`, in which case references
/// are treated as `null`).
pub fn decode_stream<R>(dict: &Dict, data: &[u8], resolve: R) -> Result<Vec<u8>>
where
    R: Fn(&Object) -> Result<Object>,
{
    decode_stream_with(dict, data, resolve, DecodeLimits::default())
}

/// Same as [`decode_stream`] with explicit limits.
pub fn decode_stream_with<R>(
    dict: &Dict,
    data: &[u8],
    resolve: R,
    limits: DecodeLimits,
) -> Result<Vec<u8>>
where
    R: Fn(&Object) -> Result<Object>,
{
    let chain = filter_chain(dict, &resolve)?;
    if chain.is_empty() {
        return Ok(data.to_vec());
    }
    let mut current = data.to_vec();
    for (name, parms) in &chain {
        current = apply_filter(name, parms.as_ref(), &current, &resolve, limits)?;
        if current.len() > limits.max_output {
            return Err(limit(limits, "decoded stream data"));
        }
    }
    Ok(current)
}

/// The filters of a stream with their parameters, in application order
/// (ISO 32000-2, 7.4.1: `/Filter` is a name or an array of names,
/// `/DecodeParms` a dictionary or an array with one item per filter).
fn filter_chain<R>(dict: &Dict, resolve: &R) -> Result<Vec<(Name, Option<Dict>)>>
where
    R: Fn(&Object) -> Result<Object>,
{
    let filters = match dict.get(&Name::new("Filter")).map(resolve).transpose()? {
        None | Some(Object::Null) => return Ok(Vec::new()),
        Some(Object::Name(n)) => vec![n],
        Some(Object::Array(items)) => items
            .iter()
            .map(|item| match resolve(item)? {
                Object::Name(n) => Ok(n),
                _ => Err(Error::Filter {
                    filter: "/Filter".into(),
                    message: "array holds something that is not a filter name".into(),
                }),
            })
            .collect::<Result<Vec<_>>>()?,
        Some(_) => {
            return Err(Error::Filter {
                filter: "/Filter".into(),
                message: "must be a name or an array of names".into(),
            })
        }
    };
    // `/DP` is the inline-image abbreviation (8.9.7); tolerated here.
    let parms_entry = dict
        .get(&Name::new("DecodeParms"))
        .or_else(|| dict.get(&Name::new("DP")));
    let mut parms: Vec<Option<Dict>> = match parms_entry.map(resolve).transpose()? {
        Some(Object::Dict(d)) => vec![Some(d)],
        Some(Object::Array(items)) => items
            .iter()
            .map(|item| match resolve(item)? {
                Object::Dict(d) => Ok(Some(d)),
                _ => Ok(None),
            })
            .collect::<Result<Vec<_>>>()?,
        // Missing, `null` or junk: no parameters.
        _ => Vec::new(),
    };
    // Tolerance: a lone dictionary next to several filters belongs to the
    // one that takes parameters (predictors of Flate/LZW), not to the first.
    if parms.len() == 1 && filters.len() > 1 {
        if let Some(pos) = filters.iter().position(takes_predictor) {
            let lone = parms.pop().flatten();
            parms = vec![None; filters.len()];
            if let Some(slot) = parms.get_mut(pos) {
                *slot = lone;
            }
        }
    }
    let mut parms = parms.into_iter();
    Ok(filters
        .into_iter()
        .map(|name| {
            let p = parms.next().flatten();
            (name, p)
        })
        .collect())
}

fn takes_predictor(name: &Name) -> bool {
    matches!(
        name.0.as_slice(),
        b"FlateDecode" | b"Fl" | b"LZWDecode" | b"LZW"
    )
}

/// One filter stage. Abbreviated names come from inline images
/// (ISO 32000-2, table 92) and are tolerated on ordinary streams.
fn apply_filter<R>(
    name: &Name,
    parms: Option<&Dict>,
    data: &[u8],
    resolve: &R,
    limits: DecodeLimits,
) -> Result<Vec<u8>>
where
    R: Fn(&Object) -> Result<Object>,
{
    match name.0.as_slice() {
        b"FlateDecode" | b"Fl" => {
            let inflated = flate_decode(data, limits)?;
            apply_predictor_parms(inflated, parms, resolve)
        }
        b"LZWDecode" | b"LZW" => Err(Error::Unsupported {
            feature: "LZWDecode filter (ISO 32000-2, 7.4.4)",
        }),
        b"ASCIIHexDecode" | b"AHx" => Ok(ascii_hex_decode(data)),
        b"ASCII85Decode" | b"A85" => ascii85_decode(data),
        b"RunLengthDecode" | b"RL" => run_length_decode(data, limits),
        b"Crypt" => Ok(data.to_vec()),
        b"CCITTFaxDecode" | b"CCF" => Err(Error::Unsupported {
            feature: "CCITTFaxDecode filter (ISO 32000-2, 7.4.6)",
        }),
        b"JBIG2Decode" => Err(Error::Unsupported {
            feature: "JBIG2Decode filter (ISO 32000-2, 7.4.7)",
        }),
        b"DCTDecode" | b"DCT" => Err(Error::Unsupported {
            feature: "DCTDecode filter (ISO 32000-2, 7.4.8)",
        }),
        b"JPXDecode" => Err(Error::Unsupported {
            feature: "JPXDecode filter (ISO 32000-2, 7.4.9)",
        }),
        _ => Err(Error::Filter {
            filter: name.as_str_lossy(),
            message: "unknown filter".into(),
        }),
    }
}

// ---------------------------------------------------------------------------
// FlateDecode (ISO 32000-2, 7.4.4)
// ---------------------------------------------------------------------------

/// Inflate zlib data, bounded by `limits.max_output`.
///
/// Tolerance: leading whitespace is skipped; data that fails after some
/// output was produced yields that output (truncated streams are common);
/// data without a zlib header is retried as raw deflate.
pub fn flate_decode(data: &[u8], limits: DecodeLimits) -> Result<Vec<u8>> {
    let start = data
        .iter()
        .position(|&b| !is_whitespace(b))
        .unwrap_or(data.len());
    let trimmed = data.get(start..).unwrap_or_default();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let (out, failed) = inflate_bounded(ZlibDecoder::new(trimmed), limits)?;
    if !failed || !out.is_empty() {
        return Ok(out);
    }
    let (out, failed) = inflate_bounded(DeflateDecoder::new(data), limits)?;
    if !failed || !out.is_empty() {
        return Ok(out);
    }
    Err(Error::Filter {
        filter: "FlateDecode".into(),
        message: "corrupt deflate data".into(),
    })
}

/// Read everything `reader` yields, up to the limit. Returns the bytes and
/// whether the reader failed (after producing them). Only a size overrun is
/// an error here: callers decide what a corrupt tail means.
fn inflate_bounded<R: Read>(reader: R, limits: DecodeLimits) -> Result<(Vec<u8>, bool)> {
    let cap = u64::try_from(limits.max_output)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut out = Vec::new();
    let failed = reader.take(cap).read_to_end(&mut out).is_err();
    if out.len() > limits.max_output {
        return Err(limit(limits, "inflated FlateDecode data"));
    }
    Ok((out, failed))
}

// ---------------------------------------------------------------------------
// Predictors (ISO 32000-2, 7.4.4.4)
// ---------------------------------------------------------------------------

/// Predictor parameters from `/DecodeParms` (ISO 32000-2, table 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Predictor {
    /// 1 (none), 2 (TIFF) or 10 to 15 (PNG).
    pub predictor: u8,
    /// Interleaved colour components per sample (`/Colors`, default 1).
    pub colors: usize,
    /// Bits per component: 1, 2, 4, 8 or 16 (`/BitsPerComponent`, default 8).
    pub bits_per_component: usize,
    /// Samples per row (`/Columns`, default 1).
    pub columns: usize,
}

/// Upper bound on `/Colors`. The standard allows more than 4 since PDF 1.5
/// but gives no maximum; this keeps row sizes sane on hostile input.
const MAX_COLORS: usize = 256;

fn apply_predictor_parms<R>(data: Vec<u8>, parms: Option<&Dict>, resolve: &R) -> Result<Vec<u8>>
where
    R: Fn(&Object) -> Result<Object>,
{
    let Some(parms) = parms else {
        return Ok(data);
    };
    let int = |key: &str, default: i64| -> Result<i64> {
        match parms.get(&Name::new(key)).map(resolve).transpose()? {
            Some(Object::Integer(i)) => Ok(i),
            _ => Ok(default),
        }
    };
    let predictor = int("Predictor", 1)?;
    if predictor == 1 {
        return Ok(data);
    }
    let bad = |message: &str| Error::Filter {
        filter: "Predictor".into(),
        message: message.into(),
    };
    let predictor = match predictor {
        2 | 10..=15 => u8::try_from(predictor).map_err(|_| bad("value out of range"))?,
        _ => return Err(bad("unknown /Predictor value")),
    };
    let colors = usize::try_from(int("Colors", 1)?)
        .ok()
        .filter(|c| (1..=MAX_COLORS).contains(c))
        .ok_or_else(|| bad("/Colors out of range"))?;
    let bits_per_component = usize::try_from(int("BitsPerComponent", 8)?)
        .ok()
        .filter(|b| matches!(b, 1 | 2 | 4 | 8 | 16))
        .ok_or_else(|| bad("/BitsPerComponent must be 1, 2, 4, 8 or 16"))?;
    let columns = usize::try_from(int("Columns", 1)?)
        .ok()
        .filter(|&c| c >= 1)
        .ok_or_else(|| bad("/Columns must be at least 1"))?;
    apply_predictor(
        &data,
        Predictor {
            predictor,
            colors,
            bits_per_component,
            columns,
        },
    )
}

/// Undo a TIFF (2) or PNG (10 to 15) predictor. Predictor 1 returns the
/// input unchanged.
pub fn apply_predictor(data: &[u8], p: Predictor) -> Result<Vec<u8>> {
    let bad = |message: &str| Error::Filter {
        filter: "Predictor".into(),
        message: message.into(),
    };
    if !(1..=MAX_COLORS).contains(&p.colors)
        || !matches!(p.bits_per_component, 1 | 2 | 4 | 8 | 16)
        || p.columns == 0
    {
        return Err(bad("invalid predictor parameters"));
    }
    let bits_per_pixel = p
        .colors
        .checked_mul(p.bits_per_component)
        .ok_or_else(|| bad("row too wide"))?;
    let row_bits = bits_per_pixel
        .checked_mul(p.columns)
        .ok_or_else(|| bad("row too wide"))?;
    let row_len = row_bits.div_ceil(8);
    // Bytes per pixel, rounded up, as PNG defines it.
    let bpp = bits_per_pixel.div_ceil(8);
    match p.predictor {
        1 => Ok(data.to_vec()),
        2 => Ok(tiff_unpredict(data, row_len, p)),
        10..=15 => png_unpredict(data, row_len, bpp),
        _ => Err(bad("unknown /Predictor value")),
    }
}

/// PNG predictors: each row starts with a filter-type byte, 0 to 4
/// (ISO 32000-2, table 10; PNG specification, "Filter Algorithms"). The
/// `/Predictor` value only sets the encoder's choice; the decoder reads the
/// type byte of every row.
fn png_unpredict(data: &[u8], row_len: usize, bpp: usize) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(data.len());
    let mut prev: Vec<u8> = Vec::new();
    let stride = row_len.saturating_add(1);
    for chunk in data.chunks(stride) {
        let Some((&filter_type, row)) = chunk.split_first() else {
            break;
        };
        let mut cur = Vec::with_capacity(row.len());
        for (i, &raw) in row.iter().enumerate() {
            let left = i
                .checked_sub(bpp)
                .and_then(|j| cur.get(j))
                .copied()
                .unwrap_or(0);
            let up = prev.get(i).copied().unwrap_or(0);
            let up_left = i
                .checked_sub(bpp)
                .and_then(|j| prev.get(j))
                .copied()
                .unwrap_or(0);
            let predicted = match filter_type {
                0 => 0,
                1 => left,
                2 => up,
                3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
                4 => paeth(left, up, up_left),
                other => {
                    return Err(Error::Filter {
                        filter: "Predictor".into(),
                        message: format!("unknown PNG filter type {other}"),
                    })
                }
            };
            cur.push(raw.wrapping_add(predicted));
        }
        out.extend_from_slice(&cur);
        prev = cur;
    }
    Ok(out)
}

/// Paeth predictor (PNG specification, filter type 4).
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let pa = (p - i16::from(a)).abs();
    let pb = (p - i16::from(b)).abs();
    let pc = (p - i16::from(c)).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// TIFF predictor 2: horizontal differencing, each sample is the difference
/// with the sample of the same component to its left (TIFF 6.0, section 14).
fn tiff_unpredict(data: &[u8], row_len: usize, p: Predictor) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let row_len = row_len.max(1);
    for row in data.chunks(row_len) {
        match p.bits_per_component {
            8 => {
                let mut cur: Vec<u8> = Vec::with_capacity(row.len());
                for (i, &raw) in row.iter().enumerate() {
                    let left = i
                        .checked_sub(p.colors)
                        .and_then(|j| cur.get(j))
                        .copied()
                        .unwrap_or(0);
                    cur.push(raw.wrapping_add(left));
                }
                out.extend_from_slice(&cur);
            }
            16 => {
                let mut cur: Vec<u16> = Vec::with_capacity(row.len() / 2);
                let mut pairs = row.chunks_exact(2);
                for pair in &mut pairs {
                    let raw = pair.iter().fold(0u16, |acc, &b| (acc << 8) | u16::from(b));
                    let left = cur
                        .len()
                        .checked_sub(p.colors)
                        .and_then(|j| cur.get(j))
                        .copied()
                        .unwrap_or(0);
                    cur.push(raw.wrapping_add(left));
                }
                out.extend(cur.iter().flat_map(|v| v.to_be_bytes()));
                // A dangling odd byte cannot be a sample: keep it as is.
                out.extend_from_slice(pairs.remainder());
            }
            bits => {
                // 1, 2 or 4 bits: unpack the samples, add, repack.
                let per_byte = 8 / bits;
                let mask = (1u8 << bits) - 1;
                let wanted = p.colors.saturating_mul(p.columns);
                let mut samples: Vec<u8> = Vec::with_capacity(row.len() * per_byte);
                for &byte in row {
                    for k in (0..per_byte).rev() {
                        if samples.len() == wanted {
                            break;
                        }
                        let raw = (byte >> (k * bits)) & mask;
                        let left = samples
                            .len()
                            .checked_sub(p.colors)
                            .and_then(|j| samples.get(j))
                            .copied()
                            .unwrap_or(0);
                        samples.push(raw.wrapping_add(left) & mask);
                    }
                }
                for group in samples.chunks(per_byte) {
                    let mut byte = 0u8;
                    for k in 0..per_byte {
                        let s = group.get(k).copied().unwrap_or(0);
                        byte |= s << ((per_byte - 1 - k) * bits);
                    }
                    out.push(byte);
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// ASCIIHexDecode (ISO 32000-2, 7.4.2)
// ---------------------------------------------------------------------------

/// Decode hexadecimal text up to the `>` end-of-data marker. Whitespace is
/// skipped; so is any other non-hex byte (tolerance, as pdf.js does). An odd
/// trailing digit is taken as the high nibble of a final byte.
pub fn ascii_hex_decode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 2);
    let mut high: Option<u8> = None;
    for &b in data {
        if b == b'>' {
            break;
        }
        let Some(digit) = hex_digit(b) else {
            continue;
        };
        match high.take() {
            None => high = Some(digit),
            Some(h) => out.push((h << 4) | digit),
        }
    }
    if let Some(h) = high {
        out.push(h << 4);
    }
    out
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// ASCII85Decode (ISO 32000-2, 7.4.3)
// ---------------------------------------------------------------------------

/// Decode base-85 text up to the `~>` end-of-data marker. Tolerates a
/// leading `<~` (the Adobe convention outside PDF) and a missing marker.
pub fn ascii85_decode(data: &[u8]) -> Result<Vec<u8>> {
    let bad = |message: &str| Error::Filter {
        filter: "ASCII85Decode".into(),
        message: message.into(),
    };
    let start = data
        .iter()
        .position(|&b| !is_whitespace(b))
        .unwrap_or(data.len());
    let body = data.get(start..).unwrap_or_default();
    let body = body.strip_prefix(b"<~").unwrap_or(body);
    let mut out = Vec::with_capacity(body.len() / 5 * 4);
    let mut acc: u32 = 0;
    let mut count = 0usize;
    for &b in body {
        match b {
            b'~' => break,
            b'z' if count == 0 => out.extend_from_slice(&[0, 0, 0, 0]),
            b'z' => return Err(bad("`z` inside a group")),
            b'!'..=b'u' => {
                acc = acc
                    .checked_mul(85)
                    .and_then(|v| v.checked_add(u32::from(b - b'!')))
                    .ok_or_else(|| bad("group value exceeds 2^32"))?;
                count += 1;
                if count == 5 {
                    out.extend_from_slice(&acc.to_be_bytes());
                    acc = 0;
                    count = 0;
                }
            }
            b if is_whitespace(b) => {}
            _ => return Err(bad("byte outside the `!`..`u` range")),
        }
    }
    // Final partial group: pad with `u`, keep count - 1 bytes.
    match count {
        0 => {}
        1 => return Err(bad("final group has a single character")),
        n => {
            for _ in n..5 {
                acc = acc
                    .checked_mul(85)
                    .and_then(|v| v.checked_add(84))
                    .ok_or_else(|| bad("group value exceeds 2^32"))?;
            }
            out.extend_from_slice(acc.to_be_bytes().get(..n - 1).unwrap_or_default());
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// RunLengthDecode (ISO 32000-2, 7.4.5)
// ---------------------------------------------------------------------------

/// Decode run-length data up to the 128 end-of-data byte. A run cut short
/// by the end of input yields what was there (tolerance). Output is bounded
/// by `limits.max_output`: a run byte expands 128-fold.
pub fn run_length_decode(data: &[u8], limits: DecodeLimits) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut bytes = data.iter().copied();
    while let Some(length) = bytes.next() {
        match length {
            128 => break,
            0..=127 => {
                let n = usize::from(length) + 1;
                out.extend(bytes.by_ref().take(n));
            }
            _ => {
                let Some(b) = bytes.next() else {
                    break;
                };
                let n = 257 - usize::from(length);
                out.extend(std::iter::repeat(b).take(n));
            }
        }
        if out.len() > limits.max_output {
            return Err(limit(limits, "RunLengthDecode output"));
        }
    }
    Ok(out)
}

fn limit(limits: DecodeLimits, what: &'static str) -> Error {
    Error::LimitExceeded {
        limit: limits.max_output,
        what,
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use std::io::Write;

    fn direct(o: &Object) -> Result<Object> {
        Ok(o.clone())
    }

    fn dict(src: &str) -> Dict {
        match Parser::new(src.as_bytes()).parse_object().expect("dict") {
            Object::Dict(d) => d,
            other => panic!("not a dict: {other:?}"),
        }
    }

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        enc.write_all(data).expect("compress");
        enc.finish().expect("finish")
    }

    const NO_LIMIT: DecodeLimits = DecodeLimits {
        max_output: usize::MAX,
    };

    // --- ASCIIHexDecode ---

    #[test]
    fn hex_known_vectors() {
        assert_eq!(ascii_hex_decode(b"48656C6C6F>"), b"Hello");
        assert_eq!(ascii_hex_decode(b"48 65\n6c 6C\t6f >"), b"Hello");
        // Odd digit count: last digit is the high nibble (7.4.2).
        assert_eq!(ascii_hex_decode(b"4>"), b"\x40");
        assert_eq!(ascii_hex_decode(b"414>"), b"\x41\x40");
        // No EOD marker, junk ignored.
        assert_eq!(ascii_hex_decode(b"41x42"), b"AB");
        assert_eq!(ascii_hex_decode(b""), b"");
        assert_eq!(ascii_hex_decode(b">"), b"");
    }

    // --- ASCII85Decode ---

    #[test]
    fn ascii85_known_vectors() {
        assert_eq!(
            ascii85_decode(b"87cURD_*#4DfTZ)~>").unwrap(),
            b"Hello, World"
        );
        // Full groups only.
        assert_eq!(ascii85_decode(b"87cURDZ~>").unwrap(), b"Hello");
        assert_eq!(ascii85_decode(b"87cURD_*#4DfTZ)").unwrap(), b"Hello, World");
        // `z` shorthand and whitespace anywhere.
        assert_eq!(ascii85_decode(b"z\n87cU\rR~>").unwrap(), b"\0\0\0\0Hell");
        assert_eq!(
            ascii85_decode(b"<~87cURD_*#4DfTZ)~>").unwrap(),
            b"Hello, World"
        );
        // Partial groups of 2, 3 and 4 characters.
        assert_eq!(ascii85_decode(b"5l~>").unwrap(), b"A");
        assert_eq!(ascii85_decode(b"5sb~>").unwrap(), b"AB");
        assert_eq!(ascii85_decode(b"5sdp~>").unwrap(), b"ABC");
        assert_eq!(ascii85_decode(b"~>").unwrap(), b"");
        // Max group value.
        assert_eq!(ascii85_decode(b"s8W-!~>").unwrap(), b"\xff\xff\xff\xff");
    }

    #[test]
    fn ascii85_errors() {
        assert!(matches!(
            ascii85_decode(b"87cUv~>"),
            Err(Error::Filter { .. })
        ));
        assert!(matches!(ascii85_decode(b"5~>"), Err(Error::Filter { .. })));
        assert!(matches!(
            ascii85_decode(b"87czR~>"),
            Err(Error::Filter { .. })
        ));
        // Group above 2^32 - 1.
        assert!(matches!(
            ascii85_decode(b"uuuuu~>"),
            Err(Error::Filter { .. })
        ));
    }

    // --- RunLengthDecode ---

    #[test]
    fn run_length_known_vectors() {
        // Literal run of 3, repeat `b` 4 times, EOD, ignored tail.
        let data = b"\x02abc\xfdb\x80zzz";
        assert_eq!(run_length_decode(data, NO_LIMIT).unwrap(), b"abcbbbb");
        assert_eq!(run_length_decode(b"", NO_LIMIT).unwrap(), b"");
        assert_eq!(run_length_decode(b"\x80", NO_LIMIT).unwrap(), b"");
        // Truncated literal run: what was there.
        assert_eq!(run_length_decode(b"\x05ab", NO_LIMIT).unwrap(), b"ab");
        // Truncated repeat run: nothing to repeat.
        assert_eq!(run_length_decode(b"\xfd", NO_LIMIT).unwrap(), b"");
        // Longest runs.
        let long = run_length_decode(b"\x81x", NO_LIMIT).unwrap();
        assert_eq!(long, vec![b'x'; 128]);
    }

    #[test]
    fn run_length_respects_limit() {
        let limits = DecodeLimits { max_output: 200 };
        // 128 + 49 = 177 bytes: under the limit.
        assert!(run_length_decode(b"\x81x\xd0y", limits).is_ok());
        assert_eq!(
            run_length_decode(b"\x81x\x81y", limits),
            Err(Error::LimitExceeded {
                limit: 200,
                what: "RunLengthDecode output"
            })
        );
    }

    // --- FlateDecode ---

    #[test]
    fn flate_round_trip_and_tolerances() {
        let text = b"Hello, Flate! Hello, Flate! Hello, Flate!";
        let z = zlib(text);
        assert_eq!(flate_decode(&z, NO_LIMIT).unwrap(), text);
        // Known vector: zlib of "hello" with default settings.
        let known = b"\x78\x9c\xcb\x48\xcd\xc9\xc9\x07\x00\x06\x2c\x02\x15";
        assert_eq!(flate_decode(known, NO_LIMIT).unwrap(), b"hello");
        // Leading whitespace before the header.
        let mut padded = b"\r\n ".to_vec();
        padded.extend_from_slice(&z);
        assert_eq!(flate_decode(&padded, NO_LIMIT).unwrap(), text);
        // Truncated tail: the data decoded so far.
        let cut = &z[..z.len() - 4];
        let partial = flate_decode(cut, NO_LIMIT).unwrap();
        assert!(text.starts_with(&partial), "{partial:?}");
        // Raw deflate without zlib header.
        let raw = &z[2..z.len() - 4];
        assert_eq!(flate_decode(raw, NO_LIMIT).unwrap(), text);
        assert_eq!(flate_decode(b"", NO_LIMIT).unwrap(), b"");
        assert!(matches!(
            flate_decode(b"\xff\xff\xff\xff", NO_LIMIT),
            Err(Error::Filter { .. })
        ));
    }

    #[test]
    fn flate_bomb_hits_the_limit() {
        let zeros = vec![0u8; 1 << 20];
        let bomb = zlib(&zeros);
        assert!(bomb.len() < 2048, "compressed size {}", bomb.len());
        let limits = DecodeLimits { max_output: 4096 };
        assert_eq!(
            flate_decode(&bomb, limits),
            Err(Error::LimitExceeded {
                limit: 4096,
                what: "inflated FlateDecode data"
            })
        );
        let exact = DecodeLimits {
            max_output: zeros.len(),
        };
        assert_eq!(flate_decode(&bomb, exact).unwrap().len(), zeros.len());
        assert_eq!(DecodeLimits::default().max_output, 256 * 1024 * 1024);
    }

    // --- Predictors ---

    fn png(rows: &[&[u8]]) -> Vec<u8> {
        rows.concat()
    }

    #[test]
    fn png_predictor_all_filter_types() {
        // 2 columns, 1 colour, 8 bits: rows of 2 bytes, bpp = 1.
        let p = Predictor {
            predictor: 12,
            colors: 1,
            bits_per_component: 8,
            columns: 2,
        };
        let data = png(&[
            &[0, 10, 20], // None
            &[1, 5, 3],   // Sub: 5, 8
            &[2, 1, 2],   // Up: 6, 10
            &[3, 4, 251], // Average: (0+6)/2+4 = 7, (7+10)/2+251 = 8+251 = 3
            &[4, 2, 250], // Paeth: left 0 up 7 ul 0 -> 7+2 = 9; a=9 b=3 c=7 -> p=5 pa=4 pb=2 pc=2 -> b=3 -> 253
        ]);
        assert_eq!(
            apply_predictor(&data, p).unwrap(),
            vec![10, 20, 5, 8, 6, 10, 7, 3, 9, 253]
        );
    }

    #[test]
    fn png_predictor_multi_byte_pixels() {
        // 2 colours × 8 bits: bpp = 2, so Sub looks 2 bytes back.
        let p = Predictor {
            predictor: 11,
            colors: 2,
            bits_per_component: 8,
            columns: 2,
        };
        let data = png(&[&[1, 1, 2, 3, 4]]);
        assert_eq!(apply_predictor(&data, p).unwrap(), vec![1, 2, 4, 6]);
        // 16-bit single colour: bpp = 2 as well.
        let p16 = Predictor {
            bits_per_component: 16,
            colors: 1,
            ..p
        };
        assert_eq!(apply_predictor(&data, p16).unwrap(), vec![1, 2, 4, 6]);
        // 1-bit, 12 columns: rows of 2 bytes, bpp = 1.
        let p1 = Predictor {
            bits_per_component: 1,
            colors: 1,
            columns: 12,
            predictor: 10,
        };
        assert_eq!(
            apply_predictor(&[1, 0x0f, 0x01], p1).unwrap(),
            vec![0x0f, 0x10]
        );
    }

    #[test]
    fn png_predictor_edge_cases() {
        let p = Predictor {
            predictor: 12,
            colors: 1,
            bits_per_component: 8,
            columns: 3,
        };
        // Short last row: decoded as far as it goes.
        assert_eq!(
            apply_predictor(&[2, 1, 2, 3, 2, 1], p).unwrap(),
            vec![1, 2, 3, 2]
        );
        assert_eq!(apply_predictor(&[], p).unwrap(), Vec::<u8>::new());
        assert!(matches!(
            apply_predictor(&[9, 1, 2, 3], p),
            Err(Error::Filter { .. })
        ));
        let huge = Predictor {
            columns: usize::MAX,
            ..p
        };
        assert!(matches!(
            apply_predictor(&[2, 1], huge),
            Err(Error::Filter { .. })
        ));
        let none = Predictor { predictor: 1, ..p };
        assert_eq!(apply_predictor(&[2, 1], none).unwrap(), vec![2, 1]);
    }

    #[test]
    fn tiff_predictor_8_16_and_sub_byte() {
        let p8 = Predictor {
            predictor: 2,
            colors: 2,
            bits_per_component: 8,
            columns: 3,
        };
        // Two rows of 3 RG pixels.
        let data = [10, 20, 1, 1, 1, 1, 5, 5, 250, 250, 1, 1];
        assert_eq!(
            apply_predictor(&data, p8).unwrap(),
            vec![10, 20, 11, 21, 12, 22, 5, 5, 255, 255, 0, 0]
        );
        let p16 = Predictor {
            colors: 1,
            bits_per_component: 16,
            columns: 3,
            predictor: 2,
        };
        let data = [0x01, 0x00, 0x00, 0xff, 0xff, 0x01];
        assert_eq!(
            apply_predictor(&data, p16).unwrap(),
            vec![0x01, 0x00, 0x01, 0xff, 0x01, 0x00]
        );
        let p4 = Predictor {
            colors: 1,
            bits_per_component: 4,
            columns: 4,
            predictor: 2,
        };
        // Samples 1,1,1,1 -> 1,2,3,4 ; next row 15,1,0,0 -> 15,0,0,0.
        assert_eq!(
            apply_predictor(&[0x11, 0x11, 0xf1, 0x00], p4).unwrap(),
            vec![0x12, 0x34, 0xf0, 0x00]
        );
        let p1 = Predictor {
            colors: 1,
            bits_per_component: 1,
            columns: 8,
            predictor: 2,
        };
        // 1 0 0 0 0 0 0 0 -> all ones (running xor-like add mod 2).
        assert_eq!(apply_predictor(&[0x80], p1).unwrap(), vec![0xff]);
        // Odd trailing byte with 16-bit samples is kept.
        assert_eq!(
            apply_predictor(&[0x00, 0x01, 0x07], p16).unwrap(),
            vec![0x00, 0x01, 0x07]
        );
    }

    // --- decode_stream ---

    #[test]
    fn decode_stream_chain_and_parms() {
        let text = b"chained filters";
        let hex: Vec<u8> = zlib(text)
            .iter()
            .flat_map(|b| format!("{b:02X}").into_bytes())
            .chain(b">".iter().copied())
            .collect();
        let d = dict("<< /Filter [/ASCIIHexDecode /FlateDecode] >>");
        assert_eq!(decode_stream(&d, &hex, direct).unwrap(), text);
        // Single name, no parms.
        let d = dict("<< /Filter /ASCIIHexDecode >>");
        assert_eq!(decode_stream(&d, b"41 42>", direct).unwrap(), b"AB");
        // No filter: identity.
        let d = dict("<< /Length 3 >>");
        assert_eq!(decode_stream(&d, b"raw", direct).unwrap(), b"raw");
        let d = dict("<< /Filter null >>");
        assert_eq!(decode_stream(&d, b"raw", direct).unwrap(), b"raw");
        // Flate + PNG Up predictor through /DecodeParms.
        let rows = png(&[&[2, 1, 2], &[2, 1, 1]]);
        let d = dict("<< /Filter /FlateDecode /DecodeParms << /Predictor 12 /Columns 2 >> >>");
        assert_eq!(
            decode_stream(&d, &zlib(&rows), direct).unwrap(),
            vec![1, 2, 2, 3]
        );
        // Array of parms with null for the hex stage.
        let d = dict("<< /Filter [/AHx /Fl] /DecodeParms [null << /Predictor 12 /Columns 2 >>] >>");
        let hex_rows: Vec<u8> = zlib(&rows)
            .iter()
            .flat_map(|b| format!("{b:02x}").into_bytes())
            .collect();
        assert_eq!(
            decode_stream(&d, &hex_rows, direct).unwrap(),
            vec![1, 2, 2, 3]
        );
        // Lone dict next to two filters goes to the Flate stage.
        let d = dict("<< /Filter [/AHx /Fl] /DecodeParms << /Predictor 12 /Columns 2 >> >>");
        assert_eq!(
            decode_stream(&d, &hex_rows, direct).unwrap(),
            vec![1, 2, 2, 3]
        );
        // Identity crypt filter.
        let d = dict("<< /Filter /Crypt /DecodeParms << /Name /Identity >> >>");
        assert_eq!(decode_stream(&d, b"same", direct).unwrap(), b"same");
    }

    #[test]
    fn decode_stream_resolves_references() {
        let d = dict("<< /Filter 9 0 R >>");
        let resolve = |o: &Object| match o {
            Object::Reference(_) => Ok(Object::Name(Name::new("ASCIIHexDecode"))),
            other => Ok(other.clone()),
        };
        assert_eq!(decode_stream(&d, b"41>", resolve).unwrap(), b"A");
        // With the direct resolver a reference stays a reference: error.
        assert!(matches!(
            decode_stream(&d, b"41>", direct),
            Err(Error::Filter { .. })
        ));
    }

    #[test]
    fn decode_stream_unknown_and_unsupported() {
        let d = dict("<< /Filter /LZWDecode >>");
        assert!(matches!(
            decode_stream(&d, b"", direct),
            Err(Error::Unsupported { .. })
        ));
        let d = dict("<< /Filter /DCTDecode >>");
        assert!(matches!(
            decode_stream(&d, b"", direct),
            Err(Error::Unsupported { .. })
        ));
        // A named crypt filter is applied by `Document` when the object is
        // read (see `encryption`); here it is the identity (7.4.10).
        let d = dict("<< /Filter /Crypt /DecodeParms << /Name /StdCF >> >>");
        assert_eq!(decode_stream(&d, b"raw", direct).unwrap(), b"raw");
        let d = dict("<< /Filter /Bogus >>");
        assert!(matches!(
            decode_stream(&d, b"", direct),
            Err(Error::Filter { .. })
        ));
        let d = dict("<< /Filter 42 >>");
        assert!(matches!(
            decode_stream(&d, b"", direct),
            Err(Error::Filter { .. })
        ));
        let d = dict("<< /Filter /FlateDecode /DecodeParms << /Predictor 12 /Columns -1 >> >>");
        assert!(matches!(
            decode_stream(&d, &zlib(b"x"), direct),
            Err(Error::Filter { .. })
        ));
        let d = dict("<< /Filter /FlateDecode /DecodeParms << /Predictor 7 >> >>");
        assert!(matches!(
            decode_stream(&d, &zlib(b"x"), direct),
            Err(Error::Filter { .. })
        ));
    }

    #[test]
    fn decode_stream_limit() {
        let d = dict("<< /Filter /FlateDecode >>");
        let bomb = zlib(&vec![0u8; 1 << 20]);
        let limits = DecodeLimits { max_output: 1000 };
        assert!(matches!(
            decode_stream_with(&d, &bomb, direct, limits),
            Err(Error::LimitExceeded { limit: 1000, .. })
        ));
    }
}
