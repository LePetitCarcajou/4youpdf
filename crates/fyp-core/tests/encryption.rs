//! Encrypted files (ISO 32000-2, 7.6): the two encrypted fixtures are
//! deciphered transparently and rewritten in the clear; hostile `/Encrypt`
//! dictionaries and wrong passwords are errors, never panics.

#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;

use fyp_core::document::Document;
use fyp_core::encryption::{Cipher, Revision};
use fyp_core::object::{Name, ObjRef, Object};
use fyp_core::writer::{Writer, XrefStyle};
use fyp_core::Error;
use fyp_crypto::{Decryptor, Params};

const CONTENT: &[u8] = b"BT /F1 24 Tf 72 720 Td (Hello) Tj ET";
const INFO: ObjRef = ObjRef { num: 5, gen: 0 };
const CONTENTS: ObjRef = ObjRef { num: 4, gen: 0 };
const ENCRYPT: ObjRef = ObjRef { num: 6, gen: 0 };

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn title(doc: &Document<'_>) -> Vec<u8> {
    let info = doc.get(INFO).expect("read /Info").expect("listed");
    match info.as_dict().and_then(|d| d.get(&Name::new("Title"))) {
        Some(Object::String(s)) => s.clone(),
        other => panic!("expected a /Title string, got {other:?}"),
    }
}

fn content(doc: &Document<'_>) -> Vec<u8> {
    let stream = doc.get(CONTENTS).expect("read contents").expect("listed");
    doc.decoded(&stream).expect("decode contents")
}

/// The fixture opens with the empty password, reports its revision, and
/// hands out plaintext strings and streams; the owner password opens it
/// too; a wrong password is a clear error.
fn check_fixture(name: &str, revision: Revision, cipher: Cipher, key_bits: u32, title_text: &str) {
    let bytes = fixture(name);
    let doc = Document::open(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(doc.reconstructed(), None, "{name}");
    let e = doc
        .encryption()
        .unwrap_or_else(|| panic!("{name}: not seen as encrypted"));
    assert_eq!(e.revision, revision, "{name}");
    assert_eq!(e.streams, cipher, "{name}");
    assert_eq!(e.strings, cipher, "{name}");
    assert_eq!(e.key_bits, key_bits, "{name}");
    assert!(e.encrypt_metadata, "{name}");
    assert!(!e.owner, "{name}");
    assert_eq!(doc.page_count(), Ok(1), "{name}");
    assert_eq!(title(&doc), title_text.as_bytes(), "{name}");
    assert_eq!(content(&doc), CONTENT, "{name}");
    // The /Encrypt dictionary itself is handed out as stored.
    let enc = doc.get(ENCRYPT).unwrap().unwrap();
    match enc.as_dict().and_then(|d| d.get(&Name::new("O"))) {
        Some(Object::String(o)) => assert!(o.len() >= 32, "{name}"),
        other => panic!("{name}: /O became {other:?}"),
    }

    let owner =
        Document::open_with_password(&bytes, b"owner").unwrap_or_else(|e| panic!("{name}: {e}"));
    assert!(owner.encryption().unwrap().owner, "{name}");
    assert_eq!(title(&owner), title_text.as_bytes(), "{name}");

    let wrong = Document::open_with_password(&bytes, b"wrong").map(|_| ());
    assert_eq!(wrong, Err(Error::WrongPassword), "{name}");
    let wrong = Document::open_with_password(&bytes, b"ownerx").map(|_| ());
    assert_eq!(wrong, Err(Error::WrongPassword), "{name}");
}

#[test]
fn rc4_fixture_is_deciphered() {
    check_fixture(
        "encrypted-rc4.pdf",
        Revision::R3,
        Cipher::Rc4,
        128,
        "Encrypted RC4 128-bit",
    );
}

#[test]
fn aes256_fixture_is_deciphered() {
    check_fixture(
        "encrypted-aes256.pdf",
        Revision::R6,
        Cipher::Aes256,
        256,
        "Encrypted AES-256 (revision 6)",
    );
}

/// The writer produces a file in the clear: no `/Encrypt`, no security
/// handler dictionary, strings and streams readable by any reader.
#[test]
fn rewritten_files_are_in_the_clear() {
    for name in ["encrypted-rc4.pdf", "encrypted-aes256.pdf"] {
        let bytes = fixture(name);
        let doc = Document::open(&bytes).expect("open");
        for style in [XrefStyle::Table, XrefStyle::Stream] {
            let out = Writer::new(doc.version())
                .xref_style(style)
                .write(&doc)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let again = Document::open(&out).unwrap_or_else(|e| panic!("{name}: reopen: {e}"));
            assert_eq!(again.reconstructed(), None, "{name}");
            assert_eq!(again.encryption(), None, "{name}");
            assert!(
                !again.trailer().contains_key(&Name::new("Encrypt")),
                "{name}"
            );
            // Number 6 is free in the table style and reused by the
            // cross-reference stream in the stream style; never a dictionary.
            assert!(
                !matches!(again.get(ENCRYPT), Ok(Some(Object::Dict(_)))),
                "{name}: /Encrypt dictionary copied"
            );
            assert_eq!(title(&again), title(&doc), "{name}");
            assert_eq!(content(&again), CONTENT, "{name}");
            // Printable strings are written literally: the title is
            // visible in the bytes, which no encrypted file would allow.
            let literal = format!(
                "(Encrypted {}",
                if name.contains("rc4") { "RC4" } else { "AES" }
            );
            assert!(
                out.windows(literal.len()).any(|w| w == literal.as_bytes()),
                "{name}: title not in the clear"
            );
            if let Err(difference) = common::compare(&doc, &again) {
                panic!("{name} ({style:?}): {difference}");
            }
        }
    }
}

/// An encrypted file with a broken table is repaired by the scan and still
/// deciphered: the trailer found by the scan carries `/Encrypt`.
#[test]
fn repaired_encrypted_file_is_still_deciphered() {
    let bytes = fixture("encrypted-rc4.pdf");
    // Patched at byte level: the ciphered stream is binary.
    let marker = b"startxref\n671";
    let at = bytes
        .windows(marker.len())
        .position(|w| w == marker)
        .expect("startxref in the fixture");
    let mut broken = bytes.clone();
    broken[at + marker.len() - 3..at + marker.len()].copy_from_slice(b"600");
    let doc = Document::open(&broken).expect("open by scan");
    assert!(doc.reconstructed().is_some());
    assert_eq!(doc.encryption().map(|e| e.revision), Some(Revision::R3));
    assert_eq!(title(&doc), b"Encrypted RC4 128-bit");
    assert_eq!(content(&doc), CONTENT);
}

// ---------------------------------------------------------------------------
// Hostile /Encrypt dictionaries
// ---------------------------------------------------------------------------

const CATALOG: &str = "<< /Type /Catalog /Pages 2 0 R >>";
const PAGES: &str = "<< /Type /Pages /Kids [] /Count 0 >>";

/// One-section PDF whose objects are given as bodies for numbers 1..=n,
/// with `trailer_extra` added to the trailer.
fn build(objects: &[&str], trailer_extra: &str) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let startxref = out.len();
    let size = offsets.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {size} /Root 1 0 R {trailer_extra} >>\nstartxref\n{startxref}\n%%EOF\n"
        )
        .as_bytes(),
    );
    out
}

fn hex(bytes: &[u8]) -> String {
    format!(
        "<{}>",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    )
}

const ID: &str = "<0102030405060708090a0b0c0d0e0f10>";

/// A revision 3 `/Encrypt` dictionary whose `/O` and `/U` are computed
/// for the given passwords, with `extra` entries appended.
fn r3_dict(user_password: &[u8], owner_password: &[u8], extra: &str) -> String {
    let base = Params {
        revision: Revision::R3,
        key_bits: 128,
        owner: Vec::new(),
        user: Vec::new(),
        owner_key: Vec::new(),
        user_key: Vec::new(),
        permissions: 0xffff_fffc,
        encrypt_metadata: true,
        streams: Cipher::Rc4,
        strings: Cipher::Rc4,
        file_id: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
    };
    let (o, u) = fyp_crypto::legacy_owner_user(&base, owner_password, user_password).unwrap();
    format!(
        "<< /Filter /Standard /V 2 /R 3 /Length 128 /P -4 /O {} /U {} {extra} >>",
        hex(&o),
        hex(&u)
    )
}

#[test]
fn non_empty_user_password_is_required_and_accepted() {
    let base = Params {
        revision: Revision::R3,
        key_bits: 128,
        owner: Vec::new(),
        user: Vec::new(),
        owner_key: Vec::new(),
        user_key: Vec::new(),
        permissions: 0xffff_fffc,
        encrypt_metadata: true,
        streams: Cipher::Rc4,
        strings: Cipher::Rc4,
        file_id: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
    };
    let (o, u) = fyp_crypto::legacy_owner_user(&base, b"boss", b"secret").unwrap();
    let params = Params {
        owner: o,
        user: u,
        ..base
    };
    let d = Decryptor::open(&params, b"secret").unwrap();
    let title = d.encrypt_with(Cipher::Rc4, 3, 0, &[0; 16], b"Top secret");
    let info = format!("<< /Title {} >>", hex(&title));
    let encrypt = format!(
        "<< /Filter /Standard /V 2 /R 3 /Length 128 /P -4 /O {} /U {} >>",
        hex(&params.owner),
        hex(&params.user)
    );
    let file = build(
        &[CATALOG, PAGES, &info, &encrypt],
        &format!("/Info 3 0 R /Encrypt 4 0 R /ID [{ID} {ID}]"),
    );
    // Empty password: refused, clearly.
    let err = Document::open(&file).map(|_| ()).unwrap_err();
    assert_eq!(err, Error::WrongPassword);
    assert!(err.to_string().contains("password"), "{err}");
    // User password.
    let doc = Document::open_with_password(&file, b"secret").expect("user password");
    let info = doc.get(ObjRef { num: 3, gen: 0 }).unwrap().unwrap();
    assert_eq!(
        info.as_dict().unwrap().get(&Name::new("Title")),
        Some(&Object::String(b"Top secret".to_vec()))
    );
    assert!(!doc.encryption().unwrap().owner);
    // Owner password.
    let doc = Document::open_with_password(&file, b"boss").expect("owner password");
    assert!(doc.encryption().unwrap().owner);
    let info = doc.get(ObjRef { num: 3, gen: 0 }).unwrap().unwrap();
    assert_eq!(
        info.as_dict().unwrap().get(&Name::new("Title")),
        Some(&Object::String(b"Top secret".to_vec()))
    );
}

#[test]
fn malformed_encrypt_dictionaries_are_clear_errors() {
    let ok = r3_dict(b"", b"owner", "");
    let trailer = format!("/Encrypt 3 0 R /ID [{ID} {ID}]");
    assert!(Document::open(&build(&[CATALOG, PAGES, &ok], &trailer)).is_ok());

    let bad_encryption: Vec<(&str, String)> = vec![
        ("not a dictionary", "[1 2 3]".into()),
        ("a stream", "<< /Length 0 >>\nstream\n\nendstream".into()),
        ("unknown revision 9", ok.replace("/R 3", "/R 9")),
        ("revision 0", ok.replace("/R 3", "/R 0")),
        ("revision as a name", ok.replace("/R 3", "/R /Three")),
        ("no /R", ok.replace("/R 3", "")),
        (
            "no /O",
            r3_dict(b"", b"owner", "").replacen("/O <", "/X <", 1),
        ),
        (
            "no /U",
            r3_dict(b"", b"owner", "").replacen("/U <", "/X <", 1),
        ),
        ("no /P", ok.replace("/P -4", "")),
        ("/Length 7", ok.replace("/Length 128", "/Length 7")),
        ("/Length 256", ok.replace("/Length 128", "/Length 256")),
        ("/Length 0", ok.replace("/Length 128", "/Length 0")),
        ("/Length -128", ok.replace("/Length 128", "/Length -128")),
        (
            "/Length huge",
            ok.replace("/Length 128", "/Length 99999999999999999"),
        ),
        ("/O an integer", ok.replacen("/O <", "/O 7 /Y <", 1)),
        (
            "/CF not a dictionary",
            ok.replace("/R 3", "/R 4 /CF 12 /StmF /StdCF"),
        ),
        (
            "/StmF undefined",
            ok.replace("/R 3", "/R 4 /CF << >> /StmF /StdCF"),
        ),
        (
            "unknown /CFM",
            ok.replace(
                "/R 3",
                "/R 4 /CF << /StdCF << /CFM /Blowfish >> >> /StmF /StdCF",
            ),
        ),
        (
            "/EncryptMetadata not a boolean",
            ok.replace("/P -4", "/P -4 /EncryptMetadata 1"),
        ),
        (
            "revision 6 without /OE /UE",
            ok.replace("/R 3", "/R 6")
                .replace("/Length 128", "/Length 256"),
        ),
    ];
    for (what, dict) in bad_encryption {
        let file = build(&[CATALOG, PAGES, &dict], &trailer);
        let got = Document::open(&file).map(|_| ());
        assert!(
            matches!(got, Err(Error::BadEncryption { .. })),
            "{what}: {got:?}"
        );
        if let Err(e) = got {
            assert!(!e.to_string().is_empty());
        }
    }
    // Empty /O or /U: malformed. Short ones are padded (see below) and
    // garbage of any size is a wrong password.
    for (what, dict, expected) in [
        (
            "/O empty",
            format!(
                "<< /Filter /Standard /V 2 /R 3 /Length 128 /P -4 /O () /U {} >>",
                hex(&[2; 32])
            ),
            "malformed",
        ),
        (
            "/U empty",
            format!(
                "<< /Filter /Standard /V 2 /R 3 /Length 128 /P -4 /O {} /U () >>",
                hex(&[1; 32])
            ),
            "malformed",
        ),
        (
            "/O 31 bytes of garbage",
            format!(
                "<< /Filter /Standard /V 2 /R 3 /Length 128 /P -4 /O {} /U {} >>",
                hex(&[1; 31]),
                hex(&[2; 32])
            ),
            "password",
        ),
    ] {
        let file = build(&[CATALOG, PAGES, &dict], &trailer);
        let got = Document::open(&file).map(|_| ());
        match expected {
            "malformed" => assert!(
                matches!(got, Err(Error::BadEncryption { .. })),
                "{what}: {got:?}"
            ),
            _ => assert_eq!(got, Err(Error::WrongPassword), "{what}"),
        }
    }
    // Public-key handler: valid but unsupported.
    let pubsec = ok.replace("/Standard", "/Adobe.PubSec");
    let file = build(&[CATALOG, PAGES, &pubsec], &trailer);
    assert!(matches!(
        Document::open(&file).map(|_| ()),
        Err(Error::Unsupported { .. })
    ));
    // Right sizes, wrong values: a wrong password, not a malformed dictionary.
    let garbage = format!(
        "<< /Filter /Standard /V 2 /R 3 /Length 128 /P -4 /O {} /U {} >>",
        hex(&[1; 32]),
        hex(&[2; 32])
    );
    let file = build(&[CATALOG, PAGES, &garbage], &trailer);
    assert_eq!(Document::open(&file).map(|_| ()), Err(Error::WrongPassword));
    // /Encrypt pointing at nothing: the file is taken as stored in the clear.
    let file = build(&[CATALOG, PAGES], "/Encrypt 9 0 R");
    let doc = Document::open(&file).expect("open");
    assert_eq!(doc.encryption(), None);
    // A direct /Encrypt dictionary works as well as an indirect one.
    let file = build(&[CATALOG, PAGES], &format!("/Encrypt {ok} /ID [{ID} {ID}]"));
    let doc = Document::open(&file).expect("open");
    assert_eq!(doc.encryption().map(|e| e.revision), Some(Revision::R3));
}

/// `/Length` and `/CF` lengths that disagree with the algorithm are
/// reconciled the way readers do: AES-128 always has a 128-bit key, a
/// crypt filter length below 40 is in bytes.
#[test]
fn inconsistent_lengths_are_reconciled_or_refused() {
    // Revision 4, AES-128, /Length 40 at the top and 16 (bytes) in /CF.
    let base = Params {
        revision: Revision::R4,
        key_bits: 128,
        owner: Vec::new(),
        user: Vec::new(),
        owner_key: Vec::new(),
        user_key: Vec::new(),
        permissions: 0xffff_fffc,
        encrypt_metadata: true,
        streams: Cipher::Aes128,
        strings: Cipher::Aes128,
        file_id: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
    };
    let (o, u) = fyp_crypto::legacy_owner_user(&base, b"owner", b"").unwrap();
    let params = Params {
        owner: o,
        user: u,
        ..base
    };
    let d = Decryptor::open(&params, b"").unwrap();
    let title = d.encrypt_with(Cipher::Aes128, 3, 0, &[5; 16], b"AES title");
    let info = format!("<< /Title {} >>", hex(&title));
    let dict = format!(
        "<< /Filter /Standard /V 4 /R 4 /Length 40 /P -4 /O {} /U {} \
         /CF << /StdCF << /CFM /AESV2 /Length 16 >> >> /StmF /StdCF /StrF /StdCF >>",
        hex(&params.owner),
        hex(&params.user)
    );
    let file = build(
        &[CATALOG, PAGES, &info, &dict],
        &format!("/Info 3 0 R /Encrypt 4 0 R /ID [{ID} {ID}]"),
    );
    let doc = Document::open(&file).expect("open");
    let e = doc.encryption().unwrap();
    assert_eq!(e.key_bits, 128);
    assert_eq!(e.streams, Cipher::Aes128);
    let info = doc.get(ObjRef { num: 3, gen: 0 }).unwrap().unwrap();
    assert_eq!(
        info.as_dict().unwrap().get(&Name::new("Title")),
        Some(&Object::String(b"AES title".to_vec()))
    );
    // /U cut to its 16 significant bytes (corpus: qpdf/short-O-U.pdf) is
    // padded with zeros and still opens (algorithm 5 compares 16 bytes).
    let short_u = r3_dict(b"", b"owner", "");
    let u_start = short_u.find("/U <").unwrap() + 4;
    let mut cut = short_u.clone();
    cut.replace_range(u_start + 32..u_start + 64, "");
    assert_ne!(cut, short_u);
    let short_file = build(
        &[CATALOG, PAGES, &cut],
        &format!("/Encrypt 3 0 R /ID [{ID} {ID}]"),
    );
    let doc = Document::open(&short_file).expect("short /U opens");
    assert_eq!(doc.encryption().map(|e| e.revision), Some(Revision::R3));
    // RC4 crypt filter without its own /Length and a top-level /Length
    // nobody can use: refused.
    let text = String::from_utf8_lossy(&file)
        .replace("/Length 40 /P", "/Length 12 /P")
        .replace("/CFM /AESV2 /Length 16", "/CFM /V2");
    assert!(matches!(
        Document::open(text.as_bytes()).map(|_| ()),
        Err(Error::BadEncryption { message }) if message.contains("/Length 12")
    ));
}
