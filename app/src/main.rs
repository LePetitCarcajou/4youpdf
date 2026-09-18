//! 4YouPDF desktop application (milestone 0.3, first step): one window
//! that opens a PDF, shows its pages as thumbnails, lets the user reorder,
//! turn and delete them, and saves the result through `fyp_core::ops`.
//!
//! The interface (`ui/`, TypeScript) talks to this side through the
//! commands below and nothing else: file dialogs, reading, rendering,
//! turning pages, merging files and writing all happen here. The page
//! order and the undo history live in the interface; this side only knows
//! the open document, as turned and extended so far: the interface undoes
//! a rotation by asking for the opposite one, and a merge by leaving the
//! pages merged out of the order it saves.
//!
//! Whether the document carries unsaved changes is decided by the
//! interface too, which reports it here (`document_modified`): on that
//! word, a request to close the window (the cross, Alt+F4, the system
//! menu) is refused and handed to the interface, which asks in place what
//! to do, and closes the window itself (`close_window`) once that is
//! settled.

#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod render;
mod session;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use base64::Engine as _;
use serde::Serialize;
use tauri::{Emitter, Manager, State, WindowEvent};
use tauri_plugin_dialog::DialogExt;

use render::RenderService;
use session::{DocumentInfo, MergeReport, PageInfo, SaveReport, Session};

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
    /// Whether the document carries unsaved changes, as the interface last
    /// reported (`document_modified`): what holds the window open when
    /// closing is asked for. A document just opened, or closed, has none.
    unsaved: AtomicBool,
}

impl AppState {
    fn new(render: Arc<RenderService>) -> AppState {
        AppState {
            session: Mutex::new(None),
            render,
            next_id: AtomicU64::new(1),
            unsaved: AtomicBool::new(false),
        }
    }

    fn session(&self) -> std::sync::MutexGuard<'_, Option<Session>> {
        self.session.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Open `path` and make it the current document. The current document
    /// is replaced only once the new one is open: when opening fails, it
    /// stays current, as the interface keeps showing it, with its unsaved
    /// changes.
    fn open(&self, path: &Path, password: &str) -> Result<DocumentInfo, AppError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let session = Session::open(id, path, password)?;
        let info = session.info.clone();
        *self.session() = Some(session);
        self.unsaved.store(false, Ordering::Relaxed);
        Ok(info)
    }

    /// Forget the current document, and the unsaved changes it carried.
    fn close(&self) {
        *self.session() = None;
        self.unsaved.store(false, Ordering::Relaxed);
    }

    /// What the interface reports about unsaved changes.
    fn set_unsaved(&self, unsaved: bool) {
        self.unsaved.store(unsaved, Ordering::Relaxed);
    }

    /// Whether a request to close the window must be refused and handed to
    /// the interface: work would be lost otherwise.
    fn asks_before_closing(&self) -> bool {
        self.unsaved.load(Ordering::Relaxed)
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
            return Err(changed(ROTATION));
        }
        let rotated = session::rotate(&current.bytes, &current.password, pages, degrees)?;
        self.commit(document, current.id, ROTATION, |session, id| {
            session.replace(id, rotated)
        })
    }

    /// Append every page of each file of `files`, in that order, to the
    /// opening `document` (`session::merge`) and make the result current;
    /// returns every page as it now stands, and what became of each file.
    /// A file that does not open is skipped, said so, and the others are
    /// merged without it; when none opens, the document is left as it is.
    /// Like a rotation, the rewrite runs without holding the document.
    fn merge(&self, document: u64, files: &[PathBuf]) -> Result<MergeReport, AppError> {
        let current = self.current()?;
        if current.document != document {
            return Err(changed(MERGE));
        }
        let merged = session::merge(&current.bytes, &current.password, files)?;
        let pages = match merged.rewrite {
            Some(rewrite) => Some(self.commit(document, current.id, MERGE, |session, id| {
                session.extend(id, rewrite, merged.added)
            })?),
            None => None,
        };
        Ok(MergeReport {
            pages,
            sources: merged.sources,
        })
    }

    /// Apply `take` to the current document, made from the bytes known as
    /// `from` of the opening `document`, under a new id for the renderer.
    /// Refused when they are no longer current (another file opened, the
    /// document closed, turned or extended meanwhile): `what` is never
    /// applied to what it was not made from.
    fn commit<T>(
        &self,
        document: u64,
        from: u64,
        what: &str,
        take: impl FnOnce(&mut Session, u64) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let mut guard = self.session();
        match guard.as_mut() {
            Some(session) if session.info.document == document && session.id == from => {
                take(session, self.next_id.fetch_add(1, Ordering::Relaxed))
            }
            _ => Err(changed(what)),
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

/// The edits `changed` names.
const ROTATION: &str = "la rotation";
const MERGE: &str = "la fusion";

fn changed(what: &str) -> AppError {
    AppError::other(format!(
        "le document a changé ; {what} n'a pas été appliquée"
    ))
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
    state.close();
}

/// The interface says whether the document carries unsaved changes
/// (`history.ts`, `unsaved`): from then on, closing the window asks first.
#[tauri::command]
fn document_modified(state: State<'_, AppState>, modified: bool) {
    state.set_unsaved(modified);
}

/// Close the window for good: the interface settled what to do with the
/// unsaved changes, or there were none. `destroy` closes without asking
/// again, unlike `close`, which would raise `CloseRequested` once more.
#[tauri::command]
fn close_window(window: tauri::Window) -> Result<(), AppError> {
    window
        .destroy()
        .map_err(|e| AppError::other(format!("la fenêtre ne se ferme pas : {e}")))
}

/// The event the interface listens to when a request to close the window
/// is refused here: it asks what to do, then calls `close_window` or not.
const CLOSE_REQUESTED: &str = "fyp://close-requested";

/// Whether a request to close the window must be refused: the document
/// carries unsaved changes, and the interface could be told (`ask`), so
/// that it asks in place. When it cannot be told, the window closes: a
/// window nobody can close again is worse than the question not asked.
fn refuses_closing(state: Option<&AppState>, ask: impl FnOnce() -> tauri::Result<()>) -> bool {
    match state {
        Some(state) if state.asks_before_closing() => match ask() {
            Ok(()) => true,
            Err(e) => {
                eprintln!("4YouPDF: closing without asking, the interface could not be told: {e}");
                false
            }
        },
        _ => false,
    }
}

/// Window events: a request to close (the cross, Alt+F4, the system menu)
/// goes through [`refuses_closing`]. No `unsafe`: Tauri exposes the
/// request with an api whose `prevent_close` keeps the window open, and
/// the interface is told by an event.
fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        let state = window.try_state::<AppState>();
        if refuses_closing(state.as_deref(), || window.emit(CLOSE_REQUESTED, ())) {
            api.prevent_close();
        }
    }
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

/// Append every page of each file of `paths`, in that order, to the open
/// document, through `ops::merge` (see `AppState::merge`): they render and
/// save after its own from then on. `document` is the opening the
/// interface means (`DocumentInfo::document`). Returns every page as it
/// now stands, and what became of each file.
#[tauri::command]
async fn merge_documents(
    app: tauri::AppHandle,
    document: u64,
    paths: Vec<String>,
) -> Result<MergeReport, AppError> {
    // Reading the files and rewriting the document take a while: off the
    // async runtime's threads, like a rotation.
    tauri::async_runtime::spawn_blocking(move || {
        let files: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        app.try_state::<AppState>()
            .ok_or_else(|| AppError::other("état de l'application indisponible"))?
            .merge(document, &files)
    })
    .await
    .map_err(|e| AppError::other(format!("fusion interrompue : {e}")))?
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

/// Native "open files" dialog for the files to merge, several at once, in
/// the order chosen; `None` when cancelled.
#[tauri::command]
async fn pick_merge_files(app: tauri::AppHandle) -> Option<Vec<String>> {
    app.dialog()
        .file()
        .add_filter("PDF", &["pdf"])
        .set_title("Fusionner à la suite du document")
        .blocking_pick_files()
        .map(|files| {
            files
                .into_iter()
                .filter_map(|f| f.into_path().ok())
                .map(|p| p.display().to_string())
                .collect()
        })
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

/// Name of the folder, next to the executable, that makes a copy portable:
/// the portable archive ships it empty (`tools/package_app.py`). WebView2
/// then keeps its profile there instead of
/// `%LOCALAPPDATA%\org.fouryoupdf.desktop`, and deleting the folder of the
/// application deletes all it wrote.
const PORTABLE_DATA: &str = "data";

/// The profile folder of a portable copy whose executable is in `exe_dir`;
/// `None` for an installed copy, and for a portable folder that cannot be
/// written to (read-only medium), where WebView2 would not start.
fn portable_data_dir(exe_dir: &Path) -> Option<PathBuf> {
    let dir = exe_dir.join(PORTABLE_DATA);
    if !dir.is_dir() {
        return None;
    }
    let probe = dir.join(".write-test");
    std::fs::write(&probe, b"").ok()?;
    let _ = std::fs::remove_file(&probe);
    Some(dir)
}

/// The title of the main window: the one of `tauri.conf.json`, followed by
/// « — DEV » in a debug build (`cargo run`, `cargo tauri dev`), so that the
/// build being tried is never taken for a packaged one: a fix tried in the
/// wrong executable shows nothing, and nothing said so. A release build
/// keeps the title as configured. The one visible difference between the
/// two builds, and it names itself.
fn window_title(configured: &str) -> String {
    if cfg!(debug_assertions) {
        format!("{configured} — DEV")
    } else {
        configured.to_owned()
    }
}

/// Open the main window described in `tauri.conf.json`, where `create` is
/// `false` so that it opens here: under the title of this build
/// ([`window_title`]), and in the profile folder of a portable copy when
/// this is one ([`portable_data_dir`]).
fn open_main_window(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .ok_or("tauri.conf.json ne décrit pas la fenêtre « main »")?;
    let mut window = tauri::WebviewWindowBuilder::from_config(app.handle(), config)?
        .title(window_title(&config.title));
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    if let Some(data) = exe_dir.as_deref().and_then(portable_data_dir) {
        window = window.data_directory(data);
    }
    window.build()?;
    Ok(())
}

/// Stop with a message box when WebView2 is missing (a portable copy on a
/// machine without it, or an installed copy whose WebView2 was removed
/// since; the installer itself stops when it cannot install WebView2): no
/// window can open, and a program without a console would otherwise exit
/// without a word. The one dialog outside ADR 0004, shown only when the
/// window cannot exist.
#[cfg(windows)]
fn require_webview2() {
    if let Err(e) = tauri::webview_version() {
        let _ = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("4YouPDF")
            .set_description(format!(
                "4YouPDF ne peut pas s'ouvrir : Microsoft Edge WebView2 Runtime, \
                 le moteur d'affichage de Windows dont il se sert, est introuvable \
                 sur cet ordinateur.\n\n\
                 Installez-le depuis le site de Microsoft \
                 (https://developer.microsoft.com/microsoft-edge/webview2/), \
                 puis relancez 4YouPDF. L'installeur de 4YouPDF s'en charge \
                 lui-même quand il manque, avec une connexion à Internet.\n\n\
                 Détail : {e}"
            ))
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
        std::process::exit(1);
    }
}

fn main() {
    #[cfg(windows)]
    require_webview2();
    // A development build (`cargo run`, not `tauri build`) also finds PDFium
    // in app/pdfium/ of its checkout; a packaged build only next to itself.
    let development = tauri::is_dev().then(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("pdfium"));
    let render = Arc::new(RenderService::start(&render::library_candidates(
        development,
    )));
    let state = AppState::new(render);
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .setup(|app| open_main_window(app))
        .on_window_event(on_window_event)
        .invoke_handler(tauri::generate_handler![
            open_document,
            close_document,
            document_modified,
            close_window,
            renderer_status,
            initial_file,
            render_page,
            rotate_pages,
            merge_documents,
            save_document,
            pick_open_file,
            pick_merge_files,
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

    /// A debug build says so in its title and a release build does not:
    /// what this checks follows the profile `cargo test` runs under, as
    /// the window does.
    #[test]
    fn the_title_marks_a_debug_build_only() {
        let expected = if cfg!(debug_assertions) {
            "4YouPDF — DEV"
        } else {
            "4YouPDF"
        };
        assert_eq!(window_title("4YouPDF"), expected);
    }

    /// When a file does not open, the interface keeps showing the current
    /// document and its notices (`ui/tests/notices.test.ts`): it must stay
    /// open here as well, for its pages to render and to be saved.
    #[test]
    fn a_failed_open_keeps_the_current_document() {
        let state = AppState::new(Arc::new(RenderService::start(&[])));
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
        let state = AppState::new(Arc::new(RenderService::start(&[])));
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
        assert!(state
            .commit(first.document, turned, ROTATION, |s, id| s
                .replace(id, late))
            .is_err());
        // Asked for the first opening, once the second is current.
        assert!(state.rotate(first.document, &[0], 90).is_err());
        assert_eq!(bytes_of(&state).unwrap(), reopened);
        assert_eq!(
            state.rotate(second.document, &[0], -90).expect("rotate")[0].rotate,
            270
        );
    }

    /// A merge extends the document it was meant for, and only that one,
    /// like a rotation; one that merges nothing leaves it as it is.
    #[test]
    fn a_merge_applies_only_to_the_document_it_was_meant_for() {
        let state = AppState::new(Arc::new(RenderService::start(&[])));
        let bytes_of = |state: &AppState| state.current().map(|c| (c.id, c.bytes));
        let files = [fixture("objstm.pdf")];
        assert!(state.merge(1, &files).is_err(), "no document open");
        let first = state.open(&fixture("minimal.pdf"), "").expect("open");
        let (id, _) = bytes_of(&state).unwrap();

        let report = state.merge(first.document, &files).expect("merge");
        assert_eq!(report.pages.map(|pages| pages.len()), Some(2));
        assert_eq!(report.sources.len(), 1);
        let (extended, _) = bytes_of(&state).unwrap();
        assert_ne!(extended, id, "the renderer must not reuse its images");

        // Nothing to merge: the document, and its id, stay.
        let report = state
            .merge(first.document, &[fixture("encrypted-user-password.pdf")])
            .expect("merge");
        assert!(report.pages.is_none());
        assert_eq!(bytes_of(&state).unwrap().0, extended);

        // Asked for the first opening, once a second is current.
        let second = state.open(&fixture("minimal.pdf"), "").expect("open again");
        assert!(state.merge(first.document, &files).is_err());
        assert_eq!(state.current().unwrap().document, second.document);
        assert_eq!(state.rotate(second.document, &[0], 90).unwrap().len(), 1);
    }

    /// Closing asks only on the interface's word that the document carries
    /// unsaved changes, and only while that word stands: a document just
    /// opened, or closed, has none. When the interface cannot be told, the
    /// window closes rather than staying open for good.
    #[test]
    fn closing_asks_only_while_the_document_is_reported_modified() {
        let state = AppState::new(Arc::new(RenderService::start(&[])));
        let told = std::cell::Cell::new(0);
        let tell = || {
            told.set(told.get() + 1);
            Ok(())
        };
        assert!(!refuses_closing(None, tell), "no state managed");
        assert!(!refuses_closing(Some(&state), tell), "nothing open");

        state.open(&fixture("minimal.pdf"), "").expect("open");
        assert!(!refuses_closing(Some(&state), tell), "just opened");
        state.set_unsaved(true);
        assert!(refuses_closing(Some(&state), tell), "modified");
        assert_eq!(told.get(), 1, "the interface is told once, when refused");
        assert!(
            !refuses_closing(Some(&state), || Err(tauri::Error::WebviewNotFound)),
            "the interface cannot be told: the window closes"
        );
        assert!(state.asks_before_closing(), "still modified");

        state.set_unsaved(false);
        assert!(!refuses_closing(Some(&state), tell), "saved");
        state.set_unsaved(true);
        state
            .open(&fixture("minimal.pdf"), "")
            .expect("open another");
        assert!(
            !refuses_closing(Some(&state), tell),
            "another document opened"
        );
        state.set_unsaved(true);
        assert!(
            state.open(&fixture("absent.pdf"), "").is_err() && refuses_closing(Some(&state), tell),
            "a failed opening keeps the document, and its changes"
        );
        state.close();
        assert!(!refuses_closing(Some(&state), tell), "closed");
        assert_eq!(told.get(), 2, "told only when refused");
    }

    /// A copy is portable when a `data` folder sits next to its executable,
    /// as in the portable archive; installed, or a plain build, otherwise.
    #[test]
    fn a_data_folder_next_to_the_executable_makes_a_copy_portable() {
        let dir = std::env::temp_dir().join(format!("fyp-app-portable-{}", std::process::id()));
        let data = dir.join(PORTABLE_DATA);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(portable_data_dir(&dir), None);
        std::fs::write(&data, b"").unwrap();
        assert_eq!(portable_data_dir(&dir), None, "a file is not the folder");
        std::fs::remove_file(&data).unwrap();
        std::fs::create_dir(&data).unwrap();
        assert_eq!(portable_data_dir(&dir), Some(data.clone()));
        // Checking that the folder can be written to leaves nothing in it.
        assert_eq!(std::fs::read_dir(&data).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Tauri must not open the main window itself: `open_main_window` does,
    /// and a second window with the same label would stop the application
    /// as it starts. The configuration has no version either: the
    /// workspace's is the only one (build.rs).
    #[test]
    fn the_configuration_leaves_the_window_and_the_version_to_the_application() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let windows = config["app"]["windows"].as_array().unwrap();
        let main = windows.iter().find(|w| w["label"] == "main").unwrap();
        assert_eq!(main["create"], false);
        assert!(config.get("version").is_none());
    }

    /// Ctrl+wheel, Ctrl+plus and Ctrl+minus would zoom the interface itself.
    /// Tauri keeps the zoom of WebView2 off, keys, wheel and pinch, unless a
    /// window sets `zoomHotkeysEnabled`, false by default: in
    /// `tauri.conf.json`, in `tauri.bundle.json`, which the packaging merges
    /// into it, or in a configuration file for Windows, which Tauri merges
    /// too. The interface prevents the zoom keys as well, but not the wheel,
    /// whose listener would make every scroll of the grid wait for it
    /// (app/README.md, « Raccourcis du navigateur neutralisés »).
    #[test]
    fn the_configuration_keeps_the_zoom_of_the_webview_off() {
        let config: tauri::utils::config::Config =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert!(!config.app.windows.is_empty());
        for window in &config.app.windows {
            assert!(!window.zoom_hotkeys_enabled, "window {}", window.label);
        }
        let bundle: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.bundle.json")).unwrap();
        assert!(
            bundle.get("app").is_none(),
            "tauri.bundle.json sets windows"
        );
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        for name in [
            "tauri.windows.conf.json",
            "tauri.windows.conf.json5",
            "Tauri.windows.toml",
        ] {
            assert!(!dir.join(name).exists(), "{name}");
        }
    }
}
