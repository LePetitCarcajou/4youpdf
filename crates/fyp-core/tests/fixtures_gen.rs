//! Generators for the fixtures of `tests/fixtures/` that involve binary or
//! compressed data and cannot be typed by hand. They are deterministic:
//! running them again rewrites identical files.
//!
//! ```text
//! cargo test -p fyp-core --test fixtures_gen -- --ignored
//! ```
//!
//! Then commit the files. Offsets are computed while building, and the
//! tests of `document.rs` check them.

#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

const CATALOG: &str = "<< /Type /Catalog /Pages 2 0 R >>";
const PAGES: &str = "<< /Type /Pages /Kids [3 0 R] /Count 1 >>";
const PAGE: &str = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << >> >>";

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    enc.write_all(data).expect("compress");
    enc.finish().expect("finish")
}

/// `<< entries /Length n >> stream ... endstream`, the body of a stream object.
fn stream(entries: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {entries} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// Rows of a cross-reference stream with `/W [1 2 2]`.
fn xref_rows(entries: &[(u8, usize, u16)]) -> Vec<u8> {
    let mut out = Vec::new();
    for &(kind, field2, field3) in entries {
        out.push(kind);
        out.extend_from_slice(
            &u16::try_from(field2)
                .expect("fits in 2 bytes")
                .to_be_bytes(),
        );
        out.extend_from_slice(&field3.to_be_bytes());
    }
    out
}

/// PNG "Up" predictor (type 2) applied to every row: `/Predictor 12`.
fn png_up(rows: &[u8], columns: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut prev = vec![0u8; columns];
    for row in rows.chunks(columns) {
        out.push(2);
        for (i, &b) in row.iter().enumerate() {
            out.push(b.wrapping_sub(prev[i]));
        }
        prev = row.to_vec();
    }
    out
}

/// Content of an object stream and its `/First`.
fn object_stream_content(objects: &[(u32, &str)]) -> (Vec<u8>, usize) {
    let mut header = String::new();
    let mut body = String::new();
    for (num, src) in objects {
        header.push_str(&format!("{num} {} ", body.len()));
        body.push_str(src);
        body.push('\n');
    }
    let first = header.len() + 1;
    (format!("{header}\n{body}").into_bytes(), first)
}

struct Builder {
    out: Vec<u8>,
    offsets: BTreeMap<u32, usize>,
}

impl Builder {
    /// Header line, then the conventional comment with four bytes above
    /// 127 (7.5.2).
    fn new(version: &str) -> Self {
        let mut out = format!("%PDF-{version}\n%").into_bytes();
        out.extend_from_slice(&[0xe2, 0xe3, 0xcf, 0xd3, b'\n']);
        Builder {
            out,
            offsets: BTreeMap::new(),
        }
    }

    fn object(&mut self, num: u32, body: &[u8]) {
        self.offsets.insert(num, self.out.len());
        self.out
            .extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        self.out.extend_from_slice(body);
        self.out.extend_from_slice(b"\nendobj\n");
    }

    fn offset(&self, num: u32) -> usize {
        *self.offsets.get(&num).expect("object written")
    }

    fn finish(mut self, startxref: usize, name: &str) {
        self.out
            .extend_from_slice(format!("startxref\n{startxref}\n%%EOF\n").as_bytes());
        let path = fixture_path(name);
        std::fs::write(&path, &self.out).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        println!(
            "{}: {} bytes, offsets {:?}",
            name,
            self.out.len(),
            self.offsets
        );
    }
}

/// `xrefstream.pdf`: the three objects of `minimal.pdf` and an
/// uncompressed cross-reference stream (7.5.8), `/W [1 2 2]`.
#[test]
#[ignore = "rewrites tests/fixtures/xrefstream.pdf"]
fn generate_xrefstream() {
    let mut b = Builder::new("1.5");
    b.object(1, CATALOG.as_bytes());
    b.object(2, PAGES.as_bytes());
    b.object(3, PAGE.as_bytes());
    let startxref = b.out.len();
    let rows = xref_rows(&[
        (0, 0, 65535),
        (1, b.offset(1), 0),
        (1, b.offset(2), 0),
        (1, b.offset(3), 0),
        (1, startxref, 0),
    ]);
    b.object(
        4,
        &stream("/Type /XRef /Size 5 /W [1 2 2] /Root 1 0 R", &rows),
    );
    b.finish(startxref, "xrefstream.pdf");
}

/// `objstm.pdf`: objects 1 and 2 in a Flate object stream (7.5.7), the
/// page as a plain object, and a Flate cross-reference stream with the
/// PNG Up predictor (`/Predictor 12`, `/Columns 5`).
#[test]
#[ignore = "rewrites tests/fixtures/objstm.pdf"]
fn generate_objstm() {
    let mut b = Builder::new("1.5");
    b.object(3, PAGE.as_bytes());
    let (content, first) = object_stream_content(&[(1, CATALOG), (2, PAGES)]);
    b.object(
        4,
        &stream(
            &format!("/Type /ObjStm /N 2 /First {first} /Filter /FlateDecode"),
            &zlib(&content),
        ),
    );
    let startxref = b.out.len();
    let rows = xref_rows(&[
        (0, 0, 65535),
        (2, 4, 0),
        (2, 4, 1),
        (1, b.offset(3), 0),
        (1, b.offset(4), 0),
        (1, startxref, 0),
    ]);
    b.object(
        5,
        &stream(
            "/Type /XRef /Size 6 /W [1 2 2] /Root 1 0 R /Filter /FlateDecode /DecodeParms << /Predictor 12 /Columns 5 >>",
            &zlib(&png_up(&rows, 5)),
        ),
    );
    b.finish(startxref, "objstm.pdf");
}

/// `hybrid.pdf`: a classic table lists objects 1 and 2; the page (object
/// 3) sits in an uncompressed object stream that only the `/XRefStm`
/// stream knows about (7.5.8.4). That stream is ASCIIHex-encoded so the
/// whole file stays readable in a text editor.
#[test]
#[ignore = "rewrites tests/fixtures/hybrid.pdf"]
fn generate_hybrid() {
    let mut b = Builder::new("1.4");
    b.object(1, CATALOG.as_bytes());
    b.object(2, PAGES.as_bytes());
    let (content, first) = object_stream_content(&[(3, PAGE)]);
    b.object(
        4,
        &stream(&format!("/Type /ObjStm /N 1 /First {first}"), &content),
    );
    let xref_stm = b.out.len();
    let rows = xref_rows(&[(2, 4, 0), (1, b.offset(4), 0)]);
    let hex: String = rows
        .chunks(5)
        .map(|row| row.iter().map(|b| format!("{b:02x}")).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
        + ">";
    b.object(
        5,
        &stream(
            "/Type /XRef /Size 6 /W [1 2 2] /Index [3 2] /Filter /ASCIIHexDecode",
            hex.as_bytes(),
        ),
    );
    let table = b.out.len();
    b.out.extend_from_slice(
        format!(
            "xref\n0 3\n0000000000 65535 f \n{:010} 00000 n \n{:010} 00000 n \ntrailer\n<< /Size 6 /Root 1 0 R /XRefStm {xref_stm} >>\n",
            b.offset(1),
            b.offset(2)
        )
        .as_bytes(),
    );
    b.finish(table, "hybrid.pdf");
}
