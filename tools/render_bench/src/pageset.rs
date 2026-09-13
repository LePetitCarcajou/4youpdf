//! The reference page set, `tools/render_bench/pages.toml`: which pages of
//! which files the bench renders, at what width. It is versioned, so that two
//! runs weeks apart measure the same pages, and it records the SHA-256 of
//! every file, so that a corpus fetched again with other bytes is noticed
//! instead of measured.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

use crate::system;

/// Format of the page set file this bench reads and writes.
pub const FORMAT: u32 = 1;

/// Widths the application's page service accepts (`app/src/render.rs`).
pub const WIDTHS: std::ops::RangeInclusive<u32> = 16..=4096;

/// A page set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageSet {
    /// [`FORMAT`].
    pub format: u32,
    /// Width of every image, in pixels.
    pub width: u32,
    /// The pages, in the order of the reports.
    #[serde(rename = "page", default)]
    pub pages: Vec<PageEntry>,
}

/// A page of the set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageEntry {
    /// The PDF file, relative to the root of the repository, with forward
    /// slashes.
    pub file: String,
    /// SHA-256 of the file when the page was chosen.
    pub sha256: String,
    /// Page number, 0-based.
    pub index: usize,
    /// Password that opens the file, when it needs one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub password: String,
    /// Why the page is in the set: what it holds that a renderer may get
    /// wrong (`select`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub why: Vec<String>,
}

/// The pages of one file, opened with one password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentPages {
    /// [`PageEntry::file`].
    pub file: String,
    /// [`PageEntry::password`].
    pub password: String,
    /// Positions of its pages in [`PageSet::pages`], in order.
    pub positions: Vec<usize>,
}

impl PageSet {
    /// Read a page set from TOML and check it.
    pub fn parse(text: &str) -> Result<PageSet, String> {
        let set: PageSet =
            toml::from_str(text).map_err(|e| format!("jeu de pages illisible : {e}"))?;
        set.check()?;
        Ok(set)
    }

    /// Read and check the page set in `path`.
    pub fn load(path: &Path) -> Result<PageSet, String> {
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("{} : {e}", path.display()))?;
        PageSet::parse(&text).map_err(|e| format!("{} : {e}", path.display()))
    }

    /// Refuse what the bench could not measure the same way twice.
    fn check(&self) -> Result<(), String> {
        if self.format != FORMAT {
            return Err(format!(
                "format {} ; ce banc lit le format {FORMAT}",
                self.format
            ));
        }
        if !WIDTHS.contains(&self.width) {
            return Err(format!(
                "largeur {} hors de {}..={}, les largeurs que sert l'application",
                self.width,
                WIDTHS.start(),
                WIDTHS.end()
            ));
        }
        let mut pages = BTreeSet::new();
        let mut digests = BTreeMap::new();
        for (position, page) in self.pages.iter().enumerate() {
            let n = position + 1;
            if !is_repository_path(&page.file) {
                return Err(format!(
                    "page {n} : {:?} n'est pas un chemin relatif à la racine du dépôt, avec des barres obliques et sans « .. »",
                    page.file
                ));
            }
            let hex = page
                .sha256
                .bytes()
                .all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'));
            if page.sha256.len() != 64 || !hex {
                return Err(format!(
                    "page {n} : empreinte SHA-256 invalide pour {}",
                    page.file
                ));
            }
            if *digests
                .entry(page.file.as_str())
                .or_insert(page.sha256.as_str())
                != page.sha256
            {
                return Err(format!(
                    "page {n} : {} porte deux empreintes différentes",
                    page.file
                ));
            }
            if !pages.insert((page.file.as_str(), page.index)) {
                return Err(format!(
                    "page {n} : la page {} de {} est déjà dans le jeu",
                    page.index, page.file
                ));
            }
        }
        Ok(())
    }

    /// The set as TOML, after `comment` turned into comment lines.
    pub fn to_toml(&self, comment: &str) -> Result<String, String> {
        let body = toml::to_string(self).map_err(|e| format!("jeu de pages : {e}"))?;
        let mut text = String::new();
        for line in comment.lines() {
            text.push('#');
            if !line.is_empty() {
                text.push(' ');
                text.push_str(line);
            }
            text.push('\n');
        }
        if !comment.is_empty() {
            text.push('\n');
        }
        text.push_str(&body);
        Ok(text)
    }

    /// The documents of the set, by file and password, in the order of their
    /// first page.
    pub fn documents(&self) -> Vec<DocumentPages> {
        let mut documents: Vec<DocumentPages> = Vec::new();
        for (position, page) in self.pages.iter().enumerate() {
            match documents
                .iter_mut()
                .find(|d| d.file == page.file && d.password == page.password)
            {
                Some(document) => document.positions.push(position),
                None => documents.push(DocumentPages {
                    file: page.file.clone(),
                    password: page.password.clone(),
                    positions: vec![position],
                }),
            }
        }
        documents
    }
}

/// A relative path of plain names, with forward slashes: no empty part, no
/// `.` or `..`, no drive.
fn is_repository_path(file: &str) -> bool {
    !file.contains('\\')
        && file
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && Path::new(file)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

/// What became of a file of the set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum FileState {
    /// Present, with the bytes the set recorded.
    Unchanged,
    /// Not found, or unreadable.
    Missing {
        /// Why.
        error: String,
    },
    /// Present with other bytes: not the corpus the set was chosen from.
    Changed {
        /// SHA-256 of the bytes found.
        sha256: String,
    },
}

/// The state of every file of `set` under `root`, each read once.
pub fn check_files(set: &PageSet, root: &Path) -> BTreeMap<String, FileState> {
    let mut states = BTreeMap::new();
    for page in &set.pages {
        if states.contains_key(&page.file) {
            continue;
        }
        let state = match std::fs::read(root.join(&page.file)) {
            Err(e) => FileState::Missing {
                error: e.to_string(),
            },
            Ok(bytes) => {
                let sha256 = system::sha256_hex(&bytes);
                if sha256 == page.sha256 {
                    FileState::Unchanged
                } else {
                    FileState::Changed { sha256 }
                }
            }
        };
        states.insert(page.file.clone(), state);
    }
    states
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const SHA_A: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const SHA_B: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    fn entry(file: &str, sha256: &str, index: usize) -> PageEntry {
        PageEntry {
            file: file.into(),
            sha256: sha256.into(),
            index,
            password: String::new(),
            why: vec!["font:Type3".into()],
        }
    }

    fn set(pages: Vec<PageEntry>) -> PageSet {
        PageSet {
            format: FORMAT,
            width: 1400,
            pages,
        }
    }

    #[test]
    fn a_set_reads_back_what_it_writes() {
        let mut locked = entry("tests/corpus/qpdf/enc,U=view.pdf", SHA_B, 0);
        locked.password = "view".into();
        let original = set(vec![entry("tests/fixtures/a b.pdf", SHA_A, 2), locked]);
        let text = original.to_toml("Jeu de pages.\n\nDeux lignes.").unwrap();
        assert!(
            text.starts_with("# Jeu de pages.\n#\n# Deux lignes.\n\n"),
            "{text}"
        );
        assert_eq!(PageSet::parse(&text).unwrap(), original);
    }

    #[test]
    fn what_could_not_be_measured_twice_is_refused() {
        let refused = |set: PageSet, words: &str| {
            let text = toml::to_string(&set).unwrap();
            let error = PageSet::parse(&text).unwrap_err();
            assert!(error.contains(words), "{error}");
        };
        let mut other_format = set(vec![]);
        other_format.format = 2;
        refused(other_format, "format 2");
        let mut narrow = set(vec![]);
        narrow.width = 8;
        refused(narrow, "largeur 8");
        for path in [
            "../x.pdf",
            "tests\\x.pdf",
            "/tests/x.pdf",
            "tests/./x.pdf",
            "",
        ] {
            refused(set(vec![entry(path, SHA_A, 0)]), "chemin relatif");
        }
        refused(set(vec![entry("a.pdf", "ABC", 0)]), "empreinte");
        refused(
            set(vec![entry("a.pdf", SHA_A, 0), entry("a.pdf", SHA_A, 0)]),
            "déjà dans le jeu",
        );
        refused(
            set(vec![entry("a.pdf", SHA_A, 0), entry("a.pdf", SHA_B, 1)]),
            "deux empreintes",
        );
        assert!(PageSet::parse("format = 1\nwidth = 800\nextra = 1\n").is_err());
    }

    #[test]
    fn documents_group_pages_in_order() {
        let mut locked = entry("b.pdf", SHA_B, 0);
        locked.password = "pw".into();
        let pages = set(vec![
            entry("a.pdf", SHA_A, 3),
            entry("b.pdf", SHA_B, 0),
            entry("a.pdf", SHA_A, 1),
            locked,
        ]);
        let documents = pages.documents();
        assert_eq!(documents.len(), 3);
        assert_eq!(
            (documents[0].file.as_str(), &documents[0].positions),
            ("a.pdf", &vec![0, 2])
        );
        assert_eq!(documents[1].positions, vec![1]);
        assert_eq!(
            (documents[2].password.as_str(), &documents[2].positions),
            ("pw", &vec![3])
        );
    }

    #[test]
    fn files_are_checked_against_their_digest() {
        let root =
            std::env::temp_dir().join(format!("fyp-render-bench-set-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("abc.pdf"), b"abc").unwrap();
        std::fs::write(root.join("other.pdf"), b"abd").unwrap();
        let pages = set(vec![
            entry("abc.pdf", SHA_A, 0),
            entry("other.pdf", SHA_A, 0),
            entry("absent.pdf", SHA_A, 0),
        ]);
        let states = check_files(&pages, &root);
        assert_eq!(states["abc.pdf"], FileState::Unchanged);
        assert!(matches!(&states["other.pdf"], FileState::Changed { sha256 } if sha256 != SHA_A));
        assert!(matches!(states["absent.pdf"], FileState::Missing { .. }));
        let _ = std::fs::remove_dir_all(&root);
    }
}
