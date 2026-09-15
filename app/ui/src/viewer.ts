// The page view: one page as large as the window allows, drawn beside the
// grid or over it, the grid staying in place. It is a state of the window,
// not a dialog (ADR 0004): the toolbar, the notices and the status bar stay
// visible.
//
// The renderer serves one request at a time, so the view keeps at most one
// request of its own in flight: the page on screen first, then its two
// neighbours once it is shown, so that turning to them is immediate. A page
// skipped while turning quickly is never drawn. Images of the pages near
// the current one are kept, the others dropped.
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

/// Widest image the renderer draws (`RenderService::render` clamps to it).
const MAX_WIDTH = 4096;
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
/// Wait after the last resize before drawing at the new size.
const RESIZE_DELAY = 200;

export interface ViewerElements {
  /// The view, beside the grid or over it.
  root: HTMLElement;
  /// The area the page is fitted in: its content box.
  stage: HTMLElement;
  /// The page itself, an image or a message, sized by the view.
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

interface Size {
  width: number;
  height: number;
}

interface Drawn {
  img: HTMLImageElement;
  /// Width requested, in device pixels.
  width: number;
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
  private resizeTimer: number | undefined;
  private wheel = { last: Number.NEGATIVE_INFINITY, travel: 0, turned: false };

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
    elements.root.addEventListener("wheel", (event) => this.wheeled(event), { passive: false });

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

  /// Whether a page is being drawn for the view.
  get isDrawing(): boolean {
    return this.busy;
  }

  /// Give the keyboard to the view.
  focus(): void {
    if (this.opened) {
      this.el.root.focus({ preventScroll: true });
    }
  }

  /// Show the page at `position` in `order` (indices into `pages`). The
  /// order must not change while the view is open; the pages may, when
  /// one is turned (`pagesChanged`).
  open(pages: readonly PageInfo[], order: readonly number[], position: number, enabled: boolean): void {
    if (order.length === 0) {
      return;
    }
    this.pages = pages;
    this.order = order;
    this.enabled = enabled;
    this.opened = true;
    this.current = clamp(position, 0, order.length - 1);
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
    if (!this.opened || event.ctrlKey || event.altKey || event.metaKey) {
      return false;
    }
    switch (event.key) {
      case "Escape":
        this.close();
        break;
      case "ArrowLeft":
      case "PageUp":
        this.go(this.current - 1);
        break;
      case "ArrowRight":
      case "PageDown":
        this.go(this.current + 1);
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

  /// Show the page at `position` in the order, or the nearest one there is.
  go(position: number): void {
    const target = clamp(position, 0, this.order.length - 1);
    if (this.opened && target !== this.current) {
      this.current = target;
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
    window.clearTimeout(this.resizeTimer);
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

  /// Size the page to fit the stage.
  private layout(): void {
    const page = this.order[this.current];
    if (!this.opened || page === undefined) {
      return;
    }
    const size = fit(this.area(), this.ratio(page));
    this.el.page.style.width = `${size.width}px`;
    this.el.page.style.height = `${size.height}px`;
  }

  /// The content box of the stage, in CSS pixels.
  private area(): Size {
    const stage = this.el.stage;
    const style = getComputedStyle(stage);
    return {
      width: Math.max(0, stage.clientWidth - px(style.paddingLeft) - px(style.paddingRight)),
      height: Math.max(0, stage.clientHeight - px(style.paddingTop) - px(style.paddingBottom)),
    };
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

  /// Start the next drawing, if none is running; with nothing left to draw,
  /// say that the renderer is free.
  private pump(): void {
    if (this.busy) {
      return;
    }
    const next = this.nextDrawing();
    if (next === null) {
      this.options.idle();
    } else {
      void this.draw(next.page, next.width);
    }
  }

  /// The drawing to start next, if any: the current page, then the next
  /// one, then the previous one, each as wide as it is shown.
  private nextDrawing(): { page: number; width: number } | null {
    if (!this.opened || !this.enabled) {
      return null;
    }
    const area = this.area();
    if (area.width === 0 || area.height === 0) {
      return null;
    }
    for (const position of [this.current, this.current + 1, this.current - 1]) {
      const page = this.order[position];
      if (page === undefined || this.failed.has(page)) {
        continue;
      }
      const width = renderWidth(fit(area, this.ratio(page)).width);
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
    window.clearTimeout(this.resizeTimer);
    this.resizeTimer = window.setTimeout(() => this.pump(), RESIZE_DELAY);
  }

  /// One page per wheel gesture (see `WHEEL_GAP`), in the direction of the
  /// larger axis: down or right is the next page.
  private wheeled(event: WheelEvent): void {
    event.preventDefault();
    if (!this.opened || event.ctrlKey) {
      return;
    }
    const scale =
      event.deltaMode === WheelEvent.DOM_DELTA_LINE
        ? WHEEL_LINE
        : event.deltaMode === WheelEvent.DOM_DELTA_PAGE
          ? WHEEL_PAGE
          : 1;
    const delta = (Math.abs(event.deltaX) > Math.abs(event.deltaY) ? event.deltaX : event.deltaY) * scale;
    const wheel = this.wheel;
    if (event.timeStamp - wheel.last > WHEEL_GAP) {
      wheel.travel = 0;
      wheel.turned = false;
    }
    wheel.last = event.timeStamp;
    if (wheel.turned) {
      return;
    }
    wheel.travel += delta;
    if (Math.abs(wheel.travel) >= WHEEL_STEP) {
      wheel.turned = true;
      this.go(this.current + Math.sign(wheel.travel));
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

/// The largest size of height-over-width `ratio` that fits in `area`.
function fit(area: Size, ratio: number): Size {
  const width = Math.max(1, Math.floor(Math.min(area.width, area.height / ratio)));
  return { width, height: Math.max(1, Math.floor(width * ratio)) };
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
