// The page order of the document being edited, with undo and redo. An
// order is a list of 0-based indices into the pages of the open file;
// deleting a page removes its index, moving one reorders them.

export class OrderHistory {
  private past: number[][] = [];
  private future: number[][] = [];
  private current: number[];

  constructor(pageCount: number) {
    this.current = Array.from({ length: pageCount }, (_, i) => i);
  }

  get order(): readonly number[] {
    return this.current;
  }

  get canUndo(): boolean {
    return this.past.length > 0;
  }

  get canRedo(): boolean {
    return this.future.length > 0;
  }

  /// Whether the order differs from the file as opened.
  get modified(): boolean {
    return this.past.length > 0 && !sameOrder(this.current, this.initial());
  }

  private initial(): number[] {
    return this.past[0] ?? this.current;
  }

  /// Record `next` as the new order. A no-op change is ignored.
  private push(next: number[]): boolean {
    if (sameOrder(next, this.current)) {
      return false;
    }
    this.past.push(this.current);
    this.current = next;
    this.future = [];
    return true;
  }

  /// Move the pages at `positions` so that they start at `target`, a
  /// position in the order *without* them. Keeps their relative order.
  move(positions: readonly number[], target: number): boolean {
    const moving = new Set(positions);
    const picked = this.current.filter((_, i) => moving.has(i));
    const rest = this.current.filter((_, i) => !moving.has(i));
    const at = Math.max(0, Math.min(target, rest.length));
    return this.push([...rest.slice(0, at), ...picked, ...rest.slice(at)]);
  }

  /// Remove the pages at `positions`. Refuses to remove every page.
  remove(positions: readonly number[]): boolean {
    const gone = new Set(positions);
    const next = this.current.filter((_, i) => !gone.has(i));
    if (next.length === 0 || next.length === this.current.length) {
      return false;
    }
    return this.push(next);
  }

  undo(): boolean {
    const previous = this.past.pop();
    if (previous === undefined) {
      return false;
    }
    this.future.push(this.current);
    this.current = previous;
    return true;
  }

  redo(): boolean {
    const next = this.future.pop();
    if (next === undefined) {
      return false;
    }
    this.past.push(this.current);
    this.current = next;
    return true;
  }
}

function sameOrder(a: readonly number[], b: readonly number[]): boolean {
  return a.length === b.length && a.every((v, i) => v === b[i]);
}
