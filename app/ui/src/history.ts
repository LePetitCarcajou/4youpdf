// The pages of the document being edited, with undo and redo: their order,
// a list of 0-based indices into the pages of the open file, and the size
// and rotation of each page as the Rust side describes them.
//
// Deleting pages removes their indices and moving pages reorders them:
// both are done here. Turning pages is not. The Rust side applies a
// rotation to the document through `ops::rotate` and answers with every
// page as it now stands; undoing a rotation is asking it for the opposite
// one. Nothing here works out a rotation.
//
// A rotation takes a round trip, so rotations run one after the other: one
// asked for while another runs waits for its turn. Until the last one is
// done, moving, deleting, undoing and redoing are refused, so that the
// history keeps the order in which things were done.

import { asAppError, type PageInfo } from "./api.js";

/// Turns the pages at the given indices of the open file by a multiple of
/// 90 degrees, clockwise when positive, and answers with every page as it
/// now stands: the `rotate_pages` command.
export type Rotator = (pages: number[], degrees: number) => Promise<PageInfo[]>;

/// How an undo, a redo or a rotation ended: the order changed; the pages
/// `pages` of the file turned; nothing to do, or refused while a rotation
/// runs; refused by the Rust side, in which case nothing changed.
export type Outcome =
  | { kind: "order" }
  | { kind: "rotation"; pages: readonly number[] }
  | { kind: "none" }
  | { kind: "failed"; message: string };

interface Reorder {
  kind: "order";
  before: readonly number[];
  after: readonly number[];
}

interface Rotation {
  kind: "rotation";
  pages: readonly number[];
  degrees: number;
}

type Edit = Reorder | Rotation;

const NONE: Outcome = { kind: "none" };

export class PageHistory {
  private past: Edit[] = [];
  private future: Edit[] = [];
  private current: readonly number[];
  private described: readonly PageInfo[];
  private readonly openedOrder: readonly number[];
  private readonly openedPages: readonly PageInfo[];
  private readonly rotator: Rotator;
  /// Rotations running or waiting, and how many of them turn each page.
  private running = 0;
  private turning = new Map<number, number>();
  private tail: Promise<unknown> = Promise.resolve();

  constructor(pages: readonly PageInfo[], rotator: Rotator) {
    this.current = pages.map((_, i) => i);
    this.openedOrder = this.current;
    this.described = pages;
    this.openedPages = pages;
    this.rotator = rotator;
  }

  get order(): readonly number[] {
    return this.current;
  }

  /// Size and rotation of each page of the file, by index.
  get pages(): readonly PageInfo[] {
    return this.described;
  }

  /// Whether a rotation is running or waiting.
  get busy(): boolean {
    return this.running > 0;
  }

  get canUndo(): boolean {
    return !this.busy && this.past.length > 0;
  }

  get canRedo(): boolean {
    return !this.busy && this.future.length > 0;
  }

  /// Whether the order, or the rotation of a page, differs from the file
  /// as opened.
  get modified(): boolean {
    return (
      !sameOrder(this.current, this.openedOrder) ||
      this.described.some((page, i) => page.rotate !== this.openedPages[i]?.rotate)
    );
  }

  /// Whether page `page` of the file is being turned, or waits to be.
  isTurning(page: number): boolean {
    return this.turning.has(page);
  }

  /// Settles once no rotation runs or waits.
  async idle(): Promise<void> {
    while (this.busy) {
      await this.tail;
    }
  }

  /// Move the pages at `positions` so that they start at `target`, a
  /// position in the order *without* them. Keeps their relative order.
  move(positions: readonly number[], target: number): boolean {
    const moving = new Set(positions);
    const picked = this.current.filter((_, i) => moving.has(i));
    const rest = this.current.filter((_, i) => !moving.has(i));
    const at = Math.max(0, Math.min(target, rest.length));
    return this.reorder([...rest.slice(0, at), ...picked, ...rest.slice(at)]);
  }

  /// Remove the pages at `positions`. Refuses to remove every page.
  remove(positions: readonly number[]): boolean {
    const gone = new Set(positions);
    const next = this.current.filter((_, i) => !gone.has(i));
    if (next.length === 0 || next.length === this.current.length) {
      return false;
    }
    return this.reorder(next);
  }

  /// Turn the pages `pages` of the file by `degrees`, 90 clockwise or -90
  /// counter-clockwise, once the rotations asked for before are done.
  rotate(pages: readonly number[], degrees: number): Promise<Outcome> {
    if (pages.length === 0) {
      return Promise.resolve(NONE);
    }
    const edit: Rotation = { kind: "rotation", pages: [...pages], degrees };
    return this.enqueue(edit, async () => {
      await this.turn(edit.pages, edit.degrees);
      this.past.push(edit);
      this.future = [];
      return { kind: "rotation", pages: edit.pages };
    });
  }

  undo(): Promise<Outcome> {
    const edit = this.past[this.past.length - 1];
    if (edit === undefined || this.busy) {
      return Promise.resolve(NONE);
    }
    if (edit.kind === "order") {
      this.past.pop();
      this.future.push(edit);
      this.current = edit.before;
      return Promise.resolve({ kind: "order" });
    }
    return this.enqueue(edit, async () => {
      await this.turn(edit.pages, -edit.degrees);
      this.past.pop();
      this.future.push(edit);
      return { kind: "rotation", pages: edit.pages };
    });
  }

  redo(): Promise<Outcome> {
    const edit = this.future[this.future.length - 1];
    if (edit === undefined || this.busy) {
      return Promise.resolve(NONE);
    }
    if (edit.kind === "order") {
      this.future.pop();
      this.past.push(edit);
      this.current = edit.after;
      return Promise.resolve({ kind: "order" });
    }
    return this.enqueue(edit, async () => {
      await this.turn(edit.pages, edit.degrees);
      this.future.pop();
      this.past.push(edit);
      return { kind: "rotation", pages: edit.pages };
    });
  }

  /// Record `next` as the new order. Refused when nothing changes, or
  /// while a rotation runs.
  private reorder(next: readonly number[]): boolean {
    if (this.busy || sameOrder(next, this.current)) {
      return false;
    }
    this.past.push({ kind: "order", before: this.current, after: next });
    this.future = [];
    this.current = next;
    return true;
  }

  /// Run `task`, which turns the pages of `rotation`, after the rotations
  /// asked for before. A task that throws changes nothing.
  private enqueue(rotation: Rotation, task: () => Promise<Outcome>): Promise<Outcome> {
    this.running += 1;
    count(this.turning, rotation.pages, 1);
    const run = this.tail
      .then(task)
      .catch(failure)
      .finally(() => {
        this.running -= 1;
        count(this.turning, rotation.pages, -1);
      });
    this.tail = run;
    return run;
  }

  /// Have the Rust side turn `pages` by `degrees`, and take the pages it
  /// answers with.
  private async turn(pages: readonly number[], degrees: number): Promise<void> {
    this.described = await this.rotator([...pages], degrees);
  }
}

function sameOrder(a: readonly number[], b: readonly number[]): boolean {
  return a.length === b.length && a.every((v, i) => v === b[i]);
}

/// Add `delta` to the count of each page of `pages`, forgetting the pages
/// whose count falls to 0.
function count(counts: Map<number, number>, pages: readonly number[], delta: number): void {
  for (const page of pages) {
    const n = (counts.get(page) ?? 0) + delta;
    if (n > 0) {
      counts.set(page, n);
    } else {
      counts.delete(page);
    }
  }
}

function failure(e: unknown): Outcome {
  if (e instanceof Error) {
    return { kind: "failed", message: e.message };
  }
  const error = asAppError(e);
  return { kind: "failed", message: error.kind === "other" ? error.message : "mot de passe refusé" };
}
