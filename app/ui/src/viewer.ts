// The page view: one page as large as the window allows, drawn over the
// grid, which stays in place underneath. It is a state of the window, not
// a dialog (ADR 0004): the toolbar, the notices and the status bar stay
// visible.
//
// The renderer serves one request at a time, so the view keeps at most one
// request of its own in flight: the page on screen first, then its two
// neighbours once it is shown, so that turning to them is immediate. A page
// skipped while turning quickly is never drawn. Images of the pages near
// the current one are kept, the others dropped.

import { asAppError, renderPage, type PageInfo } from "./api.js";

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
  /// The view, over the grid.
  root: HTMLElement;
  /// The area the page is fitted in: its content box.
  stage: HTMLElement;
  /// The page itself, an image or a message, sized by the view.
  page: HTMLElement;
  caption: HTMLElement;
  prev: HTMLButtonElement;
  next: HTMLButtonElement;
  close: HTMLButtonElement;
}

export interface ViewerOptions {
  /// The thumbnail of a source page, if drawn: shown enlarged until the
  /// page itself is.
  thumbnail(page: number): string | undefined;
  /// The user went back to the grid; `position` is the page shown last.
  closed(position: number): void;
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
  private resizeTimer: number | undefined;
  private pressedOutside = false;
  private wheel = { last: Number.NEGATIVE_INFINITY, travel: 0, turned: false };

  constructor(elements: ViewerElements, options: ViewerOptions) {
    this.el = elements;
    this.options = options;
    this.resizeObserver = new ResizeObserver(() => this.resized());
    elements.prev.addEventListener("click", () => this.go(this.current - 1));
    elements.next.addEventListener("click", () => this.go(this.current + 1));
    elements.close.addEventListener("click", () => this.close());
    // A click outside the page goes back to the grid, if the button was
    // pressed outside too: pressing on the page and releasing beside it is
    // not a click outside.
    elements.root.addEventListener("pointerdown", (event) => {
      this.pressedOutside = event.button === 0 && this.outside(event.target);
    });
    elements.root.addEventListener("click", (event) => {
      if (this.pressedOutside && this.outside(event.target)) {
        this.close();
      }
      this.pressedOutside = false;
    });
    elements.root.addEventListener("wheel", (event) => this.wheeled(event), { passive: false });
  }

  get isOpen(): boolean {
    return this.opened;
  }

  /// Show the page at `position` in `order` (indices into `pages`). The
  /// order must not change while the view is open.
  open(pages: readonly PageInfo[], order: readonly number[], position: number, enabled: boolean): void {
    if (order.length === 0) {
      return;
    }
    this.pages = pages;
    this.order = order;
    this.enabled = enabled;
    this.opened = true;
    this.current = clamp(position, 0, order.length - 1);
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
    this.el.page.replaceChildren();
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
      default:
        return false;
    }
    event.preventDefault();
    return true;
  }

  private hide(): void {
    this.opened = false;
    this.el.root.hidden = true;
    this.resizeObserver.disconnect();
    window.clearTimeout(this.resizeTimer);
  }

  private go(position: number): void {
    const target = clamp(position, 0, this.order.length - 1);
    if (this.opened && target !== this.current) {
      this.current = target;
      this.show();
    }
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
    this.el.caption.textContent =
      page === position
        ? `Page ${position + 1} / ${count}`
        : `Page ${position + 1} / ${count} (page ${page + 1} du fichier)`;
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

  /// Start the next drawing, if any and if none is running: the current
  /// page, then the next one, then the previous one.
  private pump(): void {
    if (!this.opened || !this.enabled || this.busy) {
      return;
    }
    const area = this.area();
    if (area.width === 0 || area.height === 0) {
      return;
    }
    for (const position of [this.current, this.current + 1, this.current - 1]) {
      const page = this.order[position];
      if (page === undefined || this.failed.has(page)) {
        continue;
      }
      const width = renderWidth(fit(area, this.ratio(page)).width);
      if ((this.drawn.get(page)?.width ?? 0) < width) {
        void this.draw(page, width);
        return;
      }
    }
  }

  private async draw(page: number, width: number): Promise<void> {
    this.busy = true;
    const generation = this.generation;
    try {
      const img = imageOf(await renderPage(page, width));
      // Decoded before it is shown: no blank frame when turning to it.
      await img.decode();
      if (generation === this.generation) {
        this.drawn.set(page, { img, width });
        if (this.opened && this.order[this.current] === page) {
          this.display(img);
          this.layout();
        }
      }
    } catch (e: unknown) {
      if (generation === this.generation) {
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

  /// Neither on the page nor on a button of the view.
  private outside(target: EventTarget | null): boolean {
    if (!(target instanceof Element)) {
      return false;
    }
    return !this.el.page.contains(target) && target.closest("button") === null;
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
