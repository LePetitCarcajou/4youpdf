//! The open document: its bytes, what `fyp-core` says about it, and the
//! operations the window needs, all through `fyp_core::ops`: listing
//! pages, turning some of them, appending the pages chosen of other files,
//! saving a new page order, cutting it into several files.

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

/// What cutting the document produced (see [`Session::split`]).
#[derive(Debug, Clone, Serialize)]
pub struct SplitReport {
    /// Folder written into, as chosen.
    pub dir: String,
    /// One entry per file written, in the order they were written.
    pub files: Vec<SplitFile>,
}

/// One file written by a cut.
#[derive(Debug, Clone, Serialize)]
pub struct SplitFile {
    /// File name alone: the folder is the same for all of them.
    pub name: String,
    /// Pages in it.
    pub pages: usize,
    /// Size of the file written.
    pub size: u64,
}

/// What became of one file asked to be merged.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceOutcome {
    /// Its pages follow those of the document: all of them, or those
    /// chosen.
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

/// A file chosen to be merged, looked at as soon as it is chosen (see
/// [`candidates`]): how many pages it holds, for the banner that asks which
/// of them to take, or why the merge will skip it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Candidate {
    /// Path as chosen.
    pub path: String,
    /// File name alone, for the banner.
    pub name: String,
    pub status: CandidateStatus,
}

/// What a file chosen to be merged holds, as far as merging is concerned.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CandidateStatus {
    /// It opens without a password: its pages may be chosen.
    Ready {
        /// How many pages it holds.
        pages: usize,
    },
    /// Protected by a password, which merging does not ask for: it will be
    /// skipped.
    Protected,
    /// Not read, or refused by the core even after repair: it will be
    /// skipped.
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
    /// The file as opened, which an extraction never replaces
    /// ([`Session::extract`]).
    pub path: PathBuf,
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
            path: path.to_path_buf(),
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

    /// Write the pages at `order` to `path` like [`Session::save`], for an
    /// extraction: a copy of some pages, which leaves the document as it
    /// is and so never replaces the file it was opened from. Windows asks
    /// before a file is replaced, but not in words that say it is the
    /// document open, and the window would go on taking the document for
    /// intact while its file held only the pages extracted. Refused before
    /// anything is built or written; any other file may be replaced, as
    /// the user confirmed to Windows.
    pub fn extract(&self, order: &[usize], path: &Path) -> Result<SaveReport, AppError> {
        if same_file(path, &self.path) {
            return Err(AppError::other(format!(
                "« {} » est le fichier du document ouvert, qu'une extraction ne remplace jamais ; choisissez un autre nom. Rien n'a été écrit.",
                self.info.name
            )));
        }
        self.save(order, path)
    }

    /// Write each part of `parts` (0-based indices into this document, in
    /// the wanted order) to its own file of `dir`, named after the
    /// document ([`part_name`]), through [`ops::extract_pages`] like
    /// [`Session::save`]. The document itself is left as it is.
    ///
    /// Nothing existing is ever replaced, and a cut that cannot be written
    /// whole is not written at all: the names are checked against the
    /// folder first, then every part is built and read back, and only then
    /// are the files created, each with `create_new`, which refuses a file
    /// that appeared meanwhile.
    pub fn split(&self, parts: &[Vec<usize>], dir: &Path) -> Result<SplitReport, AppError> {
        if parts.is_empty() || parts.iter().any(Vec::is_empty) {
            return Err(AppError::other(
                "découpage vide : chaque partie doit recevoir au moins une page",
            ));
        }
        // The parts share out the pages on screen, which are pages of this
        // document, each taken once: there can never be more of them than
        // the document has pages.
        if parts.len() > self.info.pages.len() {
            return Err(AppError::other(format!(
                "découpage en {} fichiers pour {} pages ; rien n'a été écrit",
                parts.len(),
                self.info.pages.len()
            )));
        }
        if !dir.is_dir() {
            return Err(AppError::other(format!(
                "{} n'est pas un dossier ; rien n'a été écrit",
                dir.display()
            )));
        }
        let stem = part_stem(&self.info.name);
        let names: Vec<String> = (0..parts.len())
            .map(|i| part_name(&stem, i, parts.len()))
            .collect();
        let taken: Vec<&str> = names
            .iter()
            .map(String::as_str)
            .filter(|name| dir.join(name).exists())
            .collect();
        if !taken.is_empty() {
            return Err(conflict(&taken, dir));
        }
        // Every part is built and read back before the first is written: a
        // part the core refuses leaves the folder as it was.
        let doc = self.document()?;
        let mut built = Vec::with_capacity(parts.len());
        for (part, name) in parts.iter().zip(&names) {
            let out = ops::extract_pages(&doc, part)?;
            let check = Document::open(&out)?;
            if let Some(reason) = check.reconstructed() {
                return Err(AppError::other(format!(
                    "« {name} » a dû être réparé à la relecture ({reason}) ; rien n'a été écrit"
                )));
            }
            built.push(out);
        }
        let mut files = Vec::with_capacity(parts.len());
        for ((part, name), bytes) in parts.iter().zip(&names).zip(&built) {
            write_new(&dir.join(name), bytes)?;
            files.push(SplitFile {
                name: name.clone(),
                pages: part.len(),
                size: bytes.len() as u64,
            });
        }
        Ok(SplitReport {
            dir: dir.display().to_string(),
            files,
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

/// The name the parts of a cut are built on: the file name of the
/// document without its `.pdf` extension, whatever its case. A name that
/// would leave nothing, or nothing but spaces, gives `document`, so that a
/// part is never called `_partie-01.pdf`.
fn part_stem(name: &str) -> String {
    let stem = match name.rfind('.') {
        Some(dot) if name[dot..].eq_ignore_ascii_case(".pdf") => &name[..dot],
        _ => name,
    };
    let stem = stem.trim();
    if stem.is_empty() {
        "document".to_owned()
    } else {
        stem.to_owned()
    }
}

/// The name of part `index` of `count`: `<stem>_partie-01.pdf`, numbered
/// from 1, with as many digits as `count` needs and never fewer than two,
/// so that the files of one cut are listed in their order by a file
/// manager, which sorts names as text.
fn part_name(stem: &str, index: usize, count: usize) -> String {
    let width = count.to_string().len().max(2);
    let number = index + 1;
    format!("{stem}_partie-{number:0width$}.pdf")
}

/// Why a cut wrote nothing: these names are taken in `dir`. Three names
/// at most, the rest counted, so that the banner names the conflict
/// without becoming a list.
fn conflict(names: &[&str], dir: &Path) -> AppError {
    let shown: Vec<String> = names.iter().take(3).map(|n| format!("« {n} »")).collect();
    let mut what = shown.join(", ");
    let rest = names.len() - shown.len();
    if rest > 0 {
        what.push_str(&format!(" et {rest} autre{}", plural(rest)));
    }
    let exist = if names.len() > 1 {
        "existent"
    } else {
        "existe"
    };
    AppError::other(format!(
        "{what} {exist} déjà dans {} ; aucun fichier n'a été écrit",
        dir.display()
    ))
}

fn plural(count: usize) -> &'static str {
    if count > 1 {
        "s"
    } else {
        ""
    }
}

/// Write `bytes` to `path`, which must not exist. `create_new` asks the
/// system to create the file only if it is not there, in one step: a file
/// that appeared since the names were checked is not replaced either.
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    use std::io::Write as _;

    let fail = |e: std::io::Error| AppError::other(format!("{} : {e}", path.display()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(fail)?;
    file.write_all(bytes).map_err(fail)
}

/// Whether writing to `target` would replace the file at `open`. Only when
/// `target` exists: a file that is not there replaces nothing, and is not
/// compared. It is then the same file when both paths are the same once
/// made canonical by the system, `..` and links resolved and, under
/// Windows, the names spelt as the disk has them, whatever their case.
/// Two hard links to one file stay two paths (`docs/backlog-technique.md`).
fn same_file(target: &Path, open: &Path) -> bool {
    let Ok(target) = std::fs::canonicalize(target) else {
        return false;
    };
    std::fs::canonicalize(open).is_ok_and(|open| open == target)
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

/// Why a file asked to be merged is skipped.
enum Skipped {
    /// It needs a password, which merging does not ask for.
    Protected,
    /// It could not be read, or the core refuses it even after repair.
    Refused(String),
}

/// A file to merge, from `content`, its bytes or why they could not be
/// read: opened without a password, with its page count, or why it is
/// skipped. What the banner is told when the file is chosen
/// ([`candidates`]), and what merging checks again ([`merge`]), the file
/// having maybe changed meanwhile.
fn open_source(content: &Result<Vec<u8>, String>) -> Result<(Document<'_>, usize), Skipped> {
    let content = content.as_ref().map_err(|e| Skipped::Refused(e.clone()))?;
    let doc = Document::open(content).map_err(|e| match e {
        fyp_core::Error::WrongPassword => Skipped::Protected,
        other => Skipped::Refused(other.to_string()),
    })?;
    let count = ops::pages(&doc)
        .map_err(|e| Skipped::Refused(e.to_string()))?
        .len();
    Ok((doc, count))
}

/// Each file of `files`, as the banner of a merge first shows it: read and
/// opened as [`merge`] will, then let go.
pub fn candidates(files: &[PathBuf]) -> Vec<Candidate> {
    files
        .iter()
        .map(|path| {
            let content = std::fs::read(path).map_err(|e| e.to_string());
            let status = match open_source(&content) {
                Ok((_, pages)) => CandidateStatus::Ready { pages },
                Err(Skipped::Protected) => CandidateStatus::Protected,
                Err(Skipped::Refused(message)) => CandidateStatus::Refused { message },
            };
            Candidate {
                path: path.display().to_string(),
                name: file_name(path),
                status,
            }
        })
        .collect()
}

/// The pages to take of the file `name`, which holds `count`: every one
/// when no list is given (the field of the banner left empty), otherwise
/// those of `wanted`, 0-based, in that order, each once and each there.
/// The banner refuses the same lists before asking, but the file may have
/// changed since it was chosen: a refusal names it, and the page as the
/// banner numbers it, from 1.
fn selection(
    name: &str,
    wanted: Option<&[usize]>,
    count: usize,
) -> Result<ops::Selection, AppError> {
    let Some(wanted) = wanted else {
        return Ok(ops::Selection::All);
    };
    if wanted.is_empty() {
        return Err(AppError::other(format!(
            "« {name} » : aucune page indiquée ; rien n'a été fusionné"
        )));
    }
    let mut seen = std::collections::HashSet::new();
    for &page in wanted {
        let number = page.saturating_add(1);
        if page >= count {
            return Err(AppError::other(format!(
                "« {name} » : page {number} hors limites, le fichier a {count} page{} ; rien n'a été fusionné",
                plural(count)
            )));
        }
        // `ops::merge_selected` would take it twice; the window does not
        // offer that (`docs/backlog-ui.md`).
        if !seen.insert(page) {
            return Err(AppError::other(format!(
                "« {name} » : la page {number} est demandée deux fois ; rien n'a été fusionné"
            )));
        }
    }
    Ok(ops::Selection::Pages(wanted.to_vec()))
}

/// The document in `bytes`, opened with `password`, followed by the pages
/// chosen of each file of `files` that opens, in that order, through
/// [`ops::merge_selected`]: `pages[i]` names those of `files[i]`, 0-based
/// and in the order wanted, or all of them when `None`; the document itself
/// keeps every page. The files are opened without a password: one that
/// needs a password, one that cannot be read, and one the core refuses even
/// after repair are each skipped and said so in the report, and the others
/// are merged without them; a file is checked to have pages before the
/// merge, so that the merge itself fails only for the lot. A list of pages
/// that does not fit its file ([`selection`]) refuses the whole merge. When
/// no file opens, nothing is rewritten. Like a rotation, the rewrite must
/// read back without repair before it is returned.
pub fn merge(
    bytes: &[u8],
    password: &str,
    files: &[PathBuf],
    pages: &[Option<Vec<usize>>],
) -> Result<Merged, AppError> {
    if pages.len() != files.len() {
        return Err(AppError::other(format!(
            "{} fichier{} à fusionner mais {} liste{} de pages ; rien n'a été fusionné",
            files.len(),
            plural(files.len()),
            pages.len(),
            plural(pages.len())
        )));
    }
    let doc = Document::open_with_password(bytes, password.as_bytes())?;
    let read: Vec<Result<Vec<u8>, String>> = files
        .iter()
        .map(|path| std::fs::read(path).map_err(|e| e.to_string()))
        .collect();
    let mut docs: Vec<Document<'_>> = vec![doc];
    let mut selections = vec![ops::Selection::All];
    let mut sources = Vec::with_capacity(files.len());
    let mut added = 0usize;
    for ((path, content), wanted) in files.iter().zip(&read).zip(pages) {
        let name = file_name(path);
        let outcome = match open_source(content) {
            Err(Skipped::Protected) => SourceOutcome::Protected,
            Err(Skipped::Refused(message)) => SourceOutcome::Refused { message },
            Ok((other, count)) => {
                let chosen = selection(&name, wanted.as_deref(), count)?;
                let taken = match &chosen {
                    ops::Selection::All => count,
                    ops::Selection::Pages(pages) => pages.len(),
                };
                added = added.saturating_add(taken);
                let outcome = SourceOutcome::Merged {
                    pages: taken,
                    reconstructed: other.reconstructed().map(ToString::to_string),
                    encryption: other.encryption().map(|e| describe_encryption(&e)),
                };
                docs.push(other);
                selections.push(chosen);
                outcome
            }
        };
        sources.push(SourceReport {
            path: path.display().to_string(),
            name,
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
    let out = ops::merge_selected(&docs, &selections)?;
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

    /// A temporary folder of this process, named after `what`, empty.
    fn temp_dir(what: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fyp-app-{what}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The `/Rotate` of each page of the file at `path`, in reading order.
    fn rotations_of(path: &Path) -> Vec<i32> {
        let bytes = std::fs::read(path).unwrap();
        let doc = Document::open(&bytes).unwrap();
        assert_eq!(doc.reconstructed(), None, "{}", path.display());
        page_infos(&doc).unwrap().iter().map(|p| p.rotate).collect()
    }

    /// Extracting a selection writes the pages as the grid shows them —
    /// the order asked for, the rotations applied so far — and leaves the
    /// document open as it was: it is the interface that does not record
    /// the file written, so nothing here may change either.
    #[test]
    fn extracting_a_selection_writes_the_pages_as_shown() {
        let dir = temp_dir("extract");
        let path = dir.join("three.pdf");
        std::fs::write(&path, three_pages()).unwrap();
        let mut session = Session::open(70, &path, "").expect("open");
        assert_eq!(turn(&mut session, &[0], 90), [180, 180, 270]);
        let (id, bytes) = (session.id, Arc::clone(&session.bytes));

        // Pages 3 and 1 of the file, in that order: a selection of the
        // grid after a move, a deletion and a rotation.
        let out = dir.join("selection.pdf");
        let report = session.save(&[2, 0], &out).expect("extract");
        assert_eq!(report.pages, 2);
        assert_eq!(rotations_of(&out), [270, 180]);
        assert_eq!(report.size, std::fs::read(&out).unwrap().len() as u64);
        // The document on screen is untouched: same bytes, same id, so the
        // renderer keeps its images.
        assert_eq!(session.id, id);
        assert!(Arc::ptr_eq(&session.bytes, &bytes));
        assert_eq!(rotations(&session.info.pages), [180, 180, 270]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An extraction never replaces the file the document was opened from,
    /// however its path is written: the same path, the same file under
    /// other capitals (Windows, whose names ignore case), or through a
    /// folder and back. Nothing is written then, and the file keeps its
    /// bytes. Any other file that exists is replaced, as the user confirmed
    /// to Windows; saving, unlike extracting, may replace the file open.
    #[test]
    fn extracting_never_replaces_the_file_open() {
        let dir = temp_dir("extract-over");
        let path = dir.join("three.pdf");
        std::fs::write(&path, three_pages()).unwrap();
        std::fs::create_dir(dir.join("sub")).unwrap();
        let session = Session::open(110, &path, "").expect("open");
        let before = std::fs::read(&path).unwrap();

        let mut same = vec![path.clone(), dir.join("sub").join("..").join("three.pdf")];
        if cfg!(windows) {
            same.push(dir.join("THREE.PDF"));
        }
        for target in &same {
            let Err(AppError::Other { message }) = session.extract(&[0], target) else {
                panic!("{}: the file open was replaced", target.display());
            };
            assert!(
                message.starts_with("« three.pdf » est le fichier du document ouvert"),
                "{message}"
            );
            assert!(message.ends_with("Rien n'a été écrit."), "{message}");
            assert_eq!(
                std::fs::read(&path).unwrap(),
                before,
                "{}",
                target.display()
            );
        }

        // Another file that exists is replaced by the pages extracted, and
        // a file that does not exist yet is written.
        let other = dir.join("other.pdf");
        std::fs::write(&other, b"not a pdf").unwrap();
        assert_eq!(session.extract(&[2, 0], &other).expect("extract").pages, 2);
        assert_eq!(rotations_of(&other), [270, 90]);
        let new = dir.join("new.pdf");
        assert_eq!(session.extract(&[1], &new).expect("extract").pages, 1);
        assert_eq!(rotations_of(&new), [180]);

        // Saving writes the whole document where the user chose, the file
        // open included.
        assert_eq!(session.save(&[0, 1, 2], &path).expect("save").pages, 3);
        assert_eq!(rotations_of(&path), [90, 180, 270]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A cut writes one file per part, from the pages on screen: the parts
    /// are slices of the order the grid holds, not ranges of the file, so
    /// a page moved, dropped or turned goes where the grid shows it.
    #[test]
    fn splitting_writes_one_file_per_part_of_the_order_on_screen() {
        let dir = temp_dir("split");
        let path = dir.join("three.pdf");
        std::fs::write(&path, three_pages()).unwrap();
        let mut session = Session::open(80, &path, "").expect("open");
        assert_eq!(turn(&mut session, &[2], 90), [90, 180, 0]);

        // The grid shows page 3 first, then page 1: page 2 was deleted.
        let report = session.split(&[vec![2], vec![0]], &dir).expect("split");
        assert_eq!(report.dir, dir.display().to_string());
        let names: Vec<&str> = report.files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["three_partie-01.pdf", "three_partie-02.pdf"]);
        assert_eq!(
            report.files.iter().map(|f| f.pages).collect::<Vec<_>>(),
            [1, 1]
        );
        assert_eq!(rotations_of(&dir.join("three_partie-01.pdf")), [0]);
        assert_eq!(rotations_of(&dir.join("three_partie-02.pdf")), [90]);
        for file in &report.files {
            assert_eq!(
                file.size,
                std::fs::read(dir.join(&file.name)).unwrap().len() as u64
            );
        }
        // The document on screen is untouched, as after an extraction.
        assert_eq!(session.id, 81);
        assert_eq!(rotations(&session.info.pages), [90, 180, 0]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A cut never replaces a file: one name taken is enough for the whole
    /// cut to be refused, before anything is written, and the message
    /// names the files in the way.
    #[test]
    fn splitting_replaces_no_file_and_writes_nothing_then() {
        let dir = temp_dir("split-again");
        let path = dir.join("three.pdf");
        std::fs::write(&path, three_pages()).unwrap();
        let session = Session::open(90, &path, "").expect("open");
        let parts = [vec![0], vec![1], vec![2]];
        assert_eq!(session.split(&parts, &dir).expect("split").files.len(), 3);
        let before: Vec<u8> = std::fs::read(dir.join("three_partie-02.pdf")).unwrap();

        // The same cut again: every name is taken, nothing is written.
        let Err(AppError::Other { message }) = session.split(&parts, &dir) else {
            panic!("the second cut must be refused");
        };
        for name in ["01", "02", "03"] {
            assert!(
                message.contains(&format!("« three_partie-{name}.pdf »")),
                "{message}"
            );
        }
        assert!(message.contains("aucun fichier n'a été écrit"), "{message}");
        assert_eq!(
            std::fs::read(dir.join("three_partie-02.pdf")).unwrap(),
            before
        );

        // One name taken is enough, and the files that were free stay free.
        std::fs::remove_file(dir.join("three_partie-01.pdf")).unwrap();
        std::fs::remove_file(dir.join("three_partie-03.pdf")).unwrap();
        assert!(session.split(&parts, &dir).is_err());
        assert!(!dir.join("three_partie-01.pdf").exists());
        assert!(!dir.join("three_partie-03.pdf").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What a cut refuses before touching the folder: no part, an empty
    /// part, more parts than pages, a page the document has not, a page
    /// twice in one part (`ops::extract_pages`), a folder that is not one.
    #[test]
    fn a_refused_split_writes_nothing() {
        let dir = temp_dir("split-refused");
        let path = dir.join("three.pdf");
        std::fs::write(&path, three_pages()).unwrap();
        let session = Session::open(100, &path, "").expect("open");
        for parts in [
            vec![],
            vec![vec![0], vec![]],
            vec![vec![0], vec![1], vec![2], vec![0]],
            vec![vec![3]],
            vec![vec![0, 0]],
        ] {
            assert!(
                matches!(session.split(&parts, &dir), Err(AppError::Other { .. })),
                "{parts:?}"
            );
        }
        assert!(session.split(&[vec![0]], &path).is_err(), "not a folder");
        assert!(session.split(&[vec![0]], &dir.join("absent")).is_err());
        // Only three.pdf, still: not one part was written.
        let left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, ["three.pdf"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The parts are numbered from 1, with as many digits as their number
    /// needs and never fewer than two, under the name of the document
    /// without its extension.
    #[test]
    fn the_parts_are_named_after_the_document_and_numbered() {
        assert_eq!(part_name("rapport", 0, 3), "rapport_partie-01.pdf");
        assert_eq!(part_name("rapport", 2, 3), "rapport_partie-03.pdf");
        assert_eq!(part_name("rapport", 9, 100), "rapport_partie-010.pdf");
        assert_eq!(part_name("rapport", 99, 100), "rapport_partie-100.pdf");
        assert_eq!(part_name("rapport", 0, 1), "rapport_partie-01.pdf");
        for (name, stem) in [
            ("rapport.pdf", "rapport"),
            ("RAPPORT.PDF", "RAPPORT"),
            ("rapport.final.pdf", "rapport.final"),
            ("sans-extension", "sans-extension"),
            ("rapport.txt", "rapport.txt"),
            ("été 2026.pdf", "été 2026"),
            (".pdf", "document"),
            ("", "document"),
            ("   ", "document"),
        ] {
            assert_eq!(part_stem(name), stem, "{name}");
        }
    }

    /// A refused cut names the files in the way, three at most, and counts
    /// the rest: a banner must say which name is taken without becoming a
    /// list of a hundred.
    #[test]
    fn a_name_taken_is_named_and_the_rest_counted() {
        let dir = Path::new("C:\\docs");
        let message = |names: &[&str]| match conflict(names, dir) {
            AppError::Other { message } => message,
            AppError::WrongPassword => unreachable!(),
        };
        assert_eq!(
            message(&["a.pdf"]),
            "« a.pdf » existe déjà dans C:\\docs ; aucun fichier n'a été écrit"
        );
        assert!(message(&["a.pdf", "b.pdf"]).starts_with("« a.pdf », « b.pdf » existent déjà"));
        assert!(message(&["a", "b", "c", "d"]).contains("« c » et 1 autre existent"));
        assert!(message(&["a", "b", "c", "d", "e"]).contains("et 2 autres existent"));
    }

    /// Merge every page of each of `files` into `session`, as the
    /// application does when no page is chosen.
    fn append(session: &mut Session, files: &[PathBuf]) -> Merged {
        append_pages(session, files, &vec![None; files.len()])
    }

    /// Merge the pages `pages` of each of `files` into `session`, as the
    /// application does.
    fn append_pages(
        session: &mut Session,
        files: &[PathBuf],
        pages: &[Option<Vec<usize>>],
    ) -> Merged {
        let mut merged = merge(&session.bytes, &session.password, files, pages).expect("merge");
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
        let other = merge(&session.bytes, "", &[fixture("minimal.pdf")], &[None]).expect("merge");
        let rewrite = other.rewrite.expect("rewritten");
        assert!(session.extend(52, rewrite, 5).is_err());
        assert_eq!(session.id, 51);
        assert_eq!(session.info.pages.len(), 2);
    }

    /// `count` A4 pages, each showing `<prefix><n>`, `n` from 1: a page can
    /// be told by its text wherever it lands.
    fn labelled(prefix: &str, count: usize) -> Vec<u8> {
        let kids: Vec<String> = (0..count).map(|i| format!("{} 0 R", 3 + 2 * i)).collect();
        let mut objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            format!(
                "<< /Type /Pages /Kids [{}] /Count {count} /MediaBox [0 0 595 842] \
                 /Resources << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >>",
                kids.join(" ")
            ),
        ];
        for i in 0..count {
            let text = format!("BT /F1 24 Tf 72 720 Td ({prefix}{}) Tj ET", i + 1);
            objects.push(format!(
                "<< /Type /Page /Parent 2 0 R /Contents {} 0 R >>",
                4 + 2 * i
            ));
            objects.push(format!(
                "<< /Length {} >>\nstream\n{text}\nendstream",
                text.len()
            ));
        }
        let objects: Vec<&str> = objects.iter().map(String::as_str).collect();
        hand_built(&objects)
    }

    /// What each page of the PDF in `bytes` shows ([`labelled`]), in
    /// reading order. The file must read back without repair.
    fn labels(bytes: &[u8]) -> Vec<String> {
        let doc = Document::open(bytes).unwrap();
        assert_eq!(doc.reconstructed(), None);
        ops::pages(&doc)
            .unwrap()
            .iter()
            .map(|page| {
                let contents = doc
                    .resolve(page.dict.get(&Name::new("Contents")).unwrap())
                    .unwrap();
                let data = doc.decoded(&contents).unwrap();
                let text = String::from_utf8_lossy(&data);
                text.split('(')
                    .nth(1)
                    .and_then(|rest| rest.split(')').next())
                    .unwrap()
                    .to_owned()
            })
            .collect()
    }

    /// Three documents, pages chosen in each. The document open keeps every
    /// page, as the grid shows it; of each file merged, the pages named, in
    /// the order named, reversed for one. The rewrite reads back without
    /// repair, and the file saved afterwards holds exactly the pages the
    /// grid shows, moved, dropped and turned, in its order.
    #[test]
    fn merging_takes_the_pages_chosen_of_each_file_in_their_order() {
        let dir = temp_dir("merge-chosen");
        let write = |name: &str, bytes: Vec<u8>| {
            let path = dir.join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        };
        let a = write("a.pdf", labelled("A", 3));
        let b = write("b.pdf", labelled("B", 5));
        let c = write("c.pdf", labelled("C", 4));
        let mut session = Session::open(120, &a, "").expect("open");
        assert_eq!(turn(&mut session, &[1], 90), [0, 90, 0]);

        // « 5-3 » typed for b.pdf, « 1,4 » for c.pdf.
        let merged = append_pages(
            &mut session,
            &[b, c],
            &[Some(vec![4, 3, 2]), Some(vec![0, 3])],
        );
        assert_eq!(merged.added, 5);
        assert!(
            matches!(
                outcomes(&merged)[..],
                [
                    SourceOutcome::Merged { pages: 3, .. },
                    SourceOutcome::Merged { pages: 2, .. }
                ]
            ),
            "{:?}",
            outcomes(&merged)
        );
        assert_eq!(
            labels(&session.bytes),
            ["A1", "A2", "A3", "B5", "B4", "B3", "C1", "C4"]
        );
        assert_eq!(rotations(&session.info.pages), [0, 90, 0, 0, 0, 0, 0, 0]);

        // The grid once pages were moved and dropped: C1, A2, B5, A1.
        let out = dir.join("out.pdf");
        assert_eq!(session.save(&[6, 1, 3, 0], &out).expect("save").pages, 4);
        assert_eq!(
            labels(&std::fs::read(&out).unwrap()),
            ["C1", "A2", "B5", "A1"]
        );
        assert_eq!(rotations_of(&out), [0, 90, 0, 0]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A list of pages that does not fit its file refuses the whole merge,
    /// before anything is rewritten, and says which file and which page: a
    /// page asked twice (which `ops::merge_selected` would take), a page the
    /// file has not, an empty list, or not one list per file. A file that
    /// is skipped keeps its list to itself: the others merge.
    #[test]
    fn a_list_of_pages_that_does_not_fit_refuses_the_whole_merge() {
        let dir = temp_dir("merge-refused");
        let a = dir.join("a.pdf");
        std::fs::write(&a, labelled("A", 1)).unwrap();
        let b = dir.join("b.pdf");
        std::fs::write(&b, labelled("B", 5)).unwrap();
        let mut session = Session::open(130, &a, "").expect("open");
        let bytes = Arc::clone(&session.bytes);
        for (pages, expected) in [
            (
                vec![Some(vec![1, 1])],
                "« b.pdf » : la page 2 est demandée deux fois",
            ),
            (
                vec![Some(vec![0, 5])],
                "« b.pdf » : page 6 hors limites, le fichier a 5 pages",
            ),
            (vec![Some(vec![])], "« b.pdf » : aucune page indiquée"),
            (
                vec![None, None],
                "1 fichier à fusionner mais 2 listes de pages",
            ),
        ] {
            let Err(AppError::Other { message }) =
                merge(&session.bytes, "", std::slice::from_ref(&b), &pages)
            else {
                panic!("{pages:?} was merged");
            };
            assert!(message.starts_with(expected), "{message}");
            assert!(message.ends_with("rien n'a été fusionné"), "{message}");
        }
        assert!(Arc::ptr_eq(&session.bytes, &bytes));
        assert_eq!(labels(&session.bytes), ["A1"]);

        let merged = append_pages(
            &mut session,
            &[fixture("encrypted-user-password.pdf"), b],
            &[Some(vec![7]), Some(vec![4])],
        );
        assert!(
            matches!(
                outcomes(&merged)[..],
                [
                    SourceOutcome::Protected,
                    SourceOutcome::Merged { pages: 1, .. }
                ]
            ),
            "{:?}",
            outcomes(&merged)
        );
        assert_eq!(labels(&session.bytes), ["A1", "B5"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What the banner is told of each file chosen: how many pages it
    /// holds, or why the merge will skip it, as the merge itself finds.
    #[test]
    fn the_files_chosen_are_counted_or_said_to_be_skipped() {
        let not_pdf = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let found = candidates(&[
            fixture("mixed12.pdf"),
            fixture("bad-offsets.pdf"),
            fixture("encrypted-user-password.pdf"),
            not_pdf,
            fixture("absent.pdf"),
        ]);
        assert_eq!(
            found.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            [
                "mixed12.pdf",
                "bad-offsets.pdf",
                "encrypted-user-password.pdf",
                "Cargo.toml",
                "absent.pdf"
            ]
        );
        let statuses: Vec<&CandidateStatus> = found.iter().map(|c| &c.status).collect();
        assert!(
            matches!(
                statuses[..],
                [
                    CandidateStatus::Ready { pages: 12 },
                    CandidateStatus::Ready { pages: 1 },
                    CandidateStatus::Protected,
                    CandidateStatus::Refused { .. },
                    CandidateStatus::Refused { .. }
                ]
            ),
            "{statuses:?}"
        );
        for candidate in &found[3..] {
            if let CandidateStatus::Refused { message } = &candidate.status {
                assert!(!message.is_empty(), "{}", candidate.name);
            }
        }
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
