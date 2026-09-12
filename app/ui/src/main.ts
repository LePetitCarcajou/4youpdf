// The window: open a PDF (dialog, drop, Ctrl+O), show its pages as
// tiles, reorder them by dragging, delete them, undo and redo, save
// through `fyp_core::ops` on the Rust side. One window, no modal dialog
// but the system file pickers; every message appears in place (ADR 0004).

import {
  asAppError,
  closeDocument,
  hasTauri,
  initialFile,
  onFileDrop,
  openDocument,
  pickOpenFile,
  pickSaveFile,
  rendererStatus,
  saveDocument,
  setWindowTitle,
  type DocumentInfo,
} from "./api.js";
import { OrderHistory } from "./history.js";
import { ThumbnailLoader } from "./thumbnails.js";

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
  docName: element<HTMLSpanElement>("doc-name"),
  undo: element<HTMLButtonElement>("undo"),
  redo: element<HTMLButtonElement>("redo"),
  delete: element<HTMLButtonElement>("delete"),
  save: element<HTMLButtonElement>("save"),
  notices: element<HTMLElement>("notices"),
  gridRoot: element<HTMLElement>("grid-root"),
  grid: element<HTMLElement>("grid"),
  empty: element<HTMLElement>("empty"),
  dropMarker: element<HTMLElement>("drop-marker"),
  statusText: element<HTMLSpanElement>("status-text"),
  rendererStatus: element<HTMLSpanElement>("renderer-status"),
  contextMenu: element<HTMLElement>("context-menu"),
  dropOverlay: element<HTMLElement>("drop-overlay"),
};

interface State {
  info: DocumentInfo | null;
  history: OrderHistory | null;
  selection: Set<number>;
  rendererAvailable: boolean;
}

const state: State = {
  info: null,
  history: null,
  selection: new Set(),
  rendererAvailable: false,
};

const thumbnails = new ThumbnailLoader(ui.gridRoot, THUMB_WIDTH, false);

// ---------------------------------------------------------------------------
// Notices and status
// ---------------------------------------------------------------------------

type NoticeKind = "info" | "warn" | "error";

function notice(kind: NoticeKind, text: string, extra?: HTMLElement): HTMLElement {
  const box = document.createElement("div");
  box.className = `notice ${kind}`;
  const span = document.createElement("span");
  span.textContent = text;
  box.append(span);
  if (extra !== undefined) {
    box.append(extra);
  }
  const close = document.createElement("button");
  close.type = "button";
  close.className = "close";
  close.textContent = "×";
  close.title = "Fermer";
  close.addEventListener("click", () => box.remove());
  box.append(close);
  ui.notices.append(box);
  return box;
}

function clearNotices(): void {
  ui.notices.replaceChildren();
}

function setStatus(text: string): void {
  ui.statusText.textContent = text;
}

/// Last component of a path, Windows or POSIX separators.
function fileName(path: string): string {
  return path.split(/[/\\]/).pop() ?? path;
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

async function open(path: string, password?: string): Promise<void> {
  clearNotices();
  setStatus(`Ouverture de ${path}…`);
  try {
    const info = await openDocument(path, password);
    loaded(info);
  } catch (e: unknown) {
    const error = asAppError(e);
    if (error.kind === "wrong_password") {
      await awaitingPassword(path);
      askPassword(path, password !== undefined);
      setStatus("Document chiffré : mot de passe requis.");
      return;
    }
    notice("error", `Impossible d'ouvrir ${path} : ${error.message}`);
    setStatus("Ouverture impossible.");
  }
}

/// The file at `path` needs a password before it can be shown. The
/// previous document is closed on both sides: the toolbar and the window
/// title name the file being opened, and no thumbnail of another document
/// stays on screen while the password is asked for.
async function awaitingPassword(path: string): Promise<void> {
  state.info = null;
  state.history = null;
  state.selection.clear();
  thumbnails.reset(state.rendererAvailable);
  ui.grid.replaceChildren();
  ui.grid.hidden = true;
  ui.empty.hidden = false;
  const name = fileName(path);
  ui.docName.textContent = name;
  void setWindowTitle(`${name} — 4YouPDF`);
  refreshButtons();
  try {
    await closeDocument();
  } catch {
    // Nothing to close, or the Rust side already forgot it: same outcome.
  }
}

/// A password field in the notices area, never a modal.
function askPassword(path: string, wrong: boolean): void {
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
    void open(path, input.value);
  });
  notice(
    "warn",
    wrong
      ? "Ce mot de passe n'ouvre pas le fichier. Essayez le mot de passe utilisateur ou propriétaire."
      : "Ce fichier est protégé par un mot de passe.",
    form,
  );
  input.focus();
}

function loaded(info: DocumentInfo): void {
  state.info = info;
  state.history = new OrderHistory(info.pages.length);
  state.selection.clear();
  thumbnails.reset(state.rendererAvailable);
  ui.docName.textContent = info.name;
  void setWindowTitle(`${info.name} — 4YouPDF`);
  ui.empty.hidden = true;
  ui.grid.hidden = false;
  if (info.reconstructed !== null) {
    notice(
      "warn",
      `Fichier endommagé, table des objets reconstruite par analyse du fichier (${info.reconstructed}). ` +
        "L'enregistrement produira un fichier sain.",
    );
  }
  if (info.relocated_startxref !== null) {
    notice(
      "info",
      `Le fichier annonce sa table à un mauvais endroit ; elle a été retrouvée à l'offset ${info.relocated_startxref}.`,
    );
  }
  if (info.encryption !== null) {
    notice(
      "warn",
      `Fichier chiffré (${info.encryption}). L'enregistrement produira un fichier EN CLAIR, sans protection.`,
    );
  }
  renderGrid();
  setStatus(
    `${info.name} — ${info.pages.length} page${info.pages.length > 1 ? "s" : ""}, PDF ${info.version}, ${formatSize(info.size)}`,
  );
}

async function chooseAndOpen(): Promise<void> {
  const path = await pickOpenFile();
  if (path !== null) {
    await open(path);
  }
}

// ---------------------------------------------------------------------------
// The grid
// ---------------------------------------------------------------------------

function renderGrid(): void {
  const info = state.info;
  const history = state.history;
  if (info === null || history === null) {
    return;
  }
  const tiles: HTMLElement[] = [];
  history.order.forEach((page, position) => {
    const size = info.pages[page] ?? { width: 612, height: 792, rotate: 0 };
    const landscape = size.rotate % 180 !== 0;
    const ratio = landscape ? size.width / size.height : size.height / size.width;

    const tile = document.createElement("div");
    tile.className = "tile";
    tile.setAttribute("role", "listitem");
    tile.tabIndex = 0;
    tile.dataset["page"] = String(page);
    tile.dataset["position"] = String(position);
    tile.setAttribute("aria-label", `Page ${position + 1} (page ${page + 1} du fichier)`);
    if (state.selection.has(position)) {
      tile.classList.add("selected");
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
    label.textContent = page === position ? `${position + 1}` : `${position + 1} (était ${page + 1})`;

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
  refreshButtons();
}

function refreshButtons(): void {
  const history = state.history;
  const hasDoc = history !== null;
  ui.undo.disabled = !(hasDoc && history.canUndo);
  ui.redo.disabled = !(hasDoc && history.canRedo);
  ui.delete.disabled = !(hasDoc && state.selection.size > 0);
  ui.save.disabled = !hasDoc;
  ui.docName.classList.toggle("modified", hasDoc && history.modified);
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
// Editing
// ---------------------------------------------------------------------------

function deletePositions(positions: number[]): void {
  const history = state.history;
  if (history === null || positions.length === 0) {
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
  if (history === null || positions.length === 0) {
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

function undo(): void {
  if (state.history?.undo() === true) {
    state.selection.clear();
    renderGrid();
    setStatus("Annulé.");
  }
}

function redo(): void {
  if (state.history?.redo() === true) {
    state.selection.clear();
    renderGrid();
    setStatus("Refait.");
  }
}

async function save(): Promise<void> {
  const info = state.info;
  const history = state.history;
  if (info === null || history === null) {
    return;
  }
  const stem = info.name.replace(/\.pdf$/i, "");
  const path = await pickSaveFile(`${stem}-modifié.pdf`);
  if (path === null) {
    return;
  }
  setStatus(`Enregistrement de ${path}…`);
  try {
    const report = await saveDocument(path, [...history.order]);
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
  }
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
  if (event.button !== 0) {
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

document.addEventListener("keydown", (event) => {
  const inField = event.target instanceof HTMLInputElement;
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
  } else if (ctrl && event.key.toLowerCase() === "z" && !event.shiftKey) {
    event.preventDefault();
    undo();
  } else if ((ctrl && event.key.toLowerCase() === "y") || (ctrl && event.shiftKey && event.key.toLowerCase() === "z")) {
    event.preventDefault();
    redo();
  } else if (ctrl && event.key.toLowerCase() === "a" && state.history !== null) {
    event.preventDefault();
    state.selection = new Set(state.history.order.map((_, i) => i));
    renderGrid();
  } else if (event.key === "Delete" || event.key === "Backspace") {
    if (state.selection.size > 0) {
      event.preventDefault();
      deletePositions([...state.selection]);
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
ui.undo.addEventListener("click", undo);
ui.redo.addEventListener("click", redo);
ui.delete.addEventListener("click", () => deletePositions([...state.selection]));
ui.save.addEventListener("click", () => void save());

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
      void open(pdf);
    }
  });
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
  const first = await initialFile();
  if (first !== null) {
    await open(first);
  }
}

void start();
