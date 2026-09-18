//! The open document: its bytes, what `fyp-core` says about it, and the
//! operations the window needs, all through `fyp_core::ops`: listing
//! pages, turning some of them, appending the pages of other files, saving
//! a new page order.

use std::path::{Path, PathBuf};
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
    /// Identifies this opening of the file. A command that changes the
    /// document names it, so that one meant for a document replaced
    /// meanwhile is refused instead of applied to the new one.
    pub document: u64,
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

/// What became of one file asked to be merged.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceOutcome {
    /// Its pages follow those of the document.
    Merged {
        /// How many pages it brought.
        pages: usize,
        /// Why its table was rebuilt by scanning, if it was.
        reconstructed: Option<String>,
        /// How it is encrypted, in words, when it is: its pages are
        /// merged in the clear.
        encryption: Option<String>,
    },
    /// Protected by a password, which merging does not ask for: skipped.
    Protected,
    /// Not read, or refused by the core even after repair: skipped.
    Refused {
        /// Why, worded for the user.
        message: String,
    },
}

/// One file asked to be merged, and what became of it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SourceReport {
    /// Path as given.
    pub path: String,
    /// File name alone, for the notices.
    pub name: String,
    pub outcome: SourceOutcome,
}

/// What merging produced: the document rewritten with the pages of the
/// files that opened after its own, when at least one did, and what
/// became of each file asked for.
#[derive(Debug)]
pub struct Merged {
    /// `None` when no file could be merged: the document is as it was.
    pub rewrite: Option<Rewrite>,
    /// Pages brought by the files merged, all together.
    pub added: usize,
    /// One entry per file asked for, in the order given.
    pub sources: Vec<SourceReport>,
}

/// What the interface is told after a merge.
#[derive(Debug, Clone, Serialize)]
pub struct MergeReport {
    /// Every page as it now stands, or `None` when no file could be
    /// merged and the document is as it was.
    pub pages: Option<Vec<PageInfo>>,
    pub sources: Vec<SourceReport>,
}

/// An open file, as it stands after the rotations and merges applied to
/// it: the bytes read at first, then the rewrite made by each rotation
/// ([`Session::replace`]) or merge ([`Session::extend`]). The bytes are
/// shared with the renderer.
#[derive(Debug)]
pub struct Session {
    /// Distinguishes the bytes for the renderer's cache: a new id whenever
    /// they change.
    pub id: u64,
    pub bytes: Arc<Vec<u8>>,
    /// Password of `bytes`: the one given when opening, empty once they
    /// are a rewrite, which is in the clear.
    pub password: String,
    /// What the interface was told when opening; `pages` follows the
    /// rotations and merges.
    pub info: DocumentInfo,
}

/// What a rotation or a merge produced: the whole document rewritten, and
/// its pages.
#[derive(Debug)]
pub struct Rewrite {
    /// Written by `fyp-core`, in the clear.
    pub bytes: Vec<u8>,
    /// Read back from `bytes`.
    pub pages: Vec<PageInfo>,
}

impl Session {
    /// Read and open `path` with `password` (empty for most files).
    pub fn open(id: u64, path: &Path, password: &str) -> Result<Session, AppError> {
        let bytes = std::fs::read(path)
            .map_err(|e| AppError::other(format!("{} : {e}", path.display())))?;
        let info = describe(id, path, &bytes, password)?;
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

    /// Make `rotated` the document from now on, known to the renderer as
    /// `id`, and return its pages. Refused when the page count changed:
    /// the page indices the interface holds must stay valid.
    pub fn replace(&mut self, id: u64, rotated: Rewrite) -> Result<Vec<PageInfo>, AppError> {
        if rotated.pages.len() != self.info.pages.len() {
            return Err(AppError::other(format!(
                "la rotation a produit {} pages au lieu de {} ; elle n'a pas été appliquée",
                rotated.pages.len(),
                self.info.pages.len()
            )));
        }
        Ok(self.take(id, rotated))
    }

    /// Make `merged` the document from now on, known to the renderer as
    /// `id`, and return its pages. Refused unless its pages are those of
    /// this document followed by `added` more: the page indices the
    /// interface holds must stay valid, and the pages of the files merged
    /// must all be there.
    pub fn extend(
        &mut self,
        id: u64,
        merged: Rewrite,
        added: usize,
    ) -> Result<Vec<PageInfo>, AppError> {
        let expected = self.info.pages.len().saturating_add(added);
        if merged.pages.len() != expected {
            return Err(AppError::other(format!(
                "la fusion a produit {} pages au lieu de {expected} ; elle n'a pas été appliquée",
                merged.pages.len()
            )));
        }
        Ok(self.take(id, merged))
    }

    /// `rewrite`, in the clear, becomes the document known as `id`.
    fn take(&mut self, id: u64, rewrite: Rewrite) -> Vec<PageInfo> {
        self.id = id;
        self.bytes = Arc::new(rewrite.bytes);
        self.password.clear();
        self.info.pages.clone_from(&rewrite.pages);
        rewrite.pages
    }
}

/// The document in `bytes`, opened with `password`, with the pages at
/// `pages` (0-based) turned by `degrees` clockwise through [`ops::rotate`]:
/// relative to the rotation of each page, inherited or not, normalised
/// into `0..360`. Like a saved file, the result must read back without
/// repair before it is returned.
pub fn rotate(
    bytes: &[u8],
    password: &str,
    pages: &[usize],
    degrees: i32,
) -> Result<Rewrite, AppError> {
    let doc = Document::open_with_password(bytes, password.as_bytes())?;
    let out = ops::rotate(&doc, pages, degrees)?;
    let pages = {
        let check = Document::open(&out)?;
        if let Some(reason) = check.reconstructed() {
            return Err(AppError::other(format!(
                "le document produit a dû être réparé à la relecture ({reason}) ; la rotation n'a pas été appliquée"
            )));
        }
        page_infos(&check)?
    };
    Ok(Rewrite { bytes: out, pages })
}

/// The document in `bytes`, opened with `password`, followed by every page
/// of each file of `files` that opens, in that order, through
/// [`ops::merge`]. The files are opened without a password: one that needs
/// a password, one that cannot be read, and one the core refuses even after
/// repair are each skipped and said so in the report, and the others are
/// merged without them; a file is checked to have pages before the merge,
/// so that the merge itself fails only for the lot. When no file opens,
/// nothing is rewritten. Like a rotation, the rewrite must read back
/// without repair before it is returned.
pub fn merge(bytes: &[u8], password: &str, files: &[PathBuf]) -> Result<Merged, AppError> {
    let doc = Document::open_with_password(bytes, password.as_bytes())?;
    let read: Vec<Result<Vec<u8>, String>> = files
        .iter()
        .map(|path| std::fs::read(path).map_err(|e| e.to_string()))
        .collect();
    let mut docs: Vec<Document<'_>> = vec![doc];
    let mut sources = Vec::with_capacity(files.len());
    let mut added = 0usize;
    for (path, content) in files.iter().zip(&read) {
        let outcome = match content {
            Err(e) => SourceOutcome::Refused { message: e.clone() },
            Ok(content) => match Document::open(content) {
                Err(fyp_core::Error::WrongPassword) => SourceOutcome::Protected,
                Err(e) => SourceOutcome::Refused {
                    message: e.to_string(),
                },
                Ok(other) => match ops::pages(&other) {
                    Err(e) => SourceOutcome::Refused {
                        message: e.to_string(),
                    },
                    Ok(pages) => {
                        added = added.saturating_add(pages.len());
                        let outcome = SourceOutcome::Merged {
                            pages: pages.len(),
                            reconstructed: other.reconstructed().map(ToString::to_string),
                            encryption: other.encryption().map(|e| describe_encryption(&e)),
                        };
                        docs.push(other);
                        outcome
                    }
                },
            },
        };
        sources.push(SourceReport {
            path: path.display().to_string(),
            name: file_name(path),
            outcome,
        });
    }
    if docs.len() == 1 {
        return Ok(Merged {
            rewrite: None,
            added: 0,
            sources,
        });
    }
    let out = ops::merge(&docs)?;
    let pages = {
        let check = Document::open(&out)?;
        if let Some(reason) = check.reconstructed() {
            return Err(AppError::other(format!(
                "le document produit a dû être réparé à la relecture ({reason}) ; la fusion n'a pas été appliquée"
            )));
        }
        page_infos(&check)?
    };
    Ok(Merged {
        rewrite: Some(Rewrite { bytes: out, pages }),
        added,
        sources,
    })
}

/// Open the bytes and gather what the interface shows about this opening,
/// `document`.
pub fn describe(
    document: u64,
    path: &Path,
    bytes: &[u8],
    password: &str,
) -> Result<DocumentInfo, AppError> {
    let doc = Document::open_with_password(bytes, password.as_bytes())?;
    let pages = page_infos(&doc)?;
    Ok(DocumentInfo {
        document,
        path: path.display().to_string(),
        name: file_name(path),
        size: bytes.len() as u64,
        version: doc.version().to_string(),
        pages,
        reconstructed: doc.reconstructed().map(ToString::to_string),
        relocated_startxref: doc.relocated_startxref(),
        encryption: doc.encryption().map(|e| describe_encryption(&e)),
    })
}

/// The last component of `path`; empty when it has none.
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Width, height and rotation of every page of `doc`, in reading order.
fn page_infos(doc: &Document<'_>) -> Result<Vec<PageInfo>, AppError> {
    Ok(ops::pages(doc)?
        .iter()
        .map(|page| page_info(doc, &page.dict))
        .collect())
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
            describe(1, Path::new("x.pdf"), &bytes, "nope"),
            Err(AppError::WrongPassword)
        ));
        assert!(describe(1, Path::new("x.pdf"), &bytes, "owner").is_ok());
        assert!(matches!(
            describe(1, Path::new("x.pdf"), b"not a pdf", ""),
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

    /// A one-section PDF whose `objects[i]` is object `i + 1`, the catalog
    /// first.
    fn hand_built(objects: &[&str]) -> Vec<u8> {
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
            format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{startxref}\n%%EOF\n")
                .as_bytes(),
        );
        out
    }

    /// Three A4 pages: the first inherits `/Rotate 90` from the page tree
    /// (ISO 32000-2, 7.7.3.4), the second says 180, the third -90.
    fn three_pages() -> Vec<u8> {
        hand_built(&[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 /MediaBox [0 0 595 842] /Rotate 90 >>",
            "<< /Type /Page /Parent 2 0 R >>",
            "<< /Type /Page /Parent 2 0 R /Rotate 180 >>",
            "<< /Type /Page /Parent 2 0 R /Rotate -90 >>",
        ])
    }

    fn rotations(pages: &[PageInfo]) -> Vec<i32> {
        pages.iter().map(|page| page.rotate).collect()
    }

    /// Turn `pages` of `session` by `degrees`, as the application does.
    fn turn(session: &mut Session, pages: &[usize], degrees: i32) -> Vec<i32> {
        let rotated = rotate(&session.bytes, &session.password, pages, degrees).expect("rotate");
        let id = session.id + 1;
        rotations(&session.replace(id, rotated).expect("replace"))
    }

    #[test]
    fn rotation_is_relative_to_each_page_and_normalised() {
        let dir = std::env::temp_dir().join(format!("fyp-app-rotate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("three.pdf");
        std::fs::write(&path, three_pages()).unwrap();
        let mut session = Session::open(10, &path, "").expect("open");
        assert_eq!(rotations(&session.info.pages), [90, 180, 270]);

        // Each page turns from its own rotation, inherited or not.
        assert_eq!(turn(&mut session, &[0, 2], 90), [180, 180, 0]);
        assert_eq!(session.id, 11);
        assert_eq!(rotations(&session.info.pages), [180, 180, 0]);
        // Counter-clockwise from 0 is 270, not -90.
        assert_eq!(turn(&mut session, &[1, 2], -90), [180, 90, 270]);
        // The size is that of the media box, whatever the rotation.
        assert_eq!(
            session.info.pages[1],
            PageInfo {
                width: 595.0,
                height: 842.0,
                rotate: 90
            }
        );
        // The opposite rotations bring back those of the file.
        assert_eq!(turn(&mut session, &[1, 2], 90), [180, 180, 0]);
        assert_eq!(turn(&mut session, &[0, 2], -90), [90, 180, 270]);

        // What is saved is the document as it stands, in the order asked.
        assert_eq!(turn(&mut session, &[0], 90), [180, 180, 270]);
        let out = dir.join("out.pdf");
        session.save(&[2, 0], &out).expect("save");
        let bytes = std::fs::read(&out).unwrap();
        let saved = Document::open(&bytes).unwrap();
        let key = Name::new("Rotate");
        let saved: Vec<Option<Object>> = ops::pages(&saved)
            .unwrap()
            .iter()
            .map(|page| page.dict.get(&key).cloned())
            .collect();
        assert_eq!(
            saved,
            [Some(Object::Integer(270)), Some(Object::Integer(180))]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Merge `files` into `session`, as the application does.
    fn append(session: &mut Session, files: &[PathBuf]) -> Merged {
        let mut merged = merge(&session.bytes, &session.password, files).expect("merge");
        if let Some(rewrite) = merged.rewrite.take() {
            let id = session.id + 1;
            let pages = session.extend(id, rewrite, merged.added).expect("extend");
            merged.rewrite = Some(Rewrite {
                bytes: Vec::new(),
                pages,
            });
        }
        merged
    }

    fn outcomes(merged: &Merged) -> Vec<&SourceOutcome> {
        merged.sources.iter().map(|s| &s.outcome).collect()
    }

    #[test]
    fn merging_appends_the_pages_of_the_files_that_open() {
        let mut session = Session::open(40, &fixture("minimal.pdf"), "").expect("open");
        let before = session.info.pages.clone();
        let merged = append(
            &mut session,
            &[fixture("objstm.pdf"), fixture("bad-offsets.pdf")],
        );
        assert_eq!(merged.added, 2);
        assert_eq!(
            merged
                .sources
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["objstm.pdf", "bad-offsets.pdf"]
        );
        // A damaged file merges, and the report says it was repaired.
        assert!(
            matches!(
                outcomes(&merged)[..],
                [
                    SourceOutcome::Merged {
                        pages: 1,
                        reconstructed: None,
                        encryption: None
                    },
                    SourceOutcome::Merged {
                        pages: 1,
                        reconstructed: Some(_),
                        encryption: None
                    }
                ]
            ),
            "{:?}",
            outcomes(&merged)
        );
        // The pages of the document come first, unchanged; the others
        // follow, and the document is a clean rewrite.
        assert_eq!(session.id, 41);
        assert_eq!(session.info.pages.len(), 3);
        assert_eq!(session.info.pages[0], before[0]);
        let doc = Document::open(&session.bytes).expect("open the rewrite");
        assert_eq!(doc.reconstructed(), None);
        assert_eq!(doc.page_count(), Ok(3));
        // A rotation applies to a merged page as to the others, and saving
        // takes the order the interface holds, merged pages in it or not.
        assert_eq!(turn(&mut session, &[2], 90), [0, 0, 90]);
        let dir = std::env::temp_dir().join(format!("fyp-app-merge-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("out.pdf");
        assert_eq!(session.save(&[2, 0], &out).expect("save").pages, 2);
        let saved = std::fs::read(&out).unwrap();
        assert_eq!(Document::open(&saved).unwrap().page_count(), Ok(2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_protected_or_refused_file_is_skipped_and_the_others_merged() {
        let mut session = Session::open(50, &fixture("minimal.pdf"), "").expect("open");
        let not_pdf = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let merged = append(
            &mut session,
            &[
                fixture("encrypted-user-password.pdf"),
                not_pdf,
                fixture("absent.pdf"),
                fixture("encrypted-rc4.pdf"),
            ],
        );
        assert!(
            matches!(
                outcomes(&merged)[..],
                [
                    SourceOutcome::Protected,
                    SourceOutcome::Refused { .. },
                    SourceOutcome::Refused { .. },
                    SourceOutcome::Merged {
                        pages: 1,
                        reconstructed: None,
                        encryption: Some(_)
                    }
                ]
            ),
            "{:?}",
            outcomes(&merged)
        );
        // Each refusal says why; the file with an empty user password is
        // merged, in the clear.
        for source in &merged.sources[1..3] {
            if let SourceOutcome::Refused { message } = &source.outcome {
                assert!(!message.is_empty(), "{}", source.path);
            }
        }
        assert_eq!(merged.added, 1);
        assert_eq!(session.info.pages.len(), 2);
        assert!(Document::open(&session.bytes)
            .expect("open")
            .encryption()
            .is_none());

        // Nothing merges: nothing is rewritten, and the report still says
        // why.
        let bytes = Arc::clone(&session.bytes);
        let none = append(&mut session, &[fixture("encrypted-user-password.pdf")]);
        assert!(none.rewrite.is_none());
        assert_eq!(none.added, 0);
        assert_eq!(outcomes(&none), [&SourceOutcome::Protected]);
        assert!(Arc::ptr_eq(&session.bytes, &bytes));

        // A rewrite whose pages do not add up is not taken.
        let other = merge(&session.bytes, "", &[fixture("minimal.pdf")]).expect("merge");
        let rewrite = other.rewrite.expect("rewritten");
        assert!(session.extend(52, rewrite, 5).is_err());
        assert_eq!(session.id, 51);
        assert_eq!(session.info.pages.len(), 2);
    }

    #[test]
    fn an_encrypted_document_is_extended_in_the_clear() {
        let mut session =
            Session::open(60, &fixture("encrypted-aes256.pdf"), "owner").expect("open");
        let merged = append(&mut session, &[fixture("minimal.pdf")]);
        assert_eq!(merged.added, 1);
        assert_eq!(session.password, "");
        let doc = Document::open(&session.bytes).expect("open the rewrite");
        assert!(doc.encryption().is_none());
        assert_eq!(doc.page_count(), Ok(2));
    }

    #[test]
    fn an_encrypted_document_is_turned_in_the_clear() {
        let mut session =
            Session::open(20, &fixture("encrypted-aes256.pdf"), "owner").expect("open");
        assert_eq!(turn(&mut session, &[0], 90), [90]);
        // The rewrite needs no password: the next rotation, the renderer
        // and saving take it as it is.
        assert_eq!(session.password, "");
        let doc = Document::open(&session.bytes).expect("open the rewrite");
        assert!(doc.encryption().is_none());
        assert_eq!(turn(&mut session, &[0], 90), [180]);
    }

    #[test]
    fn a_refused_rotation_changes_nothing() {
        let mut session = Session::open(30, &fixture("minimal.pdf"), "").expect("open");
        let before = Arc::clone(&session.bytes);
        // No second page, not a multiple of 90, a page given twice.
        for (pages, degrees) in [(&[1][..], 90), (&[0][..], 45), (&[0, 0][..], 90)] {
            assert!(
                matches!(
                    rotate(&session.bytes, "", pages, degrees),
                    Err(AppError::Other { .. })
                ),
                "{pages:?} by {degrees}"
            );
        }
        // A rewrite with another page count is not taken.
        let other = rotate(&three_pages(), "", &[0], 90).expect("rotate");
        assert!(session.replace(31, other).is_err());
        assert_eq!(session.id, 30);
        assert!(Arc::ptr_eq(&session.bytes, &before));
        assert_eq!(rotations(&session.info.pages), [0]);
    }
}
