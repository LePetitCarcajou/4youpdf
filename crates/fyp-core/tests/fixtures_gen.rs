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

use fyp_crypto::{Cipher, Decryptor, Params, Revision};

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

/// `inuse-offset-zero.pdf`: `xrefstream.pdf` plus a row for an object 5
/// that does not exist, written as type 1 at offset 0 the way some
/// writers mark unused numbers (corpus: 22 pdf.js files). The reader must
/// take it as free instead of rejecting the table.
#[test]
#[ignore = "rewrites tests/fixtures/inuse-offset-zero.pdf"]
fn generate_inuse_offset_zero() {
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
        (1, 0, 0),
    ]);
    b.object(
        4,
        &stream("/Type /XRef /Size 6 /W [1 2 2] /Root 1 0 R", &rows),
    );
    b.finish(startxref, "inuse-offset-zero.pdf");
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

// ---------------------------------------------------------------------------
// Encrypted fixtures (ISO 32000-2, 7.6)
// ---------------------------------------------------------------------------

/// Content stream of the encrypted fixtures, before Flate and encryption.
const CONTENT: &[u8] = b"BT /F1 24 Tf 72 720 Td (Hello) Tj ET";
/// Page of the encrypted fixtures: `PAGE` plus a `/Contents` reference.
const PAGE_WITH_CONTENTS: &str =
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << >> /Contents 4 0 R >>";
/// First `/ID` string of the encrypted fixtures, part of the key material
/// for revisions 2 to 4.
const FILE_ID: [u8; 16] = [
    0x4f, 0x59, 0x50, 0x44, 0x46, 0x2d, 0x66, 0x69, 0x78, 0x74, 0x75, 0x72, 0x65, 0x2d, 0x69, 0x64,
];
/// Permissions: everything but the deprecated bits, the usual `-3904`.
const PERMISSIONS: u32 = 0xffff_f0c0;

fn hex(bytes: &[u8]) -> String {
    let digits: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("<{digits}>")
}

/// Objects 1 to 5 of an encrypted fixture: catalogue, pages, page, the
/// Flate content stream ciphered with `cipher`, and an `/Info` dictionary
/// whose `/Title` is ciphered. IVs are fixed so the output is stable.
fn encrypted_objects(b: &mut Builder, d: &Decryptor, cipher: Cipher, title: &str) {
    b.object(1, CATALOG.as_bytes());
    b.object(2, PAGES.as_bytes());
    b.object(3, PAGE_WITH_CONTENTS.as_bytes());
    let iv = |num: u32| [u8::try_from(num).expect("small"); 16];
    let data = d.encrypt_with(cipher, 4, 0, &iv(4), &zlib(CONTENT));
    b.object(4, &stream("/Filter /FlateDecode", &data));
    let title = d.encrypt_with(cipher, 5, 0, &iv(5), title.as_bytes());
    let producer = d.encrypt_with(cipher, 5, 0, &iv(5), b"4YouPDF fixtures");
    b.object(
        5,
        format!("<< /Title {} /Producer {} >>", hex(&title), hex(&producer)).as_bytes(),
    );
}

/// Classic table for objects 1..=6 and the trailer of an encrypted fixture.
fn encrypted_trailer(b: &mut Builder) -> usize {
    let table = b.out.len();
    let mut text = String::from("xref\n0 7\n0000000000 65535 f \n");
    for num in 1..=6 {
        text.push_str(&format!("{:010} 00000 n \n", b.offset(num)));
    }
    let id = hex(&FILE_ID);
    text.push_str(&format!(
        "trailer\n<< /Size 7 /Root 1 0 R /Info 5 0 R /Encrypt 6 0 R /ID [{id} {id}] >>\n"
    ));
    b.out.extend_from_slice(text.as_bytes());
    table
}

/// `encrypted-rc4.pdf`: revision 3, RC4 128-bit, empty user password,
/// owner password `owner` (7.6.4.3.2).
#[test]
#[ignore = "rewrites tests/fixtures/encrypted-rc4.pdf"]
fn generate_encrypted_rc4() {
    generate_rc4(b"", "Encrypted RC4 128-bit", "encrypted-rc4.pdf");
}

/// `encrypted-user-password.pdf`: the same file, whose user password is
/// `user`: without a password, it does not open.
#[test]
#[ignore = "rewrites tests/fixtures/encrypted-user-password.pdf"]
fn generate_encrypted_user_password() {
    generate_rc4(
        b"user",
        "Encrypted RC4 128-bit, user password",
        "encrypted-user-password.pdf",
    );
}

/// A revision 3, RC4 128-bit fixture with the user password `user` and
/// the owner password `owner`, titled `title`.
fn generate_rc4(user_password: &[u8], title: &str, name: &str) {
    let base = Params {
        revision: Revision::R3,
        key_bits: 128,
        owner: Vec::new(),
        user: Vec::new(),
        owner_key: Vec::new(),
        user_key: Vec::new(),
        permissions: PERMISSIONS,
        encrypt_metadata: true,
        streams: Cipher::Rc4,
        strings: Cipher::Rc4,
        file_id: FILE_ID.to_vec(),
    };
    let (owner, user) =
        fyp_crypto::legacy_owner_user(&base, b"owner", user_password).expect("values");
    let params = Params {
        owner: owner.clone(),
        user: user.clone(),
        ..base
    };
    let d = Decryptor::open(&params, user_password).expect("user password");
    let mut b = Builder::new("1.4");
    encrypted_objects(&mut b, &d, Cipher::Rc4, title);
    b.object(
        6,
        format!(
            "<< /Filter /Standard /V 2 /R 3 /Length 128 /P -3904 /O {} /U {} >>",
            hex(&owner),
            hex(&user)
        )
        .as_bytes(),
    );
    let table = encrypted_trailer(&mut b);
    b.finish(table, name);
}

/// `encrypted-aes256.pdf`: revision 6 (PDF 2.0), AES-256 through the
/// `/StdCF` crypt filter, empty user password, owner password `owner`
/// (7.6.4.3.3).
#[test]
#[ignore = "rewrites tests/fixtures/encrypted-aes256.pdf"]
fn generate_encrypted_aes256() {
    let file_key: [u8; 32] = *b"4YouPDF fixture file key, 32 B..";
    let salts = [[0x11; 8], [0x22; 8], [0x33; 8], [0x44; 8]];
    let e = fyp_crypto::aes256_entries(
        Revision::R6,
        &file_key,
        b"",
        b"owner",
        &salts,
        PERMISSIONS,
        true,
    );
    let params = Params {
        revision: Revision::R6,
        key_bits: 256,
        owner: e.owner.clone(),
        user: e.user.clone(),
        owner_key: e.owner_key.clone(),
        user_key: e.user_key.clone(),
        permissions: PERMISSIONS,
        encrypt_metadata: true,
        streams: Cipher::Aes256,
        strings: Cipher::Aes256,
        file_id: FILE_ID.to_vec(),
    };
    let d = Decryptor::open(&params, b"").expect("empty user password");
    let mut b = Builder::new("2.0");
    encrypted_objects(&mut b, &d, Cipher::Aes256, "Encrypted AES-256 (revision 6)");
    b.object(
        6,
        format!(
            "<< /Filter /Standard /V 5 /R 6 /Length 256 /P -3904 \
             /CF << /StdCF << /CFM /AESV3 /AuthEvent /DocOpen /Length 32 >> >> \
             /StmF /StdCF /StrF /StdCF /O {} /U {} /OE {} /UE {} /Perms {} >>",
            hex(&e.owner),
            hex(&e.user),
            hex(&e.owner_key),
            hex(&e.user_key),
            hex(&e.perms)
        )
        .as_bytes(),
    );
    let table = encrypted_trailer(&mut b);
    b.finish(table, "encrypted-aes256.pdf");
}

// ---------------------------------------------------------------------------
// Tolerances found in the corpus survey
// ---------------------------------------------------------------------------

/// `object-zero.pdf`: `minimal.pdf` plus a junk `0 0 obj` that the table
/// lists in use (corpus: qpdf `obj0.pdf`). Object 0 is the head of the
/// free list (7.5.4): the reader takes the entry as free and ignores the
/// object, without reconstruction.
#[test]
#[ignore = "rewrites tests/fixtures/object-zero.pdf"]
fn generate_object_zero() {
    let mut b = Builder::new("1.7");
    b.object(0, b"<< /Junk (never an object) >>");
    b.object(1, CATALOG.as_bytes());
    b.object(2, PAGES.as_bytes());
    b.object(3, PAGE.as_bytes());
    let table = b.out.len();
    let mut text = String::from("xref\n0 4\n");
    for num in 0..=3 {
        text.push_str(&format!("{:010} 00000 n \n", b.offset(num)));
    }
    text.push_str("trailer\n<< /Size 4 /Root 1 0 R >>\n");
    b.out.extend_from_slice(text.as_bytes());
    b.finish(table, "object-zero.pdf");
}

/// `root-direct.pdf`: the catalog is a direct dictionary in the trailer
/// (`/Root << ... >>`) instead of an indirect object (corpus: pdf.js
/// `issue9105_other.pdf`). Tolerated on reading; the writer makes it an
/// indirect object.
#[test]
#[ignore = "rewrites tests/fixtures/root-direct.pdf"]
fn generate_root_direct() {
    let mut b = Builder::new("1.4");
    b.object(2, PAGES.as_bytes());
    b.object(3, PAGE.as_bytes());
    let table = b.out.len();
    let text = format!(
        "xref\n0 4\n0000000000 65535 f \n0000000000 00001 f \n{:010} 00000 n \n{:010} 00000 n \ntrailer\n<< /Size 4 /Root {CATALOG} >>\n",
        b.offset(2),
        b.offset(3)
    );
    b.out.extend_from_slice(text.as_bytes());
    b.finish(table, "root-direct.pdf");
}

// ---------------------------------------------------------------------------
// Pages of mixed orientations
// ---------------------------------------------------------------------------

/// The twelve pages of `mixed12.pdf`: width, height, and `/Rotate` (0 for a
/// page that carries none). A4 portrait and landscape, one A4 of each
/// rotated by 90, Letter, and a 300 × 800 page taller than A4 for its width.
const MIXED12: [(u32, u32, u32); 12] = [
    (595, 842, 0),
    (842, 595, 0),
    (595, 842, 90),
    (612, 792, 0),
    (842, 595, 0),
    (595, 842, 0),
    (595, 842, 0),
    (842, 595, 90),
    (300, 800, 0),
    (595, 842, 0),
    (842, 595, 0),
    (595, 842, 0),
];

/// Content stream of a page of `mixed12.pdf`: a 40-point grid inset by 20
/// points, then the page number in Helvetica, so that the shape of a page
/// and its number are both readable on a thumbnail.
fn mixed12_content(number: usize, width: u32, height: u32) -> Vec<u8> {
    let mut lines = vec![String::from("0.6 w 0 0 0 RG")];
    let mut x = 40;
    while x < width {
        lines.push(format!("{x} 20 m {x} {} l S", height - 20));
        x += 40;
    }
    let mut y = 40;
    while y < height {
        lines.push(format!("20 {y} m {} {y} l S", width - 20));
        y += 40;
    }
    lines.push(format!(
        "BT /F1 28 Tf 40 {} Td (Page {number}) Tj ET",
        height - 60
    ));
    lines.join("\n").into_bytes()
}

/// `mixed12.pdf`: twelve pages mixing orientations, so that a row of the
/// thumbnail grid holds pages of different shapes (`tests/fixtures/README.md`).
/// Objects: the catalog, the page tree, the font, then a page (4, 6, 8, ...)
/// and its content stream (5, 7, 9, ...) per page.
#[test]
#[ignore = "rewrites tests/fixtures/mixed12.pdf"]
fn generate_mixed12() {
    let mut b = Builder::new("1.7");
    b.object(1, CATALOG.as_bytes());
    let kids: Vec<String> = (0..MIXED12.len())
        .map(|i| format!("{} 0 R", 4 + 2 * i))
        .collect();
    b.object(
        2,
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            MIXED12.len()
        )
        .as_bytes(),
    );
    b.object(3, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
    for (i, &(width, height, rotate)) in MIXED12.iter().enumerate() {
        let num = 4 + 2 * i as u32;
        let turn = if rotate == 0 {
            String::new()
        } else {
            format!(" /Rotate {rotate}")
        };
        b.object(
            num,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}]{turn} \
                 /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
                num + 1
            )
            .as_bytes(),
        );
        let content = mixed12_content(i + 1, width, height);
        let mut body = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        body.extend_from_slice(&content);
        body.extend_from_slice(b"\nendstream");
        b.object(num + 1, &body);
    }
    let objects = 3 + 2 * MIXED12.len() as u32;
    let table = b.out.len();
    let mut text = format!("xref\n0 {}\n0000000000 65535 f \n", objects + 1);
    for num in 1..=objects {
        text.push_str(&format!("{:010} 00000 n \n", b.offset(num)));
    }
    text.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\n",
        objects + 1
    ));
    b.out.extend_from_slice(text.as_bytes());
    b.finish(table, "mixed12.pdf");
}
