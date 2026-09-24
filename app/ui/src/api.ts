// Typed façade over the commands of `app/src/main.rs`. Everything the
// interface knows about the Rust side is here.

export interface PageInfo {
  width: number;
  height: number;
  rotate: number;
}

export interface DocumentInfo {
  /// This opening of the file: the commands that change the document name
  /// it, so that one meant for a document replaced meanwhile is refused.
  document: number;
  path: string;
  name: string;
  size: number;
  version: string;
  pages: PageInfo[];
  reconstructed: string | null;
  relocated_startxref: number | null;
  encryption: string | null;
}

export interface SaveReport {
  path: string;
  size: number;
  pages: number;
}

/// What became of one file asked to be merged (`session.rs`,
/// `SourceOutcome`): merged, its pages after those of the document;
/// protected by a password, which merging does not ask for; refused, not
/// read or not opened even after repair.
export type SourceOutcome =
  | { kind: "merged"; pages: number; reconstructed: string | null; encryption: string | null }
  | { kind: "protected" }
  | { kind: "refused"; message: string };

export interface SourceReport {
  path: string;
  name: string;
  outcome: SourceOutcome;
}

export interface MergeReport {
  /// Every page as it now stands, or `null` when no file could be merged
  /// and the document is as it was.
  pages: PageInfo[] | null;
  /// One entry per file asked for, in the order given.
  sources: SourceReport[];
}

/// One file written by a cut (`session.rs`, `SplitFile`): its name alone,
/// the folder being the same for all of them.
export interface SplitFile {
  name: string;
  pages: number;
  size: number;
}

export interface SplitReport {
  dir: string;
  /// One entry per file written, in the order they were written.
  files: SplitFile[];
}

export interface RendererStatus {
  available: boolean;
  detail: string;
}

export type AppError =
  | { kind: "wrong_password" }
  | { kind: "other"; message: string };

/// `true` when the interface runs inside Tauri (not in a plain browser).
export function hasTauri(): boolean {
  return window.__TAURI__ !== undefined;
}

function tauri(): TauriGlobal {
  const t = window.__TAURI__;
  if (t === undefined) {
    throw new Error("Tauri n'est pas disponible dans cette page");
  }
  return t;
}

function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return tauri().core.invoke<T>(command, args);
}

/// Turn whatever `invoke` rejected with into an `AppError`.
export function asAppError(e: unknown): AppError {
  if (typeof e === "object" && e !== null && "kind" in e) {
    const kind = (e as { kind: unknown }).kind;
    if (kind === "wrong_password") {
      return { kind: "wrong_password" };
    }
    if (kind === "other" && "message" in e) {
      return { kind: "other", message: String((e as { message: unknown }).message) };
    }
  }
  return { kind: "other", message: String(e) };
}

export function openDocument(path: string, password?: string): Promise<DocumentInfo> {
  return invoke<DocumentInfo>("open_document", { path, password: password ?? null });
}

export function closeDocument(): Promise<void> {
  return invoke<void>("close_document");
}

/// Tell the Rust side whether the document carries unsaved changes: it
/// then holds the window open when closing is asked for, and asks the
/// interface instead (`onCloseRequested`).
export function documentModified(modified: boolean): Promise<void> {
  return invoke<void>("document_modified", { modified });
}

/// Close the window for good, whatever the document carries: the Rust side
/// asks nothing more.
export function closeWindow(): Promise<void> {
  return invoke<void>("close_window");
}

/// Closing the window was asked for (the cross, Alt+F4, the system menu)
/// while the document was reported modified: the Rust side held it open,
/// and `handler` decides.
export function onCloseRequested(handler: () => void): Promise<() => void> {
  return tauri().event.listen<null>("fyp://close-requested", () => handler());
}

export function rendererStatus(): Promise<RendererStatus> {
  return invoke<RendererStatus>("renderer_status");
}

export function renderPage(page: number, width: number): Promise<string> {
  return invoke<string>("render_page", { page, width });
}

/// Turn the pages at `pages` (0-based indices into the file opened as
/// `document`) by `degrees` clockwise, relative to their rotation: done on
/// the Rust side, which answers with every page as it now stands.
export function rotatePages(document: number, pages: number[], degrees: number): Promise<PageInfo[]> {
  return invoke<PageInfo[]>("rotate_pages", { document, pages, degrees });
}

/// Append every page of the files at `paths`, in that order, to the file
/// opened as `document`: done on the Rust side, which answers with every
/// page as it now stands, and what became of each file.
export function mergeDocuments(document: number, paths: string[]): Promise<MergeReport> {
  return invoke<MergeReport>("merge_documents", { document, paths });
}

/// Write the pages at `order` (0-based indices into the open file, in the
/// wanted order) to `path`: the whole document when saving it, the pages
/// selected when extracting them. The open document does not change.
export function saveDocument(path: string, order: number[]): Promise<SaveReport> {
  return invoke<SaveReport>("save_document", { path, order });
}

/// Write each part of `parts` (0-based indices into the open file, in the
/// wanted order) to its own file of the folder `dir`, named after the
/// document. No existing file is replaced: when a name is taken, nothing
/// is written and the rejection names it. The open document does not
/// change.
export function splitDocument(parts: number[][], dir: string): Promise<SplitReport> {
  return invoke<SplitReport>("split_document", { parts, dir });
}

export function initialFile(): Promise<string | null> {
  return invoke<string | null>("initial_file");
}

export function pickOpenFile(): Promise<string | null> {
  return invoke<string | null>("pick_open_file");
}

/// The files to merge, several at once, in the order chosen; `null` when
/// cancelled.
export function pickMergeFiles(): Promise<string[] | null> {
  return invoke<string[] | null>("pick_merge_files");
}

export function pickSaveFile(suggested: string): Promise<string | null> {
  return invoke<string | null>("pick_save_file", { suggested });
}

/// The folder the files of a cut go into; `null` when cancelled.
export function pickFolder(): Promise<string | null> {
  return invoke<string | null>("pick_folder");
}

export function onFileDrop(handler: (paths: string[]) => void): Promise<() => void> {
  return tauri()
    .webview.getCurrentWebview()
    .onDragDropEvent((event) => {
      if (event.payload.type === "drop" && event.payload.paths !== undefined) {
        handler(event.payload.paths);
      }
    });
}

export function setWindowTitle(title: string): Promise<void> {
  return tauri().window.getCurrentWindow().setTitle(title);
}
