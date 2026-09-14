// Progressive thumbnails: a tile asks for its image when it scrolls into
// view, a small number of requests run at once, and a page that leaves
// the view before its turn is simply not drawn. Images are cached per
// source page, so reordering never draws twice; turning a page forgets its
// image, and an answer drawn before the turn is ignored.
//
// The renderer serves one request at a time, and the page view goes first:
// how many thumbnails may be drawn at once depends on it (`thumbnailSlots`).

import { renderPage } from "./api.js";

const CONCURRENCY = 3;

/// The page view, as the thumbnails see it: whether it is open, keeps the
/// grid beside it as a panel of thumbnails, and has a page being drawn.
export interface ViewState {
  open: boolean;
  panel: boolean;
  drawing: boolean;
}

/// How many thumbnails may be drawn at once: `CONCURRENCY` with the grid on
/// screen. With the page view open, none while it draws a page, so that the
/// page on screen comes before them, nor without the panel, whose tiles are
/// then covered; otherwise one, so that the next page the view asks for
/// waits for one thumbnail at most, once those asked for over the grid
/// before the view opened are done.
export function thumbnailSlots(view: ViewState): number {
  if (!view.open) {
    return CONCURRENCY;
  }
  return view.panel && !view.drawing ? 1 : 0;
}

export class ThumbnailLoader {
  private cache = new Map<number, string>();
  private failed = new Map<number, string>();
  private queue: { page: number; tile: HTMLElement }[] = [];
  private running = 0;
  private observer: IntersectionObserver;
  private width: number;
  private enabled: boolean;
  /// How many requests may run now (`thumbnailSlots`): asked before each
  /// one is sent.
  private readonly slots: () => number;
  private generation = 0;
  /// How many times each source page was turned: a request answers for
  /// the version of its page when it was sent.
  private versions = new Map<number, number>();

  constructor(root: HTMLElement, width: number, enabled: boolean, slots: () => number) {
    this.width = width;
    this.enabled = enabled;
    this.slots = slots;
    this.observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const tile = entry.target as HTMLElement;
          if (entry.isIntersecting) {
            this.want(tile);
          } else {
            this.forget(tile);
          }
        }
      },
      { root, rootMargin: "200px" },
    );
  }

  /// Forget every image: a new document was opened.
  reset(enabled: boolean): void {
    this.generation += 1;
    this.enabled = enabled;
    this.cache.clear();
    this.failed.clear();
    this.versions.clear();
    this.queue = [];
    this.observer.disconnect();
  }

  /// Forget the images of the source pages `pages`, which were turned:
  /// their tiles, built again, ask for new ones. A request still running
  /// for one of them is ignored when it answers.
  invalidate(pages: readonly number[]): void {
    for (const page of pages) {
      this.cache.delete(page);
      this.failed.delete(page);
      this.versions.set(page, this.version(page) + 1);
    }
    this.queue = this.queue.filter((q) => !pages.includes(q.page));
  }

  /// More requests may run than `slots` allowed when last asked: send those
  /// that wait. Requests already sent always finish.
  wake(): void {
    this.pump();
  }

  /// The thumbnail of source page `page`, if it is already drawn.
  cached(page: number): string | undefined {
    return this.cache.get(page);
  }

  /// Start watching a tile whose `data-page` is the source page index.
  watch(tile: HTMLElement): void {
    const page = pageOf(tile);
    const cached = this.cache.get(page);
    if (cached !== undefined) {
      show(tile, cached);
      return;
    }
    const failure = this.failed.get(page);
    if (failure !== undefined) {
      showFailure(tile, failure);
      return;
    }
    this.observer.observe(tile);
  }

  private want(tile: HTMLElement): void {
    const page = pageOf(tile);
    if (!this.enabled || this.cache.has(page) || this.failed.has(page)) {
      return;
    }
    if (this.queue.some((q) => q.tile === tile)) {
      return;
    }
    // Newly visible pages go first: the user is looking at them.
    this.queue.unshift({ page, tile });
    this.pump();
  }

  private forget(tile: HTMLElement): void {
    this.queue = this.queue.filter((q) => q.tile !== tile);
  }

  private pump(): void {
    while (this.running < this.slots()) {
      const next = this.queue.shift();
      if (next === undefined) {
        return;
      }
      this.running += 1;
      const generation = this.generation;
      const version = this.version(next.page);
      const current = (): boolean => generation === this.generation && version === this.version(next.page);
      renderPage(next.page, this.width)
        .then((url) => {
          if (!current()) {
            return;
          }
          this.cache.set(next.page, url);
          show(next.tile, url);
        })
        .catch((e: unknown) => {
          if (!current()) {
            return;
          }
          const message = describe(e);
          this.failed.set(next.page, message);
          showFailure(next.tile, message);
        })
        .finally(() => {
          this.running -= 1;
          this.observer.unobserve(next.tile);
          this.pump();
        });
    }
  }

  private version(page: number): number {
    return this.versions.get(page) ?? 0;
  }
}

function pageOf(tile: HTMLElement): number {
  return Number(tile.dataset["page"] ?? "0");
}

function show(tile: HTMLElement, url: string): void {
  const img = tile.querySelector<HTMLImageElement>("img.thumb");
  if (img === null) {
    return;
  }
  img.src = url;
  img.hidden = false;
  tile.classList.add("loaded");
}

function showFailure(tile: HTMLElement, message: string): void {
  tile.classList.add("failed");
  tile.title = message;
}

function describe(e: unknown): string {
  if (typeof e === "object" && e !== null && "message" in e) {
    return String((e as { message: unknown }).message);
  }
  return String(e);
}
