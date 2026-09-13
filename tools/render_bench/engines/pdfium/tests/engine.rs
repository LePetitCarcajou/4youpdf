//! The PDFium engine as the bench runs it: a program that answers the
//! protocol, with PDFium when it is fetched and with a fatal answer when it
//! is not, built from app/src/render.rs with the application's dependencies.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use fyp_render_bench::protocol::{PageRequest, Reply, Request, PROTOCOL};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap()
        .to_path_buf()
}

/// Whether the engine will find a library where it looks.
fn library_present() -> bool {
    let name = format!(
        "{}pdfium{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    std::env::var_os("FYP_PDFIUM_DIR")
        .map(PathBuf::from)
        .into_iter()
        .chain([root().join("app").join("pdfium")])
        .any(|dir| dir.join(&name).exists())
}

/// The replies of the engine to `request`, and whether it exited with success.
fn ask(request: &Request) -> (Vec<Reply>, bool) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fyp-render-engine-pdfium"))
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
    let dir = std::env::temp_dir().join(format!("fyp-render-engine-pdfium-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let request = Request {
        protocol: PROTOCOL,
        document: root()
            .join("tests")
            .join("fixtures")
            .join("encrypted-rc4.pdf"),
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
    if !library_present() {
        assert!(!success);
        assert!(
            matches!(&replies[..], [Reply::Fatal { error }] if error.contains("introuvable")),
            "{replies:?}"
        );
        eprintln!("PDFium not fetched: only the refusals were checked");
        return;
    }
    assert!(success);
    let [Reply::Engine {
        protocol,
        name,
        version,
        ..
    }, Reply::Opened { .. }, Reply::Page {
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
    assert_eq!((*protocol, name.as_str()), (PROTOCOL, "pdfium"));
    assert!(version.starts_with("chromium/"), "{version}");
    assert!(*height > 120, "A4 is taller than wide");
    assert_eq!((render_ms.len(), encode_ms.len()), (2, 2));
    let image = image::open(dir.join("0.png")).unwrap();
    assert_eq!((image.width(), image.height()), (120, *height));
    assert!(!dir.join("9.png").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// app/src/render.rs is compiled here: with the crates, versions and features
/// the application asks for, or the bench would measure another renderer.
#[test]
fn builds_the_renderer_with_the_dependencies_of_the_application() {
    let manifest = |path: PathBuf| -> toml::Table {
        toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    };
    let app = manifest(root().join("app").join("Cargo.toml"));
    let engine = manifest(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"));
    for dependency in ["pdfium-render", "image", "serde"] {
        assert_eq!(
            app["dependencies"][dependency], engine["dependencies"][dependency],
            "{dependency}"
        );
    }
}
