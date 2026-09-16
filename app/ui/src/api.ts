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

export function saveDocument(path: string, order: number[]): Promise<SaveReport> {
  return invoke<SaveReport>("save_document", { path, order });
}

export function initialFile(): Promise<string | null> {
  return invoke<string | null>("initial_file");
}

export function pickOpenFile(): Promise<string | null> {
  return invoke<string | null>("pick_open_file");
}

export function pickSaveFile(suggested: string): Promise<string | null> {
  return invoke<string | null>("pick_save_file", { suggested });
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
