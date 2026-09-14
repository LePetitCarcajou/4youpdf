//! The hayro engine as the bench runs it: a program that answers the
//! protocol, draws with hayro the documents `fyp-core` opens, encodes the PNG
//! as the application's page service does and names what it draws with.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use fyp_render_bench::protocol::{PageRequest, Reply, Request, PROTOCOL};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap()
        .to_path_buf()
}

fn fixture(name: &str) -> PathBuf {
    root().join("tests").join("fixtures").join(name)
}

/// A directory of its own for the images of one test.
fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fyp-render-engine-hayro-{test}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A request for the first page of `document`, written to `output`.
fn first_page(document: PathBuf, password: &str, width: u32, output: PathBuf) -> Request {
    Request {
        protocol: PROTOCOL,
        document,
        password: password.to_string(),
        width,
        repeat: 1,
        pages: vec![PageRequest { index: 0, output }],
    }
}

/// The replies of the engine to `request`, and whether it exited with success.
fn ask(request: &Request) -> (Vec<Reply>, bool) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fyp-render-engine-hayro"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(serde_json::to_string(request).unwrap().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let replies = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    (replies, output.status.success())
}

#[test]
fn answers_the_protocol() {
    let dir = scratch("protocol");
    let request = Request {
        protocol: PROTOCOL,
        document: fixture("encrypted-rc4.pdf"),
        password: String::new(),
        width: 120,
        repeat: 2,
        pages: vec![
            PageRequest {
                index: 0,
                output: dir.join("0.png"),
            },
            PageRequest {
                index: 9,
                output: dir.join("9.png"),
            },
        ],
    };

    let (replies, success) = ask(&Request {
        protocol: PROTOCOL + 1,
        ..request.clone()
    });
    assert!(!success);
    assert!(
        matches!(&replies[..], [Reply::Fatal { error }] if error.contains("protocole")),
        "{replies:?}"
    );

    let (replies, success) = ask(&request);
    assert!(success);
    let [Reply::Engine { protocol, name, .. }, Reply::Opened { .. }, Reply::Page {
        index: 0,
        width: 120,
        height,
        render_ms,
        encode_ms,
        identical_repeats: true,
    }, Reply::PageFailed { index: 9, .. }] = &replies[..]
    else {
        panic!("{replies:?}");
    };
    assert_eq!((*protocol, name.as_str()), (PROTOCOL, "hayro"));
    // A4, 120 pixels wide: 842 × 120 / 595 = 169.8, rounded as PDFium does.
    assert_eq!(*height, 170);
    assert_eq!((render_ms.len(), encode_ms.len()), (2, 2));
    let image = image::open(dir.join("0.png")).unwrap();
    assert_eq!((image.width(), image.height()), (120, 170));
    assert!(!dir.join("9.png").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The password is the core's to check: a wrong one is refused by the core
/// before hayro reads anything, and the user password (empty here) and the
/// owner password give the same page, drawn from the core's rewrite in the
/// clear.
#[test]
fn the_core_deciphers_what_hayro_draws() {
    let dir = scratch("password");
    let document = fixture("encrypted-aes256.pdf");
    let (replies, success) = ask(&first_page(
        document.clone(),
        "nope",
        120,
        dir.join("wrong.png"),
    ));
    assert!(success);
    assert!(
        matches!(&replies[..], [Reply::Engine { .. }, Reply::OpenFailed { error }] if error.contains("noyau")),
        "{replies:?}"
    );
    assert!(!dir.join("wrong.png").exists());
    for (password, name) in [("", "user.png"), ("owner", "owner.png")] {
        let (replies, success) = ask(&first_page(document.clone(), password, 120, dir.join(name)));
        assert!(success);
        assert!(
            matches!(
                &replies[..],
                [
                    Reply::Engine { .. },
                    Reply::Opened { .. },
                    Reply::Page { index: 0, .. }
                ]
            ),
            "{password:?} : {replies:?}"
        );
    }
    assert_eq!(
        std::fs::read(dir.join("user.png")).unwrap(),
        std::fs::read(dir.join("owner.png")).unwrap()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The PNG is the one the page service would make of the same pixels: the
/// settings of `encode_png` in app/src/render.rs, which the engine copies.
#[test]
fn encodes_the_png_as_the_page_service_does() {
    let service =
        std::fs::read_to_string(root().join("app").join("src").join("render.rs")).unwrap();
    for setting in ["CompressionType::Fast", "FilterType::Up"] {
        assert!(
            service.contains(setting),
            "app/src/render.rs n'encode plus avec {setting} : src/render.rs doit le suivre"
        );
    }
    let dir = scratch("png");
    let output = dir.join("0.png");
    let (replies, success) = ask(&first_page(fixture("minimal.pdf"), "", 300, output.clone()));
    assert!(success, "{replies:?}");
    let written = std::fs::read(&output).unwrap();
    let mut again = Vec::new();
    image::load_from_memory(&written)
        .unwrap()
        .write_with_encoder(PngEncoder::new_with_quality(
            &mut again,
            CompressionType::Fast,
            FilterType::Up,
        ))
        .unwrap();
    assert!(written == again, "the engine encodes otherwise");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The page service encodes with this crate: the same version and features,
/// or the encoding times would not compare.
#[test]
fn encodes_with_the_image_crate_of_the_application() {
    let manifest = |path: PathBuf| -> toml::Table {
        toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    };
    let app = manifest(root().join("app").join("Cargo.toml"));
    let engine = manifest(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"));
    assert_eq!(
        app["dependencies"]["image"],
        engine["dependencies"]["image"]
    );
}

/// The engine names the versions of the hayro crates that Cargo.lock
/// resolves: a report says what it measured.
#[test]
fn names_the_versions_it_draws_with() {
    let lock: toml::Table =
        toml::from_str(&std::fs::read_to_string(root().join("Cargo.lock")).unwrap()).unwrap();
    let version = |name: &str| -> String {
        let versions: Vec<&str> = lock["package"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|package| package["name"].as_str() == Some(name))
            .map(|package| package["version"].as_str().unwrap())
            .collect();
        assert_eq!(versions.len(), 1, "{name} : {versions:?}");
        versions[0].to_string()
    };
    let expected = format!(
        "hayro {}, hayro-interpret {}, hayro-syntax {}",
        version("hayro"),
        version("hayro-interpret"),
        version("hayro-syntax")
    );
    let dir = scratch("version");
    let (replies, success) = ask(&Request {
        protocol: PROTOCOL,
        document: dir.join("absent.pdf"),
        password: String::new(),
        width: 120,
        repeat: 1,
        pages: Vec::new(),
    });
    assert!(success);
    let [Reply::Engine { version, .. }, Reply::OpenFailed { .. }] = &replies[..] else {
        panic!("{replies:?}");
    };
    assert_eq!(version, &expected);
    let _ = std::fs::remove_dir_all(&dir);
}
