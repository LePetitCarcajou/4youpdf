//! The open document: its bytes, what `fyp-core` says about it, and the
//! two operations the window needs, listing pages and saving a new page
//! order through `fyp_core::ops`.

use std::path::Path;
use std::sync::Arc;

use fyp_core::document::Document;
use fyp_core::encryption::{Cipher, Encryption};
use fyp_core::object::{Name, Object};
use fyp_core::ops;
use serde::Serialize;

use crate::AppError;

/// What the interface shows about the open document.
#[derive(Debug, Clone, Serialize)]
pub struct DocumentInfo {
    /// Path as opened.
    pub path: String,
    /// File name alone, for the title bar.
    pub name: String,
    /// Size of the file in bytes.
    pub size: u64,
    /// Header version, e.g. `1.7`.
    pub version: String,
    /// One entry per page, in reading order.
    pub pages: Vec<PageInfo>,
    /// Why the cross-reference table was rebuilt by scanning, if it was
    /// (the file is damaged and was repaired on the way in).
    pub reconstructed: Option<String>,
    /// Offset where the table was found when `startxref` was wrong.
    pub relocated_startxref: Option<usize>,
    /// How the file is encrypted, in words, when it is. Saving produces a
    /// file in the clear.
    pub encryption: Option<String>,
}

/// Size and rotation of a page, for a placeholder of the right shape
/// before the thumbnail arrives.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct PageInfo {
    /// Width of the `/MediaBox` in points.
    pub width: f64,
    /// Height of the `/MediaBox` in points.
    pub height: f64,
    /// `/Rotate`, a multiple of 90 in `0..360`.
    pub rotate: i32,
}

/// What saving produced.
#[derive(Debug, Clone, Serialize)]
pub struct SaveReport {
    /// Path written.
    pub path: String,
    /// Size of the file written.
    pub size: u64,
    /// Pages in the file written.
    pub pages: usize,
}

/// An open file. The bytes are shared with the renderer.
#[derive(Debug)]
pub struct Session {
    /// Distinguishes documents for the renderer's cache.
    pub id: u64,
    pub bytes: Arc<Vec<u8>>,
    pub password: String,
    pub info: DocumentInfo,
}

impl Session {
    /// Read and open `path` with `password` (empty for most files).
    pub fn open(id: u64, path: &Path, password: &str) -> Result<Session, AppError> {
        let bytes = std::fs::read(path)
            .map_err(|e| AppError::other(format!("{} : {e}", path.display())))?;
        let info = describe(path, &bytes, password)?;
        Ok(Session {
            id,
            bytes: Arc::new(bytes),
            password: password.to_string(),
            info,
        })
    }

    /// The document, opened again from the bytes (cheap: the table is
    /// parsed, objects are read on demand).
    fn document(&self) -> Result<Document<'_>, AppError> {
        Document::open_with_password(&self.bytes, self.password.as_bytes()).map_err(AppError::from)
    }

    /// Write the pages at `order` (0-based indices into this document, in
    /// the wanted order) to `path`, through [`ops::extract_pages`]. The
    /// output is a clean, single-section file in the clear.
    pub fn save(&self, order: &[usize], path: &Path) -> Result<SaveReport, AppError> {
        let doc = self.document()?;
        let out = ops::extract_pages(&doc, order)?;
        // The result must open without repair before it replaces anything.
        let check = Document::open(&out)?;
        if let Some(reason) = check.reconstructed() {
            return Err(AppError::other(format!(
                "le fichier produit a dû être réparé à la relecture ({reason}) ; rien n'a été écrit"
            )));
        }
        std::fs::write(path, &out)
            .map_err(|e| AppError::other(format!("{} : {e}", path.display())))?;
        Ok(SaveReport {
            path: path.display().to_string(),
            size: out.len() as u64,
            pages: order.len(),
        })
    }
}

/// Open the bytes and gather what the interface shows.
pub fn describe(path: &Path, bytes: &[u8], password: &str) -> Result<DocumentInfo, AppError> {
    let doc = Document::open_with_password(bytes, password.as_bytes())?;
    let pages = ops::pages(&doc)?
        .iter()
        .map(|page| page_info(&doc, &page.dict))
        .collect();
    Ok(DocumentInfo {
        path: path.display().to_string(),
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        size: bytes.len() as u64,
        version: doc.version().to_string(),
        pages,
        reconstructed: doc.reconstructed().map(ToString::to_string),
        relocated_startxref: doc.relocated_startxref(),
        encryption: doc.encryption().map(|e| describe_encryption(&e)),
    })
}

/// Width, height and rotation of a page whose inheritable attributes are
/// already resolved. A missing or malformed `/MediaBox` gives Letter.
fn page_info(doc: &Document<'_>, page: &fyp_core::object::Dict) -> PageInfo {
    let number = |o: &Object| match doc.resolve(o) {
        Ok(Object::Integer(i)) => Some(i as f64),
        Ok(Object::Real(f)) => Some(f),
        _ => None,
    };
    let (width, height) = match page.get(&Name::new("MediaBox")).map(|m| doc.resolve(m)) {
        Some(Ok(Object::Array(items))) if items.len() == 4 => {
            let v: Vec<Option<f64>> = items.iter().map(number).collect();
            match (v[0], v[1], v[2], v[3]) {
                (Some(x0), Some(y0), Some(x1), Some(y1)) => ((x1 - x0).abs(), (y1 - y0).abs()),
                _ => (612.0, 792.0),
            }
        }
        _ => (612.0, 792.0),
    };
    let rotate = match page.get(&Name::new("Rotate")).map(|r| doc.resolve(r)) {
        Some(Ok(Object::Integer(i))) => i32::try_from(i).unwrap_or(0).rem_euclid(360) / 90 * 90,
        _ => 0,
    };
    let (width, height) = if width > 0.0 && height > 0.0 {
        (width, height)
    } else {
        (612.0, 792.0)
    };
    PageInfo {
        width,
        height,
        rotate,
    }
}

/// One line about how a file is protected, the same words as `fyp info`.
pub fn describe_encryption(e: &Encryption) -> String {
    let cipher = |c: Cipher| match c {
        Cipher::Identity => "en clair",
        Cipher::Rc4 => "RC4",
        Cipher::Aes128 => "AES-128",
        Cipher::Aes256 => "AES-256",
    };
    let ciphers = if e.streams == e.strings {
        cipher(e.streams).to_string()
    } else {
        format!("flux {}, chaînes {}", cipher(e.streams), cipher(e.strings))
    };
    let mut text = format!(
        "révision {}, {ciphers}, clé {} bits",
        e.revision.number(),
        e.key_bits
    );
    if !e.encrypt_metadata {
        text.push_str(", métadonnées en clair");
    }
    if e.owner {
        text.push_str(", ouvert avec le mot de passe propriétaire");
    }
    text
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures")
            .join(name)
    }

    #[test]
    fn minimal_fixture_is_described() {
        let session = Session::open(1, &fixture("minimal.pdf"), "").expect("open");
        let info = &session.info;
        assert_eq!(info.name, "minimal.pdf");
        assert_eq!(info.version, "1.7");
        assert_eq!(info.pages.len(), 1);
        assert_eq!(
            info.pages[0],
            PageInfo {
                width: 595.0,
                height: 842.0,
                rotate: 0
            }
        );
        assert_eq!(info.reconstructed, None);
        assert_eq!(info.encryption, None);
    }

    #[test]
    fn repaired_and_encrypted_files_are_flagged() {
        let repaired = Session::open(2, &fixture("bad-offsets.pdf"), "").expect("open");
        assert!(repaired.info.reconstructed.is_some());
        let encrypted = Session::open(3, &fixture("encrypted-rc4.pdf"), "").expect("open");
        assert_eq!(
            encrypted.info.encryption.as_deref(),
            Some("révision 3, RC4, clé 128 bits")
        );
        let relocated = Session::open(4, &fixture("startxref-off.pdf"), "").expect("open");
        assert_eq!(relocated.info.relocated_startxref, Some(209));
    }

    #[test]
    fn wrong_password_is_its_own_error() {
        let bytes = std::fs::read(fixture("encrypted-aes256.pdf")).unwrap();
        assert!(matches!(
            describe(Path::new("x.pdf"), &bytes, "nope"),
            Err(AppError::WrongPassword)
        ));
        assert!(describe(Path::new("x.pdf"), &bytes, "owner").is_ok());
        assert!(matches!(
            describe(Path::new("x.pdf"), b"not a pdf", ""),
            Err(AppError::Other { .. })
        ));
    }

    #[test]
    fn saving_reorders_and_drops_pages() {
        let session = Session::open(5, &fixture("objstm.pdf"), "").expect("open");
        let dir = std::env::temp_dir().join(format!("fyp-app-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("out.pdf");
        let report = session.save(&[0], &out).expect("save");
        assert_eq!(report.pages, 1);
        let bytes = std::fs::read(&out).unwrap();
        assert_eq!(report.size, bytes.len() as u64);
        let doc = Document::open(&bytes).unwrap();
        assert_eq!(doc.reconstructed(), None);
        assert_eq!(doc.page_count(), Ok(1));
        // A bad order is refused before anything is written.
        assert!(session.save(&[3], &dir.join("never.pdf")).is_err());
        assert!(!dir.join("never.pdf").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
