// The window: open a PDF (dialog, drop, Ctrl+O), show its pages as
// tiles, reorder them by dragging, turn them, delete them, merge other
// files after them (Ctrl+M), undo and redo, look at one page at a time,
// beside the grid reduced to a panel of thumbnails or over it, save
// through `fyp_core::ops` on the Rust side.
// One window, no modal dialog but the system file pickers; every message
// appears in place (ADR 0004). Unsaved changes are never lost without a
// word: closing the window or opening another file first asks, in place,
// what to do with them (see « Leaving »).

import {
  asAppError,
  closeDocument,
  closeWindow,
  documentModified,
  hasTauri,
  initialFile,
  mergeDocuments,
  onCloseRequested,
  onFileDrop,
  openDocument,
  pickMergeFiles,
  pickOpenFile,
  pickSaveFile,
  rendererStatus,
  rotatePages,
  saveDocument,
  setWindowTitle,
  type DocumentInfo,
  type SourceReport,
} from "./api.js";
import { PageHistory, type Outcome } from "./history.js";
import { mergeNotices, mergeStatus } from "./merge.js";
import { attemptOpen, choices, NoticeBoard, type Leaving, type Notice, type NoticeKind } from "./notices.js";
import { browserShortcut, stopsHere } from "./shortcuts.js";
import { ThumbnailLoader, thumbnailSlots } from "./thumbnails.js";
import { PageViewer, pageRatio } from "./viewer.js";

const THUMB_WIDTH = 160;

function element<T extends HTMLElement>(id: string): T {
  const el = document.getElementById(id);
  if (el === null) {
    throw new Error(`élément #${id} absent de la page`);
  }
  return el as T;
}

const ui = {
  open: element<HTMLButtonElement>("open"),
  openEmpty: element<HTMLButtonElement>("open-empty"),
  merge: element<HTMLButtonElement>("merge"),
  docName: element<HTMLSpanElement>("doc-name"),
  undo: element<HTMLButtonElement>("undo"),
  redo: element<HTMLButtonElement>("redo"),
  rotateLeft: element<HTMLButtonElement>("rotate-left"),
  rotateRight: element<HTMLButtonElement>("rotate-right"),
  delete: element<HTMLButtonElement>("delete"),
  save: element<HTMLButtonElement>("save"),
  notices: element<HTMLElement>("notices"),
  workspace: element<HTMLElement>("workspace"),
  gridRoot: element<HTMLElement>("grid-root"),
  grid: element<HTMLElement>("grid"),
  empty: element<HTMLElement>("empty"),
  dropMarker: element<HTMLElement>("drop-marker"),
  statusText: element<HTMLSpanElement>("status-text"),
  rendererStatus: element<HTMLSpanElement>("renderer-status"),
  contextMenu: element<HTMLElement>("context-menu"),
  dropOverlay: element<HTMLElement>("drop-overlay"),
  viewer: element<HTMLElement>("viewer"),
  viewerStage: element<HTMLElement>("viewer-stage"),
  viewerPage: element<HTMLElement>("viewer-page"),
  viewerPanel: element<HTMLButtonElement>("viewer-panel"),
  viewerCaption: element<HTMLSpanElement>("viewer-caption"),
  viewerNumberForm: element<HTMLFormElement>("viewer-number-form"),
  viewerNumber: element<HTMLInputElement>("viewer-number"),
  viewerNumberAfter: element<HTMLSpanElement>("viewer-number-after"),
  viewerHint: element<HTMLSpanElement>("viewer-hint"),
  viewerHelp: element<HTMLSpanElement>("viewer-help"),
  viewerPrev: element<HTMLButtonElement>("viewer-prev"),
  viewerNext: element<HTMLButtonElement>("viewer-next"),
  viewerClose: element<HTMLButtonElement>("viewer-close"),
  viewerRotateLeft: element<HTMLButtonElement>("viewer-rotate-left"),
  viewerRotateRight: element<HTMLButtonElement>("viewer-rotate-right"),
  viewerZoomOut: element<HTMLButtonElement>("viewer-zoom-out"),
  viewerZoomLevel: element<HTMLButtonElement>("viewer-zoom-level"),
  viewerZoomIn: element<HTMLButtonElement>("viewer-zoom-in"),
};

interface State {
  info: DocumentInfo | null;
  history: PageHistory | null;
  selection: Set<number>;
  rendererAvailable: boolean;
  /// Whether the page view keeps the grid beside it as a panel of
  /// thumbnails (F4, button `Vignettes`). A state of the window, not of the
  /// document (ADR 0004): it stays as it is from one page to the next, from
  /// one opening of the view to the next, and for another document.
  panel: boolean;
}

const state: State = {
  info: null,
  history: null,
  selection: new Set(),
  rendererAvailable: false,
  panel: true,
};

const thumbnails = new ThumbnailLoader(ui.gridRoot, THUMB_WIDTH, false, () =>
  thumbnailSlots({ open: viewer.isOpen, panel: state.panel, drawing: viewer.isDrawing }),
);

const viewer = new PageViewer(
  {
    root: ui.viewer,
    stage: ui.viewerStage,
    page: ui.viewerPage,
    caption: ui.viewerCaption,
    numberForm: ui.viewerNumberForm,
    number: ui.viewerNumber,
    numberAfter: ui.viewerNumberAfter,
    hint: ui.viewerHint,
    help: ui.viewerHelp,
    prev: ui.viewerPrev,
    next: ui.viewerNext,
    close: ui.viewerClose,
    rotateLeft: ui.viewerRotateLeft,
    rotateRight: ui.viewerRotateRight,
    zoomOut: ui.viewerZoomOut,
    zoomLevel: ui.viewerZoomLevel,
    zoomIn: ui.viewerZoomIn,
  },
  {
    thumbnail: (page) => thumbnails.cached(page),
    closed: viewerClosed,
    rotate: (page, degrees) => void turnPages([page], degrees, true),
    turning: (page) => state.history?.isTurning(page) === true,
    shown: markShown,
    idle: () => thumbnails.wake(),
  },
);

// ---------------------------------------------------------------------------
// Notices and status
// ---------------------------------------------------------------------------

/// The notices above the grid, as data (notices.ts), and the box drawn for
/// each one on screen.
const notices = new NoticeBoard();
const noticeBoxes = new Map<number, HTMLElement>();

/// Draw the notices of the board. A box stays in place as long as its
/// notice does (a password being typed keeps its field); the board adds
/// notices at the end only, so new boxes go at the end.
function renderNotices(): void {
  const shown = new Set(notices.list.map((n) => n.id));
  for (const [id, box] of noticeBoxes) {
    if (!shown.has(id)) {
      box.remove();
      noticeBoxes.delete(id);
    }
  }
  for (const n of notices.list) {
    if (!noticeBoxes.has(n.id)) {
      const box = noticeBox(n);
      noticeBoxes.set(n.id, box);
      ui.notices.append(box);
      box.querySelector<HTMLElement>("input, [data-autofocus]")?.focus();
    }
  }
}

function noticeBox(n: Notice): HTMLElement {
  const box = document.createElement("div");
  box.className = `notice ${n.kind}`;
  const span = document.createElement("span");
  span.textContent = n.text;
  box.append(span);
  if (n.role === "password" && n.path !== null) {
    box.append(passwordForm(n.path));
  }
  if (n.role === "question" && n.leaving !== null) {
    // Answered by its three buttons only: no cross, and nothing else
    // dismisses it (ADR 0004, an explicit stop for the irreversible).
    box.append(questionChoices(n.leaving));
    return box;
  }
  const close = document.createElement("button");
  close.type = "button";
  close.className = "close";
  close.textContent = "×";
  close.title = "Fermer";
  close.addEventListener("click", () => {
    notices.close(n.id);
    renderNotices();
  });
  box.append(close);
  return box;
}

/// The password field of a request, in its notice, never a modal.
function passwordForm(path: string): HTMLFormElement {
  const form = document.createElement("form");
  const input = document.createElement("input");
  input.type = "password";
  input.placeholder = "Mot de passe";
  input.autocomplete = "off";
  const submit = document.createElement("button");
  submit.type = "submit";
  submit.textContent = "Ouvrir";
  form.append(input, submit);
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    void requestOpen(path, input.value);
  });
  return form;
}

/// The three ways out of the question asked before `leaving`, as buttons.
/// The keyboard goes to « Annuler »: Enter pressed by reflex loses nothing.
function questionChoices(leaving: Leaving): HTMLElement {
  const row = document.createElement("div");
  row.className = "choices";
  for (const choice of choices(leaving)) {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = choice.label;
    if (choice.action === "save") {
      button.className = "primary";
    }
    if (choice.action === "cancel") {
      button.dataset["autofocus"] = "";
    }
    button.addEventListener("click", () => void answered(leaving, choice.action));
    row.append(button);
  }
  return row;
}

/// Report something that happened, in place, until closed or until a
/// document opens.
function notice(kind: NoticeKind, text: string): void {
  notices.event(kind, text);
  renderNotices();
}

function setStatus(text: string): void {
  ui.statusText.textContent = text;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} o`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} Kio`;
  }
  return `${(bytes / (1024 * 1024)).toFixed(1)} Mio`;
}

// ---------------------------------------------------------------------------
// Opening
// ---------------------------------------------------------------------------

/// Open `path`. The document on screen, its notices and the status bar
/// change only once the new file is open; when it does not open, a notice
/// says why or asks for its password, and the rest stays as it was.
async function open(path: string, password?: string): Promise<void> {
  const status = ui.statusText.textContent ?? "";
  setStatus(`Ouverture de ${path}…`);
  const opening = await attemptOpen(notices, openDocument, path, password);
  if (opening.kind === "opened") {
    loaded(opening.info);
  } else {
    setStatus(status);
  }
  renderNotices();
}

/// Show the document just opened; its notices are already on the board.
function loaded(info: DocumentInfo): void {
  state.info = info;
  state.history = new PageHistory(info.pages, (pages, degrees) => rotatePages(info.document, pages, degrees));
  state.selection.clear();
  thumbnails.reset(state.rendererAvailable);
  resetViewer();
  ui.docName.textContent = info.name;
  void setWindowTitle(`${info.name} — 4YouPDF`);
  ui.empty.hidden = true;
  ui.grid.hidden = false;
  renderGrid();
  setStatus(
    `${info.name} — ${info.pages.length} page${info.pages.length > 1 ? "s" : ""}, PDF ${info.version}, ${formatSize(info.size)}`,
  );
}

async function chooseAndOpen(): Promise<void> {
  const path = await pickOpenFile();
  if (path !== null) {
    await requestOpen(path);
  }
}

// ---------------------------------------------------------------------------
// Leaving: closing the window, or opening another file, while the document
// carries unsaved changes (history.ts, `unsaved`)
// ---------------------------------------------------------------------------

/// Open `path` as the user asked (Ctrl+O, the button, a dropped file, a
/// password typed), unless that would lose unsaved changes: then the
/// question is asked in place, and its answer decides (`answered`).
async function requestOpen(path: string, password?: string): Promise<void> {
  if (mayLeave({ kind: "open", path })) {
    await open(path, password);
  }
}

/// Whether `leaving` may go ahead now: yes when nothing would be lost, or
/// when a password is being typed for the file to open, an opening the
/// user already chose. Otherwise the question is asked in place, in the
/// place of the one asked before, and nothing else happens: the window
/// stays open, the document on screen.
function mayLeave(leaving: Leaving): boolean {
  const info = state.info;
  const history = state.history;
  if (info === null || history === null || !history.unsaved) {
    return true;
  }
  if (leaving.kind === "open" && notices.asksPasswordFor(leaving.path)) {
    return true;
  }
  notices.ask(info.name, leaving);
  renderNotices();
  return false;
}

/// The question asked before `leaving` was answered by `action`. Saving
/// goes on with `leaving` once the file is written; cancelled or failed,
/// it leaves the document where it is, to be asked again.
async function answered(leaving: Leaving, action: "save" | "discard" | "cancel"): Promise<void> {
  notices.answered();
  renderNotices();
  if (action === "cancel") {
    return;
  }
  if (action === "save" && !(await save())) {
    return;
  }
  if (leaving.kind === "close") {
    await closeWindow();
  } else {
    await open(leaving.path);
  }
}

/// The Rust side held the window open because the document was reported
/// modified: ask, or let it close when nothing would be lost after all.
function closeRequested(): void {
  if (mayLeave({ kind: "close" })) {
    void closeWindow();
  }
}

// ---------------------------------------------------------------------------
// The grid
// ---------------------------------------------------------------------------

function renderGrid(): void {
  const history = state.history;
  if (history === null) {
    return;
  }
  const tiles: HTMLElement[] = [];
  history.order.forEach((page, position) => {
    const ratio = pageRatio(history.pages[page]);

    const tile = document.createElement("div");
    tile.className = "tile";
    tile.setAttribute("role", "listitem");
    tile.tabIndex = 0;
    tile.dataset["page"] = String(page);
    tile.dataset["position"] = String(position);
    tile.setAttribute(
      "aria-label",
      history.ofFile(page) ? `Page ${position + 1} (page ${page + 1} du fichier)` : `Page ${position + 1} (ajoutée par une fusion)`,
    );
    if (state.selection.has(position)) {
      tile.classList.add("selected");
    }
    if (history.isTurning(page)) {
      tile.classList.add("turning");
    }

    const frame = document.createElement("div");
    frame.className = "page";
    frame.style.aspectRatio = `1 / ${ratio}`;
    const placeholder = document.createElement("span");
    placeholder.className = "placeholder";
    placeholder.textContent = state.rendererAvailable ? "…" : "aperçu indisponible";
    const img = document.createElement("img");
    img.className = "thumb";
    img.alt = "";
    img.hidden = true;
    img.draggable = false;
    frame.append(placeholder, img);

    const label = document.createElement("div");
    label.className = "label";
    label.textContent = pageLabel(history, page, position);

    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "remove";
    remove.title = "Supprimer cette page";
    remove.setAttribute("aria-label", `Supprimer la page ${position + 1}`);
    remove.textContent = "×";
    remove.addEventListener("click", (event) => {
      event.stopPropagation();
      deletePositions([position]);
    });

    tile.append(frame, label, remove);
    tiles.push(tile);
  });
  ui.grid.replaceChildren(...tiles);
  for (const tile of tiles) {
    thumbnails.watch(tile);
  }
  markShown();
  refreshButtons();
}

/// The label of a tile: its position, then, when that is not the number
/// of the page in the file, where the page comes from: another place in
/// the file, or a merge, whose pages have no number in the file.
function pageLabel(history: PageHistory, page: number, position: number): string {
  if (!history.ofFile(page)) {
    return `${position + 1} (ajoutée)`;
  }
  return page === position ? `${position + 1}` : `${position + 1} (était ${page + 1})`;
}

function refreshButtons(): void {
  const history = state.history;
  const hasDoc = history !== null;
  // Pages are edited on the grid; the page view only turns the page it
  // shows, with its own buttons.
  const editable = hasDoc && !viewer.isOpen;
  const selected = editable && state.selection.size > 0;
  ui.undo.disabled = !(editable && history.canUndo);
  ui.redo.disabled = !(editable && history.canRedo);
  // Rotations wait for one another; deleting is refused until they are
  // done (history.ts).
  ui.delete.disabled = !(selected && !history.busy);
  ui.rotateLeft.disabled = !selected;
  ui.rotateRight.disabled = !selected;
  // A merge waits its turn among the rotations (history.ts).
  ui.merge.disabled = !editable;
  ui.save.disabled = !hasDoc;
  const unsaved = hasDoc && history.unsaved;
  ui.docName.classList.toggle("modified", unsaved);
  reportUnsaved(unsaved);
}

/// What the Rust side was last told about unsaved changes.
let reported = false;

/// Keep the Rust side told whether closing would lose work, whenever that
/// changes: it holds the window open on that word alone (see « Leaving »).
function reportUnsaved(unsaved: boolean): void {
  if (unsaved === reported) {
    return;
  }
  reported = unsaved;
  void documentModified(unsaved);
}

/// Build the grid again with the keyboard focus on the same position, for
/// a rotation, which moves no page.
function renderGridKeepingFocus(): void {
  const focused = positionOf(document.activeElement);
  renderGrid();
  if (focused !== null) {
    ui.grid.querySelector<HTMLElement>(`.tile[data-position="${focused}"]`)?.focus({ preventScroll: true });
  }
}

/// Dim the pages being turned, in the grid and in the page view.
function showTurning(): void {
  const history = state.history;
  for (const tile of ui.grid.querySelectorAll<HTMLElement>(".tile")) {
    const page = Number(tile.dataset["page"] ?? "-1");
    tile.classList.toggle("turning", history?.isTurning(page) === true);
  }
  viewer.refreshTurning();
}

function positionOf(target: EventTarget | null): number | null {
  if (!(target instanceof Element)) {
    return null;
  }
  const tile = target.closest<HTMLElement>(".tile");
  if (tile === null) {
    return null;
  }
  return Number(tile.dataset["position"] ?? "-1");
}

function select(position: number, extend: boolean, range: boolean): void {
  if (range && state.selection.size > 0) {
    const anchor = Math.min(...state.selection);
    const [from, to] = anchor < position ? [anchor, position] : [position, anchor];
    for (let p = from; p <= to; p += 1) {
      state.selection.add(p);
    }
  } else if (extend) {
    if (state.selection.has(position)) {
      state.selection.delete(position);
    } else {
      state.selection.add(position);
    }
  } else {
    state.selection.clear();
    state.selection.add(position);
  }
  for (const tile of ui.grid.querySelectorAll<HTMLElement>(".tile")) {
    const p = Number(tile.dataset["position"] ?? "-1");
    tile.classList.toggle("selected", state.selection.has(p));
  }
  refreshButtons();
}

// ---------------------------------------------------------------------------
// Page view: one page beside the grid, reduced to a panel of thumbnails, or
// over it; a state of the window (ADR 0004)
// ---------------------------------------------------------------------------

/// Position the view was opened at: on the way back, the page seen last
/// becomes the selection only if it is another one.
let viewerOpenedAt = -1;

function openViewer(position: number): void {
  const history = state.history;
  if (history === null || viewer.isOpen || position < 0 || position >= history.order.length) {
    return;
  }
  hideMenu();
  viewerOpenedAt = position;
  // Laid out first: the view fits the page in the room the panel leaves.
  // The thumbnails wait for the page on screen (`thumbnailSlots`).
  layOut(true);
  viewer.open(history.pages, history.order, position, state.rendererAvailable);
  refreshButtons();
}

/// Back to the grid, on the page seen last: it gets the focus, and becomes
/// the selection unless it is the page the view was opened on (the
/// selection then stays as it was).
function viewerClosed(position: number): void {
  layOut(false);
  markShown();
  thumbnails.wake();
  if (position !== viewerOpenedAt) {
    select(position, false, false);
  }
  const tile = ui.grid.querySelector<HTMLElement>(`.tile[data-position="${position}"]`);
  tile?.focus({ preventScroll: true });
  tile?.scrollIntoView({ block: "nearest" });
  refreshButtons();
}

/// Another document: leave the view without going back to a page. Called
/// after `thumbnails.reset`, so that waking finds an empty queue.
function resetViewer(): void {
  viewer.reset();
  layOut(false);
  thumbnails.wake();
}

/// Lay out the workspace for a page view `open` or not: the grid alone; the
/// view over the grid, left inert underneath; or, with the panel, the view
/// beside the grid, then one column of thumbnails where a click shows a
/// page and nothing is edited.
function layOut(open: boolean): void {
  ui.workspace.classList.toggle("with-panel", open && state.panel);
  ui.gridRoot.inert = open && !state.panel;
  ui.viewerPanel.setAttribute("aria-pressed", String(state.panel));
}

/// Show or hide the panel beside the page view (F4, button `Vignettes`).
function togglePanel(): void {
  if (!viewer.isOpen) {
    return;
  }
  state.panel = !state.panel;
  // A tile that has the keyboard gives it back to the view before the grid
  // goes inert.
  if (!state.panel && ui.gridRoot.contains(document.activeElement)) {
    viewer.focus();
  }
  layOut(true);
  markShown();
  thumbnails.wake();
}

/// Mark the tile of the page the view shows, and keep it in sight while the
/// grid is the panel beside the view.
function markShown(): void {
  for (const tile of ui.grid.querySelectorAll<HTMLElement>(".tile.current")) {
    tile.classList.remove("current");
    tile.removeAttribute("aria-current");
  }
  if (!viewer.isOpen) {
    return;
  }
  const tile = ui.grid.querySelector<HTMLElement>(`.tile[data-position="${viewer.position}"]`);
  if (tile === null) {
    return;
  }
  tile.classList.add("current");
  tile.setAttribute("aria-current", "page");
  if (state.panel) {
    tile.scrollIntoView({ block: "nearest" });
  }
}

/// The page Enter opens: the focused tile, else the first selected page.
function enterTarget(target: EventTarget | null): number | null {
  if (target instanceof HTMLElement && target.classList.contains("tile")) {
    return positionOf(target);
  }
  if (target === document.body && state.selection.size > 0) {
    return Math.min(...state.selection);
  }
  return null;
}

// The grid captures the pointer while a button is down (to drag), which
// can make the grid itself the target of a double-click: the tile is the
// one under the pointer.
ui.grid.addEventListener("dblclick", (event) => {
  const target = document.elementFromPoint(event.clientX, event.clientY);
  if (target === null || target.closest("button") !== null) {
    return;
  }
  const position = positionOf(target);
  if (position !== null) {
    openViewer(position);
  }
});

// Beside the page view, the grid is a panel: a click on a tile shows its
// page. Nothing is selected, dragged or deleted there.
ui.grid.addEventListener("click", (event) => {
  if (!viewer.isOpen) {
    return;
  }
  const position = positionOf(event.target);
  if (position !== null) {
    viewer.go(position);
  }
});

// ---------------------------------------------------------------------------
// Editing
// ---------------------------------------------------------------------------

function deletePositions(positions: number[]): void {
  const history = state.history;
  // Not while the page view is open, whose order must not change; beside
  // it, the panel edits nothing.
  if (history === null || viewer.isOpen || positions.length === 0 || refusedWhileTurning()) {
    return;
  }
  if (positions.length >= history.order.length) {
    notice("warn", "Un document doit garder au moins une page.");
    return;
  }
  if (history.remove(positions)) {
    state.selection.clear();
    renderGrid();
    setStatus(
      `${positions.length} page${positions.length > 1 ? "s supprimées" : " supprimée"} (Ctrl+Z pour annuler).`,
    );
  }
}

function movePositions(positions: number[], target: number): void {
  const history = state.history;
  // Not while the page view is open (see `deletePositions`).
  if (history === null || viewer.isOpen || positions.length === 0 || refusedWhileTurning()) {
    return;
  }
  const sorted = [...positions].sort((a, b) => a - b);
  // Target is a position in the order without the moved pages.
  const before = sorted.filter((p) => p < target).length;
  if (history.move(sorted, target - before)) {
    state.selection = new Set(sorted.map((_, i) => target - before + i));
    renderGrid();
    setStatus("Pages déplacées (Ctrl+Z pour annuler).");
  }
}

/// Whether a rotation is running, which moving, deleting, undoing and
/// redoing wait for (`history.ts`); says so in the status bar.
function refusedWhileTurning(): boolean {
  if (state.history?.busy !== true) {
    return false;
  }
  setStatus("Rotation en cours : réessayez dans un instant.");
  return true;
}

/// Turn the source pages `pages` by `degrees`, 90 clockwise or -90
/// counter-clockwise. The Rust side applies it to the document through
/// `ops::rotate`; the pages are then drawn again from the result.
/// `fromViewer` when the page view asked for it.
async function turnPages(pages: readonly number[], degrees: number, fromViewer: boolean): Promise<void> {
  const history = state.history;
  if (history === null || pages.length === 0) {
    return;
  }
  hideMenu();
  const running = history.rotate(pages, degrees);
  setStatus(`Rotation ${pages.length > 1 ? `de ${pages.length} pages` : "de la page"}…`);
  showTurning();
  refreshButtons();
  const outcome = await running;
  if (state.history !== history) {
    return;
  }
  if (outcome.kind === "rotation") {
    const turned = pages.length > 1 ? `${pages.length} pages pivotées` : "Page pivotée";
    const how = fromViewer ? "Ctrl+Z dans la grille" : "Ctrl+Z";
    setStatus(`${turned} ${degrees > 0 ? "à droite" : "à gauche"} (${how} pour annuler).`);
  } else if (outcome.kind === "failed") {
    notice("error", `Rotation impossible : ${outcome.message}`);
    setStatus("Rotation impossible.");
  }
  edited(outcome);
}

/// Turn the pages selected in the grid.
function turnSelection(degrees: number): void {
  const history = state.history;
  if (history === null || viewer.isOpen) {
    return;
  }
  const pages = [...state.selection]
    .sort((a, b) => a - b)
    .map((position) => history.order[position])
    .filter((page): page is number => page !== undefined);
  void turnPages(pages, degrees, false);
}

/// Pick the files to merge, several at once, then merge them: at the end
/// of the grid, or from position `at` (« Fusionner ici… » on a tile).
async function chooseAndMerge(at?: number): Promise<void> {
  if (state.history === null || viewer.isOpen) {
    return;
  }
  const paths = await pickMergeFiles();
  if (paths !== null && paths.length > 0) {
    await mergeFiles(paths, at);
  }
}

/// Append every page of the files at `paths`, in that order, to the
/// document: the Rust side rewrites it through `ops::merge`, and the new
/// pages go into the grid at the end, or from position `at`, selected, as
/// one edit that Ctrl+Z undoes. A file that does not open is skipped and
/// said so, and the others merge without it (merge.ts). Not while the page
/// view is open, whose order must not change.
async function mergeFiles(paths: readonly string[], at?: number): Promise<void> {
  const info = state.info;
  const history = state.history;
  if (info === null || history === null || viewer.isOpen || paths.length === 0) {
    return;
  }
  hideMenu();
  // Filled by the merger, which runs once the rotations before are done.
  const report: { sources: readonly SourceReport[] } = { sources: [] };
  const running = history.merge(async () => {
    const answer = await mergeDocuments(info.document, [...paths]);
    report.sources = answer.sources;
    return answer.pages;
  }, at);
  setStatus(`Fusion ${paths.length > 1 ? `de ${paths.length} fichiers` : "d'un fichier"}…`);
  refreshButtons();
  const outcome = await running;
  if (state.history !== history) {
    return;
  }
  for (const [kind, text] of mergeNotices(report.sources)) {
    notices.event(kind, text);
  }
  renderNotices();
  if (outcome.kind === "failed") {
    notice("error", `Fusion impossible : ${outcome.message}`);
    setStatus("Fusion impossible.");
  } else {
    setStatus(mergeStatus(report.sources));
  }
  edited(outcome);
}

async function undo(): Promise<void> {
  const history = state.history;
  if (history === null || refusedWhileTurning()) {
    return;
  }
  const running = history.undo();
  if (history.busy) {
    setStatus("Annulation de la rotation…");
    showTurning();
    refreshButtons();
  }
  const outcome = await running;
  if (state.history !== history) {
    return;
  }
  if (outcome.kind === "failed") {
    notice("error", `Annulation impossible : ${outcome.message}`);
    setStatus("Annulation impossible.");
  } else if (outcome.kind !== "none") {
    setStatus("Annulé.");
  }
  edited(outcome);
}

async function redo(): Promise<void> {
  const history = state.history;
  if (history === null || refusedWhileTurning()) {
    return;
  }
  const running = history.redo();
  if (history.busy) {
    setStatus("Rétablissement de la rotation…");
    showTurning();
    refreshButtons();
  }
  const outcome = await running;
  if (state.history !== history) {
    return;
  }
  if (outcome.kind === "failed") {
    notice("error", `Rétablissement impossible : ${outcome.message}`);
    setStatus("Rétablissement impossible.");
  } else if (outcome.kind !== "none") {
    setStatus("Refait.");
  }
  edited(outcome);
}

/// Bring the window up to date after an edit that ended with `outcome`.
/// Positions change only with the order: a rotation keeps the selection
/// and the keyboard focus where they are; the pages a merge added become
/// the selection, the first of them brought into sight.
function edited(outcome: Outcome): void {
  const history = state.history;
  if (history === null) {
    return;
  }
  if (outcome.kind === "order") {
    state.selection.clear();
    renderGrid();
  } else if (outcome.kind === "merged") {
    state.selection = new Set(outcome.added.map((_, i) => outcome.at + i));
    renderGrid();
    ui.grid.querySelector<HTMLElement>(`.tile[data-position="${outcome.at}"]`)?.scrollIntoView({ block: "nearest" });
  } else if (outcome.kind === "rotation") {
    thumbnails.invalidate(outcome.pages);
    viewer.pagesChanged(history.pages, outcome.pages);
    renderGridKeepingFocus();
  }
  showTurning();
  refreshButtons();
}

/// Save as: `true` once the file is written; `false` when the picker was
/// cancelled or writing failed. The document in memory stays the file
/// opened, under its name, and is intact from then on: what it would save
/// is on disk (`history.saved`).
async function save(): Promise<boolean> {
  const info = state.info;
  const history = state.history;
  if (info === null || history === null) {
    return false;
  }
  const stem = info.name.replace(/\.pdf$/i, "");
  const path = await pickSaveFile(`${stem}-modifié.pdf`);
  if (path === null) {
    return false;
  }
  if (history.busy) {
    // The file saved must hold the rotations asked for.
    setStatus("Enregistrement à la fin de la rotation en cours…");
    await history.idle();
    if (state.history !== history) {
      return false;
    }
  }
  // What is written: an edit made while the file is written comes after.
  const point = history.toSave;
  setStatus(`Enregistrement de ${path}…`);
  try {
    const report = await saveDocument(path, [...point.order]);
    setStatus(
      `Enregistré : ${report.path} (${report.pages} page${report.pages > 1 ? "s" : ""}, ${formatSize(report.size)}).`,
    );
    if (info.encryption !== null) {
      notice("info", "Le fichier enregistré est en clair : la protection du fichier d'origine n'a pas été reportée.");
    }
  } catch (e: unknown) {
    const error = asAppError(e);
    const message = error.kind === "other" ? error.message : "mot de passe";
    notice("error", `Enregistrement impossible : ${message}`);
    setStatus("Enregistrement impossible.");
    return false;
  }
  if (state.history === history) {
    history.saved(point);
    refreshButtons();
  }
  return true;
}

// ---------------------------------------------------------------------------
// Drag to reorder (pointer events: the native file drop stays enabled,
// which rules out HTML5 drag and drop inside the window)
// ---------------------------------------------------------------------------

interface Drag {
  positions: number[];
  ghost: HTMLElement;
  target: number;
  started: boolean;
  startX: number;
  startY: number;
}

let drag: Drag | null = null;

function dropTargetAt(x: number, y: number): number {
  const tiles = [...ui.grid.querySelectorAll<HTMLElement>(".tile")];
  let target = tiles.length;
  for (const tile of tiles) {
    const rect = tile.getBoundingClientRect();
    const position = Number(tile.dataset["position"] ?? "0");
    if (y < rect.top) {
      target = Math.min(target, position);
      continue;
    }
    if (y <= rect.bottom) {
      if (x < rect.left + rect.width / 2) {
        return position;
      }
      target = position + 1;
    }
  }
  return target;
}

function showDropMarker(target: number): void {
  const tiles = [...ui.grid.querySelectorAll<HTMLElement>(".tile")];
  const rootRect = ui.gridRoot.getBoundingClientRect();
  const marker = ui.dropMarker;
  let rect: DOMRect;
  let left: number;
  if (target < tiles.length) {
    const tile = tiles[target];
    if (tile === undefined) {
      return;
    }
    rect = tile.getBoundingClientRect();
    left = rect.left - 10;
  } else {
    const last = tiles[tiles.length - 1];
    if (last === undefined) {
      return;
    }
    rect = last.getBoundingClientRect();
    left = rect.right + 6;
  }
  marker.hidden = false;
  marker.style.left = `${left - rootRect.left + ui.gridRoot.scrollLeft}px`;
  marker.style.top = `${rect.top - rootRect.top + ui.gridRoot.scrollTop}px`;
  marker.style.height = `${rect.height}px`;
}

ui.grid.addEventListener("pointerdown", (event) => {
  // Beside the page view, a tile of the panel is only clicked (page view).
  if (event.button !== 0 || viewer.isOpen) {
    return;
  }
  if (event.target instanceof Element && event.target.closest("button") !== null) {
    return;
  }
  const position = positionOf(event.target);
  if (position === null) {
    return;
  }
  if (!state.selection.has(position) || event.ctrlKey || event.shiftKey) {
    select(position, event.ctrlKey, event.shiftKey);
  }
  const tile = (event.target as Element).closest<HTMLElement>(".tile");
  if (tile === null) {
    return;
  }
  tile.focus();
  const ghost = tile.cloneNode(true) as HTMLElement;
  ghost.className = "tile drag-ghost";
  ghost.style.width = `${tile.getBoundingClientRect().width}px`;
  drag = {
    positions: [...state.selection].sort((a, b) => a - b),
    ghost,
    target: position,
    started: false,
    startX: event.clientX,
    startY: event.clientY,
  };
  ui.grid.setPointerCapture(event.pointerId);
});

ui.grid.addEventListener("pointermove", (event) => {
  if (drag === null) {
    return;
  }
  if (!drag.started) {
    if (Math.hypot(event.clientX - drag.startX, event.clientY - drag.startY) < 6) {
      return;
    }
    drag.started = true;
    document.body.append(drag.ghost);
    for (const tile of ui.grid.querySelectorAll<HTMLElement>(".tile.selected")) {
      tile.classList.add("dragging");
    }
  }
  drag.ghost.style.left = `${event.clientX}px`;
  drag.ghost.style.top = `${event.clientY}px`;
  drag.target = dropTargetAt(event.clientX, event.clientY);
  showDropMarker(drag.target);
  // Scroll when dragging near the edges.
  const rect = ui.gridRoot.getBoundingClientRect();
  if (event.clientY > rect.bottom - 40) {
    ui.gridRoot.scrollTop += 12;
  } else if (event.clientY < rect.top + 40) {
    ui.gridRoot.scrollTop -= 12;
  }
});

function endDrag(apply: boolean): void {
  if (drag === null) {
    return;
  }
  const current = drag;
  drag = null;
  ui.dropMarker.hidden = true;
  current.ghost.remove();
  for (const tile of ui.grid.querySelectorAll<HTMLElement>(".tile.dragging")) {
    tile.classList.remove("dragging");
  }
  if (apply && current.started) {
    movePositions(current.positions, current.target);
  }
}

ui.grid.addEventListener("pointerup", () => endDrag(true));
ui.grid.addEventListener("pointercancel", () => endDrag(false));

// ---------------------------------------------------------------------------
// Context menu on a tile (in the page, never a native menu)
// ---------------------------------------------------------------------------

let menuPositions: number[] = [];

function hideMenu(): void {
  ui.contextMenu.hidden = true;
  menuPositions = [];
}

ui.grid.addEventListener("contextmenu", (event) => {
  const position = positionOf(event.target);
  if (position === null) {
    return;
  }
  event.preventDefault();
  if (viewer.isOpen) {
    // Beside the page view, the panel edits nothing: no menu.
    return;
  }
  if (!state.selection.has(position)) {
    select(position, false, false);
  }
  menuPositions = [...state.selection].sort((a, b) => a - b);
  const menu = ui.contextMenu;
  menu.hidden = false;
  const width = menu.offsetWidth;
  const height = menu.offsetHeight;
  menu.style.left = `${Math.min(event.clientX, window.innerWidth - width - 8)}px`;
  menu.style.top = `${Math.min(event.clientY, window.innerHeight - height - 8)}px`;
  menu.querySelector<HTMLButtonElement>("button")?.focus();
});

ui.contextMenu.addEventListener("click", (event) => {
  const button = (event.target as Element).closest<HTMLButtonElement>("button");
  if (button === null) {
    return;
  }
  const positions = menuPositions;
  hideMenu();
  const count = state.history?.order.length ?? 0;
  switch (button.dataset["action"]) {
    case "delete":
      deletePositions(positions);
      break;
    case "move-first":
      movePositions(positions, 0);
      break;
    case "move-last":
      movePositions(positions, count);
      break;
    case "merge-here":
      // In front of the first selected page: the right click selected the
      // tile under the pointer.
      void chooseAndMerge(positions[0]);
      break;
    default:
      break;
  }
});

document.addEventListener("pointerdown", (event) => {
  if (!ui.contextMenu.hidden && !(event.target instanceof Element && event.target.closest("#context-menu"))) {
    hideMenu();
  }
});

// ---------------------------------------------------------------------------
// Keyboard
// ---------------------------------------------------------------------------

// The shortcuts of the browser, such as F5, which would reload the page of
// the interface and lose the open document with its history (shortcuts.ts):
// their default is prevented first, on the way down to the target, and they
// go on to the listeners below, but for the keys that would open DevTools,
// which stop here.
window.addEventListener(
  "keydown",
  (event) => {
    if (browserShortcut(event) !== undefined) {
      event.preventDefault();
    }
    if (stopsHere(event)) {
      event.stopPropagation();
    }
  },
  true,
);

document.addEventListener("keydown", (event) => {
  const inField = event.target instanceof HTMLInputElement;
  if (!inField && viewer.handleKey(event)) {
    return;
  }
  if (event.key === "Escape") {
    hideMenu();
    if (drag !== null) {
      endDrag(false);
    }
    return;
  }
  if (inField) {
    return;
  }
  const ctrl = event.ctrlKey || event.metaKey;
  if (ctrl && event.key.toLowerCase() === "o") {
    event.preventDefault();
    void chooseAndOpen();
  } else if (ctrl && event.key.toLowerCase() === "s") {
    event.preventDefault();
    void save();
  } else if (viewer.isOpen) {
    // The keys below edit the grid, which the page view covers or reduces
    // to a panel. There, F4 shows or hides the panel, and Enter on a tile
    // shows its page.
    if (event.key === "F4" && !ctrl && !event.altKey && !event.shiftKey) {
      event.preventDefault();
      if (!event.repeat) {
        togglePanel();
      }
    } else if (event.key === "Enter" && !ctrl && !event.altKey && event.target instanceof HTMLElement) {
      const position = event.target.classList.contains("tile") ? positionOf(event.target) : null;
      if (position !== null) {
        event.preventDefault();
        viewer.go(position);
      }
    }
  } else if (ctrl && event.key.toLowerCase() === "z" && !event.shiftKey) {
    event.preventDefault();
    void undo();
  } else if ((ctrl && event.key.toLowerCase() === "y") || (ctrl && event.shiftKey && event.key.toLowerCase() === "z")) {
    event.preventDefault();
    void redo();
  } else if (ctrl && event.key.toLowerCase() === "m") {
    event.preventDefault();
    void chooseAndMerge();
  } else if (ctrl && event.key.toLowerCase() === "a" && state.history !== null) {
    event.preventDefault();
    state.selection = new Set(state.history.order.map((_, i) => i));
    renderGrid();
  } else if (event.key.toLowerCase() === "r" && !ctrl && !event.altKey) {
    // R turns the selected pages clockwise, Shift+R counter-clockwise.
    if (state.selection.size > 0) {
      event.preventDefault();
      if (!event.repeat) {
        turnSelection(event.shiftKey ? -90 : 90);
      }
    }
  } else if (event.key === "Delete" || event.key === "Backspace") {
    if (state.selection.size > 0) {
      event.preventDefault();
      deletePositions([...state.selection]);
    }
  } else if (event.key === "Enter" && !ctrl && !event.altKey) {
    const position = enterTarget(event.target);
    if (position !== null) {
      event.preventDefault();
      openViewer(position);
    }
  } else if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
    const count = state.history?.order.length ?? 0;
    if (count === 0) {
      return;
    }
    const current = state.selection.size > 0 ? Math.max(...state.selection) : -1;
    const next = event.key === "ArrowRight" ? Math.min(current + 1, count - 1) : Math.max(current - 1, 0);
    event.preventDefault();
    select(next, false, event.shiftKey);
    ui.grid.querySelector<HTMLElement>(`.tile[data-position="${next}"]`)?.focus();
  }
});

// ---------------------------------------------------------------------------
// Buttons and file drop
// ---------------------------------------------------------------------------

ui.open.addEventListener("click", () => void chooseAndOpen());
ui.openEmpty.addEventListener("click", () => void chooseAndOpen());
ui.merge.addEventListener("click", () => void chooseAndMerge());
ui.undo.addEventListener("click", () => void undo());
ui.redo.addEventListener("click", () => void redo());
ui.rotateLeft.addEventListener("click", () => turnSelection(-90));
ui.rotateRight.addEventListener("click", () => turnSelection(90));
ui.delete.addEventListener("click", () => deletePositions([...state.selection]));
ui.save.addEventListener("click", () => void save());
ui.viewerPanel.addEventListener("click", togglePanel);

async function start(): Promise<void> {
  if (!hasTauri()) {
    notice("error", "Cette page doit être ouverte par l'application 4YouPDF, pas par un navigateur.");
    return;
  }
  try {
    const status = await rendererStatus();
    state.rendererAvailable = status.available;
    ui.rendererStatus.textContent = status.available
      ? "Aperçus : PDFium"
      : `Aperçus indisponibles — ${status.detail}`;
    ui.rendererStatus.title = status.detail;
  } catch (e: unknown) {
    ui.rendererStatus.textContent = `Aperçus indisponibles — ${String(e)}`;
  }
  await onFileDrop((paths) => {
    ui.dropOverlay.hidden = true;
    const pdf = paths.find((p) => p.toLowerCase().endsWith(".pdf")) ?? paths[0];
    if (pdf !== undefined) {
      void requestOpen(pdf);
    }
  });
  // The Rust side holds the window open while the document is reported
  // modified, and asks here (« Leaving »).
  await onCloseRequested(closeRequested);
  // The overlay follows the native drag events, which the webview also
  // reports through the same listener with other types.
  const webview = window.__TAURI__?.webview.getCurrentWebview();
  if (webview !== undefined) {
    await webview.onDragDropEvent((event) => {
      if (event.payload.type === "enter" || event.payload.type === "over") {
        ui.dropOverlay.hidden = false;
      } else if (event.payload.type === "leave") {
        ui.dropOverlay.hidden = true;
      }
    });
  }
  window.addEventListener("beforeunload", () => void closeDocument());
  // Read once, here, before any edit: a file given to a second launch of
  // the application opens in that second window, and touches nothing here.
  const first = await initialFile();
  if (first !== null) {
    await open(first);
  }
}

void start();
