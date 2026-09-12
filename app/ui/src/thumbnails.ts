// Progressive thumbnails: a tile asks for its image when it scrolls into
// view, a small number of requests run at once, and a page that leaves
// the view before its turn is simply not drawn. Images are cached per
// source page, so reordering never draws twice.

import { renderPage } from "./api.js";

const CONCURRENCY = 3;

export class ThumbnailLoader {
  private cache = new Map<number, string>();
  private failed = new Map<number, string>();
  private queue: { page: number; tile: HTMLElement }[] = [];
  private running = 0;
  private observer: IntersectionObserver;
  private width: number;
  private enabled: boolean;
  private generation = 0;

  constructor(root: HTMLElement, width: number, enabled: boolean) {
    this.width = width;
    this.enabled = enabled;
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
    this.queue = [];
    this.observer.disconnect();
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
    while (this.running < CONCURRENCY) {
      const next = this.queue.shift();
      if (next === undefined) {
        return;
      }
      this.running += 1;
      const generation = this.generation;
      renderPage(next.page, this.width)
        .then((url) => {
          if (generation !== this.generation) {
            return;
          }
          this.cache.set(next.page, url);
          show(next.tile, url);
        })
        .catch((e: unknown) => {
          if (generation !== this.generation) {
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
