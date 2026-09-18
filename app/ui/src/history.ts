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
//
// Merging other files takes the same round trip: the Rust side appends
// their pages to the document through `ops::merge` and answers with every
// page as it now stands. The pages beyond those known are new indices,
// added at the end of the order as an edit like a move: undoing it takes
// them out of the order, where saving leaves them out, and the file keeps
// them for a redo. A merge runs in its turn among the rotations.
//
// Whether the document carries unsaved changes is decided here too, by
// what saving would write, not by what was done: the order and the
// rotations are compared with those of the file as opened, or as last
// saved. Undoing back to that state, or turning a page back by the
// opposite rotation, leaves nothing to save.

import { asAppError, type PageInfo } from "./api.js";

/// Turns the pages at the given indices of the open file by a multiple of
/// 90 degrees, clockwise when positive, and answers with every page as it
/// now stands: the `rotate_pages` command.
export type Rotator = (pages: number[], degrees: number) => Promise<PageInfo[]>;

/// Appends the pages of other files to the open file and answers with every
/// page as it now stands, or `null` when no file could be merged: the
/// `merge_documents` command, with the files already chosen.
export type Merger = () => Promise<PageInfo[] | null>;

/// How an undo, a redo, a rotation or a merge ended: the order changed; the
/// pages `pages` of the file turned; the pages `added` were appended to
/// the file and put in the order from position `at`; nothing to do, or
/// refused while a rotation runs; refused by the Rust side, in which case
/// nothing changed.
export type Outcome =
  | { kind: "order" }
  | { kind: "rotation"; pages: readonly number[] }
  | { kind: "merged"; added: readonly number[]; at: number }
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

/// What saving writes: the order of the pages, and the pages as the Rust
/// side describes them, the rotations included. The document is intact as
/// long as it matches the last one written, or the file as opened.
export interface SavePoint {
  readonly order: readonly number[];
  readonly pages: readonly PageInfo[];
}

const NONE: Outcome = { kind: "none" };

export class PageHistory {
  private past: Edit[] = [];
  private future: Edit[] = [];
  private current: readonly number[];
  private described: readonly PageInfo[];
  /// The file as opened, or as last saved.
  private reference: SavePoint;
  private readonly rotator: Rotator;
  /// How many pages the file had as opened: the indices from there on
  /// were appended by merges.
  private readonly opened: number;
  /// Rotations running or waiting, and how many of them turn each page.
  private running = 0;
  private turning = new Map<number, number>();
  private tail: Promise<unknown> = Promise.resolve();

  constructor(pages: readonly PageInfo[], rotator: Rotator) {
    this.current = pages.map((_, i) => i);
    this.described = pages;
    this.reference = { order: this.current, pages };
    this.rotator = rotator;
    this.opened = pages.length;
  }

  /// Whether page `page` is a page of the file as opened, rather than one
  /// a merge appended: `page + 1` is then its number in the file.
  ofFile(page: number): boolean {
    return page < this.opened;
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

  /// Whether the order, or the rotation of a page in it, differs from the
  /// file as opened or as last saved (`saved`): what saving now would write
  /// is not on disk. How it came to differ does not count: undone back to
  /// that state, or turned back by the opposite rotation, the document is
  /// intact again, whatever the Rust side rewrote meanwhile; and a page out
  /// of the order, deleted or merged then undone, counts for nothing.
  get modified(): boolean {
    return (
      !sameOrder(this.current, this.reference.order) ||
      this.current.some((page) => this.described[page]?.rotate !== this.reference.pages[page]?.rotate)
    );
  }

  /// Whether closing, or opening another file, would lose work: the
  /// document is modified, or a rotation still runs, whose result is not
  /// known yet.
  get unsaved(): boolean {
    return this.busy || this.modified;
  }

  /// What saving now would write.
  get toSave(): SavePoint {
    return { order: this.current, pages: this.described };
  }

  /// `point`, taken from `toSave`, was written to a file: the document is
  /// intact until it differs from it again, by an edit or by undoing one
  /// made before.
  saved(point: SavePoint): void {
    this.reference = point;
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
    return this.enqueue(edit.pages, async () => {
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
    return this.enqueue(edit.pages, async () => {
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
    return this.enqueue(edit.pages, async () => {
      await this.turn(edit.pages, edit.degrees);
      this.future.pop();
      this.past.push(edit);
      return { kind: "rotation", pages: edit.pages };
    });
  }

  /// Append the pages of other files to the document through `merger`,
  /// once the rotations asked for before are done: the pages it answers
  /// with beyond those known go into the order from position `at`, the end
  /// when not given, as one edit undone and redone like a move. Nothing is
  /// recorded when no file could be merged, or when the Rust side refused.
  merge(merger: Merger, at?: number): Promise<Outcome> {
    return this.enqueue([], async () => {
      const pages = await merger();
      if (pages === null || pages.length <= this.described.length) {
        return NONE;
      }
      const added = pages.slice(this.described.length).map((_, i) => this.described.length + i);
      this.described = pages;
      const position = Math.max(0, Math.min(at ?? this.current.length, this.current.length));
      const after = [...this.current.slice(0, position), ...added, ...this.current.slice(position)];
      this.past.push({ kind: "order", before: this.current, after });
      this.future = [];
      this.current = after;
      return { kind: "merged", added, at: position };
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

  /// Run `task`, a round trip that turns the pages `turning` (none for a
  /// merge), after the round trips asked for before. A task that throws
  /// changes nothing.
  private enqueue(turning: readonly number[], task: () => Promise<Outcome>): Promise<Outcome> {
    this.running += 1;
    count(this.turning, turning, 1);
    const run = this.tail
      .then(task)
      .catch(failure)
      .finally(() => {
        this.running -= 1;
        count(this.turning, turning, -1);
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
