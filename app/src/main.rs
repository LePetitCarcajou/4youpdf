//! 4YouPDF desktop application (milestone 0.3, first step): one window
//! that opens a PDF, shows its pages as thumbnails, lets the user reorder,
//! turn and delete them, and saves the result through `fyp_core::ops`.
//!
//! The interface (`ui/`, TypeScript) talks to this side through the
//! commands below and nothing else: file dialogs, reading, rendering,
//! turning pages and writing all happen here. The page order and the undo
//! history live in the interface; this side only knows the open document,
//! as turned so far: the interface undoes a rotation by asking for the
//! opposite one.

#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod render;
mod session;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use base64::Engine as _;
use serde::Serialize;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

use render::RenderService;
use session::{DocumentInfo, PageInfo, Rotated, SaveReport, Session};

/// What a command reports when it fails. `WrongPassword` lets the
/// interface ask for one in place; everything else is shown as text.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppError {
    /// The file is encrypted and the password given does not open it.
    WrongPassword,
    /// Anything else, already worded for the user.
    Other {
        /// Human-readable explanation.
        message: String,
    },
}

impl AppError {
    fn other(message: impl Into<String>) -> AppError {
        AppError::Other {
            message: message.into(),
        }
    }
}

impl From<fyp_core::Error> for AppError {
    fn from(e: fyp_core::Error) -> AppError {
        match e {
            fyp_core::Error::WrongPassword => AppError::WrongPassword,
            other => AppError::other(other.to_string()),
        }
    }
}

/// Shared state: the open document and the renderer (shared with the
/// blocking tasks that wait for page images).
struct AppState {
    session: Mutex<Option<Session>>,
    render: Arc<RenderService>,
    next_id: AtomicU64,
}

impl AppState {
    fn session(&self) -> std::sync::MutexGuard<'_, Option<Session>> {
        self.session.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Open `path` and make it the current document. The current document
    /// is replaced only once the new one is open: when opening fails, it
    /// stays current, as the interface keeps showing it.
    fn open(&self, path: &Path, password: &str) -> Result<DocumentInfo, AppError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let session = Session::open(id, path, password)?;
        let info = session.info.clone();
        *self.session() = Some(session);
        Ok(info)
    }

    /// The current document as rendering and rotating take it.
    fn current(&self) -> Result<Current, AppError> {
        let guard = self.session();
        let session = guard
            .as_ref()
            .ok_or_else(|| AppError::other("aucun document ouvert"))?;
        Ok(Current {
            document: session.info.document,
            id: session.id,
            bytes: Arc::clone(&session.bytes),
            password: session.password.clone(),
        })
    }

    /// Turn the pages at `pages` of the opening `document` by `degrees`
    /// clockwise (`session::rotate`) and make the result current; returns
    /// every page as it now stands. The rewrite runs without holding the
    /// document, so that pages keep rendering meanwhile.
    fn rotate(
        &self,
        document: u64,
        pages: &[usize],
        degrees: i32,
    ) -> Result<Vec<PageInfo>, AppError> {
        let current = self.current()?;
        if current.document != document {
            return Err(changed());
        }
        let rotated = session::rotate(&current.bytes, &current.password, pages, degrees)?;
        self.commit(document, current.id, rotated)
    }

    /// Make `rotated`, made from the bytes known as `from` of the opening
    /// `document`, the current document. Refused when they are no longer
    /// current (another file opened, the document closed or turned
    /// meanwhile): a rotation is never applied to what it was not made from.
    fn commit(
        &self,
        document: u64,
        from: u64,
        rotated: Rotated,
    ) -> Result<Vec<PageInfo>, AppError> {
        let mut guard = self.session();
        match guard.as_mut() {
            Some(session) if session.info.document == document && session.id == from => {
                session.replace(self.next_id.fetch_add(1, Ordering::Relaxed), rotated)
            }
            _ => Err(changed()),
        }
    }
}

/// The current document as rendering and rotating take it.
struct Current {
    /// Its opening (`DocumentInfo::document`).
    document: u64,
    /// Its bytes for the renderer's cache (`Session::id`).
    id: u64,
    bytes: Arc<Vec<u8>>,
    password: String,
}

fn changed() -> AppError {
    AppError::other("le document a changé ; la rotation n'a pas été appliquée")
}

/// Open `path` and make it the current document (see `AppState::open`).
#[tauri::command]
fn open_document(
    state: State<'_, AppState>,
    path: String,
    password: Option<String>,
) -> Result<DocumentInfo, AppError> {
    state.open(Path::new(&path), password.as_deref().unwrap_or(""))
}

/// Forget the current document.
#[tauri::command]
fn close_document(state: State<'_, AppState>) {
    *state.session() = None;
}

/// Whether thumbnails can be drawn, and why not otherwise.
#[tauri::command]
fn renderer_status(state: State<'_, AppState>) -> render::Status {
    state.render.status().clone()
}

/// Image of page `page` (0-based) of the current document, `width` pixels
/// wide (a thumbnail, or the page view sized to the window), as a
/// `data:image/png;base64,…` URL.
#[tauri::command]
async fn render_page(
    state: State<'_, AppState>,
    page: usize,
    width: u32,
) -> Result<String, AppError> {
    let Current {
        id,
        bytes,
        password,
        ..
    } = state.current()?;
    // The render service blocks until the worker answers: keep that off
    // the async runtime's threads.
    let service = Arc::clone(&state.render);
    let png = tauri::async_runtime::spawn_blocking(move || {
        service.render(id, bytes, &password, page, width)
    })
    .await
    .map_err(|e| AppError::other(format!("rendu interrompu : {e}")))?
    .map_err(AppError::other)?;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    ))
}

/// Turn the pages at `pages` (0-based indices into the open document) by
/// `degrees` clockwise, relative to their rotation, through `ops::rotate`
/// (see `AppState::rotate`): from then on they render and save turned.
/// `document` is the opening the interface means (`DocumentInfo::document`).
/// Returns every page as it now stands.
#[tauri::command]
async fn rotate_pages(
    app: tauri::AppHandle,
    document: u64,
    pages: Vec<usize>,
    degrees: i32,
) -> Result<Vec<PageInfo>, AppError> {
    // Rewriting a large document takes a while: off the async runtime's
    // threads, like rendering.
    tauri::async_runtime::spawn_blocking(move || {
        app.try_state::<AppState>()
            .ok_or_else(|| AppError::other("état de l'application indisponible"))?
            .rotate(document, &pages, degrees)
    })
    .await
    .map_err(|e| AppError::other(format!("rotation interrompue : {e}")))?
}

/// Write the pages at `order` (0-based indices into the open document,
/// in the wanted order) to `path`.
#[tauri::command]
async fn save_document(
    state: State<'_, AppState>,
    path: String,
    order: Vec<usize>,
) -> Result<SaveReport, AppError> {
    let guard = state.session();
    let session = guard
        .as_ref()
        .ok_or_else(|| AppError::other("aucun document ouvert"))?;
    session.save(&order, &PathBuf::from(path))
}

/// The file given on the command line (`fyp-app document.pdf`), opened
/// by the interface when it starts; `None` otherwise.
#[tauri::command]
fn initial_file() -> Option<String> {
    std::env::args_os()
        .nth(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .filter(|arg| !arg.is_empty())
}

/// Native "open file" dialog; `None` when cancelled.
#[tauri::command]
async fn pick_open_file(app: tauri::AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .add_filter("PDF", &["pdf"])
        .blocking_pick_file()
        .and_then(|f| f.into_path().ok())
        .map(|p| p.display().to_string())
}

/// Native "save as" dialog with a suggested name; `None` when cancelled.
#[tauri::command]
async fn pick_save_file(app: tauri::AppHandle, suggested: String) -> Option<String> {
    app.dialog()
        .file()
        .add_filter("PDF", &["pdf"])
        .set_file_name(suggested)
        .blocking_save_file()
        .and_then(|f| f.into_path().ok())
        .map(|p| p.display().to_string())
}

fn main() {
    let render = Arc::new(RenderService::start(&render::library_candidates()));
    let state = AppState {
        session: Mutex::new(None),
        render,
        next_id: AtomicU64::new(1),
    };
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            open_document,
            close_document,
            renderer_status,
            initial_file,
            render_page,
            rotate_pages,
            save_document,
            pick_open_file,
            pick_save_file,
        ])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("4YouPDF: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures")
            .join(name)
    }

    /// When a file does not open, the interface keeps showing the current
    /// document and its notices (`ui/tests/notices.test.ts`): it must stay
    /// open here as well, for its pages to render and to be saved.
    #[test]
    fn a_failed_open_keeps_the_current_document() {
        let state = AppState {
            session: Mutex::new(None),
            render: Arc::new(RenderService::start(&[])),
            next_id: AtomicU64::new(1),
        };
        let current = || {
            state
                .session()
                .as_ref()
                .map(|s| (s.id, s.info.path.clone()))
        };
        let damaged = state.open(&fixture("bad-offsets.pdf"), "").expect("open");
        assert!(damaged.reconstructed.is_some());
        let before = current();
        assert!(before.is_some());

        let not_pdf = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert!(matches!(
            state.open(&not_pdf, ""),
            Err(AppError::Other { .. })
        ));
        assert!(matches!(
            state.open(&fixture("absent.pdf"), ""),
            Err(AppError::Other { .. })
        ));
        assert!(matches!(
            state.open(&fixture("encrypted-aes256.pdf"), "nope"),
            Err(AppError::WrongPassword)
        ));
        assert_eq!(current(), before);

        // A file that opens does replace it.
        let clean = state.open(&fixture("minimal.pdf"), "").expect("open");
        assert_eq!(current().map(|(_, path)| path), Some(clean.path));
    }

    /// A rotation replaces the document it was made from, and only that
    /// one: neither a file opened while it was computed, nor a file opened
    /// before it was even asked for, is turned in place of the one meant.
    #[test]
    fn a_rotation_applies_only_to_the_document_it_was_meant_for() {
        let state = AppState {
            session: Mutex::new(None),
            render: Arc::new(RenderService::start(&[])),
            next_id: AtomicU64::new(1),
        };
        let bytes_of = |state: &AppState| state.current().map(|c| (c.id, c.bytes));
        assert!(state.rotate(1, &[0], 90).is_err(), "no document open");
        let first = state.open(&fixture("minimal.pdf"), "").expect("open");
        let (id, bytes) = bytes_of(&state).unwrap();

        let pages = state.rotate(first.document, &[0], 90).expect("rotate");
        assert_eq!(pages[0].rotate, 90);
        let (turned, turned_bytes) = bytes_of(&state).unwrap();
        assert_ne!(turned, id, "the renderer must not reuse its images");
        assert_ne!(turned_bytes, bytes);

        // Computed from the first opening, committed after another one.
        let late = session::rotate(&turned_bytes, "", &[0], 90).expect("rotate");
        let second = state.open(&fixture("minimal.pdf"), "").expect("open again");
        let reopened = bytes_of(&state).unwrap();
        assert!(state.commit(first.document, turned, late).is_err());
        // Asked for the first opening, once the second is current.
        assert!(state.rotate(first.document, &[0], 90).is_err());
        assert_eq!(bytes_of(&state).unwrap(), reopened);
        assert_eq!(
            state.rotate(second.document, &[0], -90).expect("rotate")[0].rotate,
            270
        );
    }
}
