// The page view: one page as large as the window allows, drawn beside the
// grid or over it, the grid staying in place. It is a state of the window,
// not a dialog (ADR 0004): the toolbar, the notices and the status bar stay
// visible.
//
// The page can be enlarged, up to what the renderer draws (`zoom.ts`):
// Ctrl+wheel around the pointer, Ctrl+plus and Ctrl+minus around the
// centre, Ctrl+0 back to the fitted page, or the buttons of the bar, which
// also say the zoom. A page larger than the window is moved by dragging
// it, with the wheel, or with the arrow and page keys; the wheel and those
// keys turn the page from its edge, as in reading. The zoom and the place
// in the page are kept from one page to the next, to compare them; opening
// the view fits the page again.
//
// The renderer serves one request at a time, so the view keeps at most one
// request of its own in flight: the page on screen first, at its size on
// screen, then its two neighbours once it is shown, as fitted to the
// window, so that turning to them is immediate. A page skipped while
// turning quickly is never drawn. A zoom or a resize scales the image on
// screen at once, and asks for a sharper one only once the gesture has
// settled: a gesture of the wheel costs one drawing, not one per notch.
// Images of the pages near the current one are kept, the others dropped.
//
// The page on screen can be rotated (buttons, R and Shift+R). The view only
// asks for it: `main.ts` has the Rust side turn the page, then tells the
// view, which drops the images drawn before and draws the page again.
//
// `main.ts` can keep the grid beside the view, as a panel of thumbnails.
// The view tells it which page it shows, and when it has nothing left to
// draw: the thumbnails of the panel take their turn only then.
//
// The number in the caption is the position of the page in the order.
// Typed over, or typed anywhere in the view, it goes to another page
// (`pagenumber.ts`).

import { asAppError, renderPage, type PageInfo } from "./api.js";
import { pageCaption, pageNumberHelp, pageNumberRefusal, readPageNumber } from "./pagenumber.js";
import {
  arriveAt,
  atEdge,
  clampOffset,
  fit,
  inner,
  MAX_WIDTH,
  maxZoom,
  overflows,
  scaled,
  ZOOM_NOTCH,
  zoomAround,
  zoomIn,
  zoomKey,
  zoomLabel,
  zoomLevels,
  zoomOut,
  zoomTicks,
  type Arrival,
  type Axis,
  type Point,
  type Room,
  type Size,
} from "./zoom.js";

/// Widths are requested in steps of this many device pixels, so that a
/// slightly resized window reuses the images already drawn.
const WIDTH_STEP = 200;
/// Drawn images kept on each side of the current page.
const KEEP_AROUND = 2;
/// Wheel events less than `WHEEL_GAP` milliseconds apart form one gesture:
/// a notch of a mouse wheel, or a touchpad swipe and its momentum. A
/// gesture turns one page, once it has travelled `WHEEL_STEP` pixels.
const WHEEL_GAP = 40;
const WHEEL_STEP = 40;
/// Pixels counted for a wheel that reports lines or pages.
const WHEEL_LINE = 40;
const WHEEL_PAGE = 800;
/// Wait after the last resize, or the last change of zoom, before drawing
/// at the new size.
const SETTLE_DELAY = 200;
/// How far an arrow key moves the page, in CSS pixels; a page key moves it
/// by the height of the window less this overlap.
const ARROW_STEP = 40;
const PAGE_OVERLAP = 40;

/// The keys of the view, in its bar: with the page fitted to the window,
/// and with a page larger than it.
const HINT_FITTED =
  "← → ou molette : page précédente ou suivante · Ctrl+molette : zoom · un numéro puis Entrée : aller à la page · F4 : vignettes · R, Maj+R : pivoter · Échap : retour à la grille";
const HINT_ZOOMED =
  "glisser, molette ou flèches : se déplacer dans la page · au bord, page précédente ou suivante · Ctrl+molette : zoom · Ctrl+0 : page entière · Échap : retour à la grille";

export interface ViewerElements {
  /// The view, beside the grid or over it.
  root: HTMLElement;
  /// The area the page is shown in; its padding is what a fitted page keeps
  /// clear.
  stage: HTMLElement;
  /// The page itself, an image or a message, sized and placed by the view.
  page: HTMLElement;
  /// The caption as a sentence, read out when the page changes.
  caption: HTMLElement;
  /// The number of the page shown, in its form, and what follows it.
  numberForm: HTMLFormElement;
  number: HTMLInputElement;
  numberAfter: HTMLElement;
  /// The keys of the view, and in their place, while a number is typed,
  /// what it expects or why it went nowhere.
  hint: HTMLElement;
  help: HTMLElement;
  prev: HTMLButtonElement;
  next: HTMLButtonElement;
  close: HTMLButtonElement;
  rotateLeft: HTMLButtonElement;
  rotateRight: HTMLButtonElement;
  /// The zoom: out, the level (a click fits the page again), in.
  zoomOut: HTMLButtonElement;
  zoomLevel: HTMLButtonElement;
  zoomIn: HTMLButtonElement;
}

export interface ViewerOptions {
  /// The thumbnail of a source page, if drawn: shown enlarged until the
  /// page itself is.
  thumbnail(page: number): string | undefined;
  /// The user went back to the grid; `position` is the page shown last.
  closed(position: number): void;
  /// The user asked to turn source page `page` by `degrees`, 90 clockwise
  /// or -90 counter-clockwise.
  rotate(page: number, degrees: number): void;
  /// Whether source page `page` is being turned.
  turning(page: number): boolean;
  /// A page is shown: another one, or the same one again.
  shown(): void;
  /// The view has nothing left to draw for now.
  idle(): void;
}

interface Drawn {
  img: HTMLImageElement;
  /// Width requested, in device pixels.
  width: number;
}

/// A wheel gesture: when its last event came, how far it has travelled,
/// and what it does. Without Ctrl, it turns one page at most (`turned`),
/// or moves the page under the wheel when it began away from the edge
/// (`turning` false); with Ctrl, `applied` counts the zoom levels it has
/// gone out, so that a gesture that turns back comes back.
interface Gesture {
  last: number;
  travel: number;
  turned: boolean;
  turning: boolean;
  applied: number;
}

/// A drag of the page: the pointer, where it went down, and where the page
/// was then.
interface Drag {
  pointerId: number;
  from: Point;
  offset: Point;
}

/// Height over width of a page as displayed, `/Rotate` applied; Letter
/// when the size is unknown.
export function pageRatio(size: PageInfo | undefined): number {
  const { width, height, rotate } = size ?? { width: 612, height: 792, rotate: 0 };
  return rotate % 180 !== 0 ? width / height : height / width;
}

export class PageViewer {
  private readonly el: ViewerElements;
  private readonly options: ViewerOptions;
  private readonly resizeObserver: ResizeObserver;
  private pages: readonly PageInfo[] = [];
  private order: readonly number[] = [];
  private enabled = false;
  private opened = false;
  private current = 0;
  private drawn = new Map<number, Drawn>();
  private failed = new Map<number, string>();
  private busy = false;
  private generation = 0;
  /// How many times each source page was turned: a drawing counts for the
  /// version of its page when it was asked for.
  private versions = new Map<number, number>();
  private settleTimer: number | undefined;
  /// The zoom, as a factor of the page fitted to the window (`zoom.ts`).
  private zoom = 1;
  /// Where the page is: its top-left corner, in CSS pixels from the
  /// top-left of the stage. Kept from one page to the next.
  private offset: Point = { x: 0, y: 0 };
  /// The page as shown, and the stage, as last laid out.
  private size: Size = { width: 0, height: 0 };
  private room: Room = { width: 0, height: 0, left: 0, top: 0, right: 0, bottom: 0 };
  private wheel: Gesture = { last: Number.NEGATIVE_INFINITY, travel: 0, turned: false, turning: true, applied: 0 };
  private zoomWheel: Gesture = { last: Number.NEGATIVE_INFINITY, travel: 0, turned: false, turning: true, applied: 0 };
  private drag: Drag | null = null;

  constructor(elements: ViewerElements, options: ViewerOptions) {
    this.el = elements;
    this.options = options;
    this.resizeObserver = new ResizeObserver(() => this.resized());
    elements.prev.addEventListener("click", () => this.go(this.current - 1));
    elements.next.addEventListener("click", () => this.go(this.current + 1));
    // Of what the view offers, only this button and Escape go back to the
    // grid. Beside the panel, the
    // dark ground around the page is part of the view: a click that strays
    // there must not lose the place, and a double-click on the page is kept
    // for selecting a word, once pages have their text.
    elements.close.addEventListener("click", () => this.close());
    elements.rotateLeft.addEventListener("click", () => this.rotate(-90));
    elements.rotateRight.addEventListener("click", () => this.rotate(90));
    elements.zoomOut.addEventListener("click", () => this.zoomStep(-1));
    elements.zoomIn.addEventListener("click", () => this.zoomStep(1));
    elements.zoomLevel.addEventListener("click", () => this.zoomTo(1));
    elements.root.addEventListener("wheel", (event) => this.wheeled(event), { passive: false });
    // A page larger than the window is dragged, from the page or from the
    // ground around it.
    elements.stage.addEventListener("pointerdown", (event) => this.dragStart(event));
    elements.stage.addEventListener("pointermove", (event) => this.dragMove(event));
    elements.stage.addEventListener("pointerup", (event) => this.dragEnd(event));
    elements.stage.addEventListener("pointercancel", (event) => this.dragEnd(event));

    const number = elements.number;
    elements.numberForm.addEventListener("submit", (event) => {
      event.preventDefault();
      this.goToNumber();
    });
    // A click in the number selects it, so that typing replaces it; a click
    // in a number being typed places the caret.
    number.addEventListener("mousedown", (event) => {
      if (event.button === 0 && document.activeElement !== number) {
        event.preventDefault();
        number.focus();
        number.select();
      }
    });
    number.addEventListener("focus", () => this.numberHelp());
    number.addEventListener("input", () => this.numberHelp());
    number.addEventListener("blur", () => this.leaveNumber());
    number.addEventListener("keydown", (event) => {
      // Escape drops what was typed: the keys are those of the view again.
      if (event.key === "Escape") {
        event.preventDefault();
        this.focus();
      }
    });
  }

  get isOpen(): boolean {
    return this.opened;
  }

  /// Position in the order of the page shown, or shown last.
  get position(): number {
    return this.current;
  }

  /// Whether a page is being drawn for the view, or about to be: a zoom or
  /// a resize waits for the gesture to settle before it draws, and the
  /// thumbnails wait with it.
  get isDrawing(): boolean {
    return this.busy || this.settleTimer !== undefined;
  }

  /// Give the keyboard to the view.
  focus(): void {
    if (this.opened) {
      this.el.root.focus({ preventScroll: true });
    }
  }

  /// Show the page at `position` in `order` (indices into `pages`), fitted
  /// to the window. The order must not change while the view is open; the
  /// pages may, when one is turned (`pagesChanged`).
  open(pages: readonly PageInfo[], order: readonly number[], position: number, enabled: boolean): void {
    if (order.length === 0) {
      return;
    }
    this.pages = pages;
    this.order = order;
    this.enabled = enabled;
    this.opened = true;
    this.current = clamp(position, 0, order.length - 1);
    this.zoom = 1;
    this.offset = { x: 0, y: 0 };
    // Wide enough for the largest number.
    this.el.number.style.width = `calc(${String(order.length).length + 1}ch + 10px)`;
    this.el.root.hidden = false;
    this.resizeObserver.observe(this.el.stage);
    this.show();
    this.el.root.focus({ preventScroll: true });
  }

  /// Back to the grid.
  close(): void {
    if (!this.opened) {
      return;
    }
    this.hide();
    this.options.closed(this.current);
  }

  /// Another document: hide the view and forget its images.
  reset(): void {
    this.hide();
    this.generation += 1;
    this.drawn.clear();
    this.failed.clear();
    this.versions.clear();
    this.el.page.replaceChildren();
  }

  /// The pages changed: `pages` describes them all, and the source pages
  /// `turned` were turned, so their images are out of date. The page on
  /// screen is shown again if it is one of them.
  pagesChanged(pages: readonly PageInfo[], turned: readonly number[]): void {
    this.pages = pages;
    for (const page of turned) {
      this.drawn.delete(page);
      this.failed.delete(page);
      this.versions.set(page, this.version(page) + 1);
    }
    const page = this.order[this.current];
    if (this.opened && page !== undefined && turned.includes(page)) {
      this.show();
    } else {
      this.refreshTurning();
      this.pump();
    }
  }

  /// Dim the page on screen while it is being turned.
  refreshTurning(): void {
    const page = this.order[this.current];
    this.el.page.classList.toggle("turning", this.opened && page !== undefined && this.options.turning(page));
  }

  /// The keys of the view; `true` when `event` was one of them.
  handleKey(event: KeyboardEvent): boolean {
    if (!this.opened) {
      return false;
    }
    // Ctrl with plus, minus or 0, whose default the guard of `main.ts`
    // prevents (`shortcuts.ts`), and which the view gives its zoom.
    const command = zoomKey(event);
    if (command !== undefined) {
      if (command === "fit") {
        this.zoomTo(1);
      } else {
        this.zoomStep(command === "in" ? 1 : -1);
      }
      event.preventDefault();
      return true;
    }
    if (event.ctrlKey || event.altKey || event.metaKey) {
      return false;
    }
    switch (event.key) {
      case "Escape":
        this.close();
        break;
      case "ArrowLeft":
        this.moveOrTurn("x", -1, ARROW_STEP, event.repeat);
        break;
      case "ArrowRight":
        this.moveOrTurn("x", 1, ARROW_STEP, event.repeat);
        break;
      case "ArrowUp":
      case "ArrowDown":
        // Up and down only move a page taller than the window: they are
        // no keys of the view otherwise.
        if (!overflows(this.size, this.room).y) {
          return false;
        }
        this.moveBy("y", event.key === "ArrowUp" ? ARROW_STEP : -ARROW_STEP);
        break;
      case "PageUp":
        this.moveOrTurn("y", -1, this.screenStep(), event.repeat);
        break;
      case "PageDown":
        this.moveOrTurn("y", 1, this.screenStep(), event.repeat);
        break;
      case "Home":
        this.go(0);
        break;
      case "End":
        this.go(this.order.length - 1);
        break;
      case "r":
      case "R":
        if (!event.repeat) {
          this.rotate(event.shiftKey ? -90 : 90);
        }
        break;
      default:
        // A digit starts the number of a page to go to.
        if (!/^[0-9]$/.test(event.key)) {
          return false;
        }
        this.startNumber(event.key);
    }
    event.preventDefault();
    return true;
  }

  /// Show the page at `position` in the order, or the nearest one there is,
  /// at the same zoom and, unless `arrival` says which edge it is entered
  /// by, at the same place in the page.
  go(position: number, arrival?: Arrival): void {
    const target = clamp(position, 0, this.order.length - 1);
    if (this.opened && target !== this.current) {
      this.current = target;
      if (arrival !== undefined) {
        this.offset = arriveAt(this.offset, arrival);
      }
      this.show();
    }
  }

  private hide(): void {
    this.opened = false;
    // A number being typed goes with the view, and the keys go back to the
    // grid.
    if (document.activeElement === this.el.number) {
      this.el.number.blur();
    }
    this.el.root.hidden = true;
    this.resizeObserver.disconnect();
    window.clearTimeout(this.settleTimer);
    this.settleTimer = undefined;
    this.drag = null;
    this.el.stage.classList.remove("panning");
  }

  /// Ask for the page on screen to be turned by `degrees`.
  private rotate(degrees: number): void {
    const page = this.order[this.current];
    if (this.opened && page !== undefined) {
      this.options.rotate(page, degrees);
    }
  }

  /// A digit typed in the view: the number of a page to go to begins.
  private startNumber(digit: string): void {
    const number = this.el.number;
    number.value = digit;
    number.focus();
    number.setSelectionRange(digit.length, digit.length);
  }

  /// Go to the page whose number was typed. A number that names no page
  /// changes nothing: the view says what it expects, and the number stays,
  /// selected, to be typed again.
  private goToNumber(): void {
    const count = this.order.length;
    const typed = readPageNumber(this.el.number.value, count);
    if (typed.kind !== "page") {
      this.el.number.setAttribute("aria-invalid", "true");
      this.el.number.select();
      this.say(pageNumberRefusal(typed, count), true);
      return;
    }
    this.go(typed.position);
    this.focus();
  }

  /// While a number is typed: what it expects, in place of the keys.
  private numberHelp(): void {
    if (this.el.help.hidden || this.el.number.hasAttribute("aria-invalid")) {
      this.el.number.removeAttribute("aria-invalid");
      this.say(pageNumberHelp(this.order.length), false);
    }
  }

  /// The number is no longer typed: it is that of the page shown again, and
  /// the keys of the view come back.
  private leaveNumber(): void {
    this.el.number.value = String(this.current + 1);
    this.el.number.removeAttribute("aria-invalid");
    this.el.help.hidden = true;
    this.el.hint.hidden = false;
  }

  /// Put `text` in place of the keys: what the number expects or, when
  /// `refused`, why it went nowhere.
  private say(text: string, refused: boolean): void {
    this.el.help.textContent = text;
    this.el.help.classList.toggle("refused", refused);
    this.el.help.hidden = false;
    this.el.hint.hidden = true;
  }

  /// Caption, buttons and the best image at hand for the current page,
  /// then draw what is missing.
  private show(): void {
    const position = this.current;
    const page = this.order[position];
    if (page === undefined) {
      return;
    }
    const count = this.order.length;
    const caption = pageCaption(position, page, count);
    this.el.caption.textContent = caption.spoken;
    this.el.numberAfter.textContent = caption.after;
    // Not over a number being typed.
    if (document.activeElement !== this.el.number) {
      this.el.number.value = String(position + 1);
    }
    this.el.prev.disabled = position === 0;
    this.el.next.disabled = position === count - 1;
    this.forgetFar();
    const drawn = this.drawn.get(page);
    const failure = this.failed.get(page);
    const thumbnail = this.options.thumbnail(page);
    if (drawn !== undefined) {
      this.display(drawn.img);
    } else if (failure !== undefined) {
      this.message(`Rendu impossible : ${failure}`, true);
    } else if (!this.enabled) {
      this.message("aperçu indisponible", false);
    } else if (thumbnail !== undefined) {
      this.display(imageOf(thumbnail));
    } else {
      this.message("…", false);
    }
    this.layout();
    this.refreshTurning();
    this.options.shown();
    this.pump();
  }

  /// Size the page: fitted to the stage and enlarged by the zoom, which the
  /// stage may cap (a window that grew), and placed where the offset says,
  /// within bounds. The bar follows: the zoom it shows, the buttons it
  /// leaves active, the keys it names.
  private layout(): void {
    const page = this.order[this.current];
    if (!this.opened || page === undefined) {
      return;
    }
    const room = this.measure();
    const fitted = fit(inner(room), this.ratio(page));
    const max = maxZoom(fitted.width, window.devicePixelRatio);
    if (this.zoom > max) {
      this.zoom = max;
    }
    this.size = scaled(fitted, this.zoom);
    this.el.page.style.width = `${this.size.width}px`;
    this.el.page.style.height = `${this.size.height}px`;
    this.place(this.offset);
    const over = overflows(this.size, room);
    const movable = over.x || over.y;
    this.el.stage.classList.toggle("pannable", movable);
    this.el.zoomOut.disabled = this.zoom <= 1;
    this.el.zoomIn.disabled = this.zoom >= max;
    this.el.zoomLevel.textContent = zoomLabel(this.zoom);
    this.el.hint.textContent = movable ? HINT_ZOOMED : HINT_FITTED;
  }

  /// The stage as it is now: its box, and its padding, in CSS pixels.
  private measure(): Room {
    const stage = this.el.stage;
    const style = getComputedStyle(stage);
    this.room = {
      width: stage.clientWidth,
      height: stage.clientHeight,
      left: px(style.paddingLeft),
      top: px(style.paddingTop),
      right: px(style.paddingRight),
      bottom: px(style.paddingBottom),
    };
    return this.room;
  }

  /// Put the page at `offset`, within bounds, on whole device pixels: at
  /// the largest zoom the image is shown pixel for pixel.
  private place(offset: Point): void {
    const bounded = clampOffset(offset, this.size, this.room);
    const dpr = window.devicePixelRatio;
    this.offset = { x: Math.round(bounded.x * dpr) / dpr, y: Math.round(bounded.y * dpr) / dpr };
    this.el.page.style.transform = `translate(${this.offset.x}px, ${this.offset.y}px)`;
  }

  /// Height over width of `page`: that of its image once drawn, which is
  /// what the renderer shows; before that, the media box and rotation
  /// listed by the document.
  private ratio(page: number): number {
    const img = this.drawn.get(page)?.img;
    if (img !== undefined && img.naturalWidth > 0) {
      return img.naturalHeight / img.naturalWidth;
    }
    return pageRatio(this.pages[page]);
  }

  /// The zoom levels the stage allows for the current page.
  private levels(): number[] {
    const page = this.order[this.current];
    const fitted = fit(inner(this.room), this.ratio(page ?? 0));
    return zoomLevels(maxZoom(fitted.width, window.devicePixelRatio));
  }

  /// Show the page `zoom` times its fitted size, or as close as the stage
  /// allows, the point of the page under `pointer` (in the coordinates of
  /// the stage) staying where it is; the centre of the stage without a
  /// pointer. The image on screen is scaled at once; a sharper one is asked
  /// for once the gesture has settled.
  private zoomTo(zoom: number, pointer?: Point): void {
    const page = this.order[this.current];
    if (!this.opened || page === undefined) {
      return;
    }
    const room = this.measure();
    const padded = inner(room);
    const fitted = fit(padded, this.ratio(page));
    const target = clamp(zoom, 1, maxZoom(fitted.width, window.devicePixelRatio));
    if (target === this.zoom) {
      return;
    }
    const at = pointer ?? { x: room.left + padded.width / 2, y: room.top + padded.height / 2 };
    this.offset = zoomAround(at, this.offset, this.size, scaled(fitted, target));
    this.zoom = target;
    this.layout();
    this.settle();
  }

  /// One level in (`direction` 1) or out (-1).
  private zoomStep(direction: 1 | -1, pointer?: Point): void {
    const levels = this.levels();
    this.zoomTo(direction > 0 ? zoomIn(this.zoom, levels) : zoomOut(this.zoom, levels), pointer);
  }

  /// Move the page by `amount` along `axis`, within bounds.
  private moveBy(axis: Axis, amount: number): void {
    const offset = { ...this.offset };
    offset[axis] += amount;
    this.place(offset);
  }

  /// A key that moves the page by `step` along `axis`, in `direction` (1
  /// for the end, -1 for the start), where the page exceeds the window on
  /// that axis; where it does not, the key turns the page, as it did before
  /// the zoom. At the edge, the key turns the page too, on a fresh press
  /// only: a key held down stops at the edge.
  private moveOrTurn(axis: Axis, direction: 1 | -1, step: number, repeat: boolean): void {
    if (!overflows(this.size, this.room)[axis]) {
      this.go(this.current + direction);
    } else if (atEdge(this.offset, this.size, this.room, axis, direction)) {
      if (!repeat) {
        this.go(this.current + direction, { axis, edge: direction > 0 ? "start" : "end" });
      }
    } else {
      this.moveBy(axis, -direction * step);
    }
  }

  /// How far a page key moves the page: a window's height, less an overlap.
  private screenStep(): number {
    return Math.max(ARROW_STEP, inner(this.room).height - PAGE_OVERLAP);
  }

  /// Where `event` points, in the coordinates of the stage.
  private pointerAt(event: MouseEvent): Point {
    const rect = this.el.stage.getBoundingClientRect();
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  }

  /// Start the next drawing, if none is running and nothing is settling;
  /// with nothing left to draw, say that the renderer is free.
  private pump(): void {
    if (this.busy || this.settleTimer !== undefined) {
      return;
    }
    const next = this.nextDrawing();
    if (next === null) {
      this.options.idle();
    } else {
      void this.draw(next.page, next.width);
    }
  }

  /// Draw once the zoom or the window has settled: the image on screen is
  /// scaled by the browser meanwhile, and a drawing already running is
  /// left to finish. The thumbnails wait too (`isDrawing`).
  private settle(): void {
    window.clearTimeout(this.settleTimer);
    this.settleTimer = window.setTimeout(() => {
      this.settleTimer = undefined;
      this.pump();
    }, SETTLE_DELAY);
  }

  /// The drawing to start next, if any: the current page, as wide as it is
  /// shown, then the next one, then the previous one, as wide as fitted to
  /// the window. A neighbour turned to while zoomed is shown from its
  /// fitted image, enlarged, until its own drawing comes.
  private nextDrawing(): { page: number; width: number } | null {
    if (!this.opened || !this.enabled) {
      return null;
    }
    const padded = inner(this.measure());
    if (padded.width === 0 || padded.height === 0) {
      return null;
    }
    for (const position of [this.current, this.current + 1, this.current - 1]) {
      const page = this.order[position];
      if (page === undefined || this.failed.has(page)) {
        continue;
      }
      const zoom = position === this.current ? this.zoom : 1;
      const width = renderWidth(scaled(fit(padded, this.ratio(page)), zoom).width);
      if ((this.drawn.get(page)?.width ?? 0) < width) {
        return { page, width };
      }
    }
    return null;
  }

  private async draw(page: number, width: number): Promise<void> {
    this.busy = true;
    const generation = this.generation;
    const version = this.version(page);
    // Not for another document, nor for the page as it was before a turn.
    const current = (): boolean => generation === this.generation && version === this.version(page);
    try {
      const img = imageOf(await renderPage(page, width));
      // Decoded before it is shown: no blank frame when turning to it.
      await img.decode();
      if (current()) {
        this.drawn.set(page, { img, width });
        if (this.opened && this.order[this.current] === page) {
          this.display(img);
          this.layout();
        }
      }
    } catch (e: unknown) {
      if (current()) {
        const error = asAppError(e);
        const text = error.kind === "other" ? error.message : "mot de passe refusé";
        this.failed.set(page, text);
        if (this.opened && this.order[this.current] === page && !this.drawn.has(page)) {
          this.message(`Rendu impossible : ${text}`, true);
        }
      }
    } finally {
      this.busy = false;
      this.forgetFar();
      this.pump();
    }
  }

  private version(page: number): number {
    return this.versions.get(page) ?? 0;
  }

  /// Drop the images of pages more than `KEEP_AROUND` positions away.
  private forgetFar(): void {
    const from = Math.max(0, this.current - KEEP_AROUND);
    const near = new Set(this.order.slice(from, this.current + KEEP_AROUND + 1));
    for (const page of this.drawn.keys()) {
      if (!near.has(page)) {
        this.drawn.delete(page);
      }
    }
  }

  private display(img: HTMLImageElement): void {
    this.el.page.classList.remove("failed");
    this.el.page.replaceChildren(img);
  }

  private message(text: string, failed: boolean): void {
    const span = document.createElement("span");
    span.className = "placeholder";
    span.textContent = text;
    this.el.page.classList.toggle("failed", failed);
    this.el.page.replaceChildren(span);
  }

  private resized(): void {
    this.layout();
    this.settle();
  }

  /// The wheel, in the direction of the larger axis (Shift makes a
  /// vertical wheel horizontal where the browser does not). With Ctrl, it
  /// zooms around the pointer; without, it moves the page or turns it.
  private wheeled(event: WheelEvent): void {
    event.preventDefault();
    if (!this.opened) {
      return;
    }
    let dx = event.deltaX;
    let dy = event.deltaY;
    if (event.shiftKey && dx === 0) {
      dx = dy;
      dy = 0;
    }
    const axis: Axis = Math.abs(dx) > Math.abs(dy) ? "x" : "y";
    const raw = axis === "x" ? dx : dy;
    if (raw === 0) {
      return;
    }
    const pixels = event.deltaMode === WheelEvent.DOM_DELTA_PIXEL;
    if (event.ctrlKey) {
      // A wheel that reports lines or pages: one notch, one level.
      this.wheelZoom(event, pixels ? raw : Math.sign(raw) * ZOOM_NOTCH);
      return;
    }
    const scale = event.deltaMode === WheelEvent.DOM_DELTA_LINE ? WHEEL_LINE : pixels ? 1 : WHEEL_PAGE;
    this.wheelMove(event, axis, raw * scale);
  }

  /// Ctrl+wheel, or the pinch of a touchpad, which the browser reports the
  /// same way: one level per notch (`zoomTicks`), around the pointer. Down
  /// zooms out, as in the browsers.
  private wheelZoom(event: WheelEvent, delta: number): void {
    const wheel = this.zoomWheel;
    if (event.timeStamp - wheel.last > WHEEL_GAP) {
      wheel.travel = 0;
      wheel.applied = 0;
    }
    wheel.last = event.timeStamp;
    wheel.travel += delta;
    const wanted = Math.sign(wheel.travel) * zoomTicks(wheel.travel);
    const pointer = this.pointerAt(event);
    for (; wheel.applied < wanted; wheel.applied += 1) {
      this.zoomStep(-1, pointer);
    }
    for (; wheel.applied > wanted; wheel.applied -= 1) {
      this.zoomStep(1, pointer);
    }
  }

  /// The wheel without Ctrl, along `axis`. Where the page exceeds the
  /// window on that axis, a gesture that begins away from its edge moves
  /// the page, and goes no further than the edge: the momentum of a
  /// touchpad never turns the page. A gesture that begins at the edge, or
  /// on an axis where the page fits, turns one page, once it has travelled
  /// `WHEEL_STEP`, entered by the edge the travel continues through.
  private wheelMove(event: WheelEvent, axis: Axis, delta: number): void {
    const wheel = this.wheel;
    if (event.timeStamp - wheel.last > WHEEL_GAP) {
      wheel.travel = 0;
      wheel.turned = false;
      wheel.turning = atEdge(this.offset, this.size, this.room, axis, Math.sign(delta));
    }
    wheel.last = event.timeStamp;
    if (!wheel.turning) {
      this.moveBy(axis, -delta);
      return;
    }
    if (wheel.turned) {
      return;
    }
    wheel.travel += delta;
    if (Math.abs(wheel.travel) >= WHEEL_STEP) {
      wheel.turned = true;
      this.go(this.current + Math.sign(wheel.travel), { axis, edge: wheel.travel > 0 ? "start" : "end" });
    }
  }

  /// A press on the page, or on the ground around it, takes hold of a page
  /// larger than the window; the buttons over the stage keep their clicks.
  private dragStart(event: PointerEvent): void {
    if (!this.opened || event.button !== 0 || this.drag !== null) {
      return;
    }
    if (event.target instanceof Element && event.target.closest("button") !== null) {
      return;
    }
    const over = overflows(this.size, this.room);
    if (!over.x && !over.y) {
      return;
    }
    this.drag = { pointerId: event.pointerId, from: { x: event.clientX, y: event.clientY }, offset: this.offset };
    this.el.stage.setPointerCapture(event.pointerId);
    this.el.stage.classList.add("panning");
  }

  /// The page follows the pointer, within bounds, from where it was taken
  /// hold of: pulled past an edge and back, it does not drift.
  private dragMove(event: PointerEvent): void {
    const drag = this.drag;
    if (drag === null || event.pointerId !== drag.pointerId) {
      return;
    }
    this.place({ x: drag.offset.x + event.clientX - drag.from.x, y: drag.offset.y + event.clientY - drag.from.y });
  }

  private dragEnd(event: PointerEvent): void {
    const drag = this.drag;
    if (drag === null || event.pointerId !== drag.pointerId) {
      return;
    }
    this.drag = null;
    this.el.stage.classList.remove("panning");
    if (this.el.stage.hasPointerCapture(event.pointerId)) {
      this.el.stage.releasePointerCapture(event.pointerId);
    }
  }
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(value, max));
}

/// A computed CSS length in pixels, 0 when it is not one.
function px(value: string): number {
  const n = Number.parseFloat(value);
  return Number.isFinite(n) ? n : 0;
}

/// Device pixels to request for a page shown `cssWidth` CSS pixels wide.
function renderWidth(cssWidth: number): number {
  const pixels = Math.ceil((cssWidth * window.devicePixelRatio) / WIDTH_STEP) * WIDTH_STEP;
  return clamp(pixels, WIDTH_STEP, MAX_WIDTH);
}

function imageOf(url: string): HTMLImageElement {
  const img = new Image();
  img.alt = "";
  img.draggable = false;
  img.src = url;
  return img;
}
