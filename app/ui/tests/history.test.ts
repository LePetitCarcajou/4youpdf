// The history of the pages: moves and deletions are undone here; a
// rotation is made and undone by the Rust side, one at a time, and the
// other edits wait for it.

import type { AppError, PageInfo } from "../src/api.js";
import { PageHistory, type Rotator } from "../src/history.js";
import { equal, run, test } from "./check.js";

/// A4 pages, turned as given.
function pages(...rotations: number[]): PageInfo[] {
  return rotations.map((rotate) => ({ width: 595, height: 842, rotate }));
}

/// A Rust side that answers when the test says so, and remembers what it
/// was asked.
class RustSide {
  readonly calls: [number[], number][] = [];
  private waiting: ((answer: PageInfo[] | AppError) => void)[] = [];

  readonly rotator: Rotator = (turned, degrees) => {
    this.calls.push([turned, degrees]);
    return new Promise((resolve, reject) => {
      this.waiting.push((answer) => (Array.isArray(answer) ? resolve(answer) : reject(answer)));
    });
  };

  /// Answer the oldest question.
  answer(answer: PageInfo[] | AppError): void {
    this.waiting.shift()?.(answer);
  }
}

/// Let the promises already settled run their continuations.
async function settle(): Promise<void> {
  for (let i = 0; i < 20; i += 1) {
    await Promise.resolve();
  }
}

test("moves and deletions are undone and redone without the Rust side", async () => {
  const rust = new RustSide();
  const history = new PageHistory(pages(0, 0, 0, 0), rust.rotator);
  equal(history.move([0], 3), true, "move");
  equal(history.order, [1, 2, 3, 0], "order after the move");
  equal(history.remove([1, 2]), true, "delete");
  equal(history.order, [1, 0], "order after the deletion");
  equal(history.remove([0, 1]), false, "the last page stays");
  equal(history.modified, true, "modified");

  equal(await history.undo(), { kind: "order" }, "undo the deletion");
  equal(await history.undo(), { kind: "order" }, "undo the move");
  equal([history.order, history.modified], [[0, 1, 2, 3], false], "as opened");
  equal(await history.undo(), { kind: "none" }, "nothing more to undo");
  equal(await history.redo(), { kind: "order" }, "redo the move");
  equal(history.order, [1, 2, 3, 0], "order redone");
  equal(rust.calls, [], "the Rust side is not asked");
});

test("a rotation is made, undone and redone by the Rust side", async () => {
  const rust = new RustSide();
  const history = new PageHistory(pages(0, 90, 0), rust.rotator);

  const turned = history.rotate([0, 1], 90);
  equal(history.busy, true, "busy while the Rust side works");
  equal([0, 1, 2].map((page) => history.isTurning(page)), [true, true, false], "pages being turned");
  await settle();
  rust.answer(pages(90, 180, 0));
  equal(await turned, { kind: "rotation", pages: [0, 1] }, "outcome");
  equal(history.pages, pages(90, 180, 0), "pages as the Rust side describes them");
  equal([history.busy, history.isTurning(0), history.modified], [false, false, true], "done");

  const undone = history.undo();
  await settle();
  rust.answer(pages(0, 90, 0));
  equal(await undone, { kind: "rotation", pages: [0, 1] }, "undo");
  equal([history.pages, history.modified], [pages(0, 90, 0), false], "as opened");

  const redone = history.redo();
  await settle();
  rust.answer(pages(90, 180, 0));
  equal(await redone, { kind: "rotation", pages: [0, 1] }, "redo");
  equal(
    rust.calls,
    [
      [[0, 1], 90],
      [[0, 1], -90],
      [[0, 1], 90],
    ],
    "asked of the Rust side: the rotation, its opposite, the rotation",
  );
  equal(history.order, [0, 1, 2], "order untouched");
});

test("rotations run one after the other, and the other edits wait for them", async () => {
  const rust = new RustSide();
  const history = new PageHistory(pages(0, 0, 0), rust.rotator);
  equal(history.move([2], 0), true, "a move first");

  const first = history.rotate([0], 90);
  const second = history.rotate([2], -90);
  await settle();
  equal(rust.calls.length, 1, "the second rotation waits for the first");
  equal(
    [history.move([0], 2), history.remove([0]), history.canUndo, history.canRedo],
    [false, false, false, false],
    "moving, deleting, undoing and redoing refused meanwhile",
  );
  equal(await history.undo(), { kind: "none" }, "undo refused meanwhile");

  let idle = false;
  const waiting = history.idle().then(() => {
    idle = true;
  });
  rust.answer(pages(90, 0, 0));
  await first;
  await settle();
  equal([rust.calls.length, history.busy, idle], [2, true, false], "the second rotation runs");
  rust.answer(pages(90, 0, 270));
  equal(await second, { kind: "rotation", pages: [2] }, "second outcome");
  await waiting;
  equal([idle, history.busy, history.canUndo], [true, false, true], "idle");

  // Undone in the reverse order: the second rotation, the first, the move.
  const undoSecond = history.undo();
  await settle();
  equal(rust.calls[2], [[2], 90], "the second rotation is undone first");
  rust.answer(pages(90, 0, 0));
  await undoSecond;
  const undoFirst = history.undo();
  await settle();
  equal(rust.calls[3], [[0], -90], "then the first");
  rust.answer(pages(0, 0, 0));
  await undoFirst;
  equal(await history.undo(), { kind: "order" }, "then the move");
  equal([history.order, history.modified], [[0, 1, 2], false], "as opened");
});

test("a rotation the Rust side refuses changes nothing", async () => {
  const rust = new RustSide();
  const history = new PageHistory(pages(0, 0), rust.rotator);

  const refused = history.rotate([1], 90);
  await settle();
  rust.answer({ kind: "other", message: "page 1 : objet illisible" });
  equal(await refused, { kind: "failed", message: "page 1 : objet illisible" }, "outcome");
  equal(
    [history.pages, history.canUndo, history.busy, history.isTurning(1)],
    [pages(0, 0), false, false, false],
    "nothing changed",
  );

  // An undo refused by the Rust side leaves the rotation to undo.
  const turned = history.rotate([0], 90);
  await settle();
  rust.answer(pages(90, 0));
  await turned;
  const undone = history.undo();
  await settle();
  rust.answer({ kind: "other", message: "mémoire insuffisante" });
  equal(await undone, { kind: "failed", message: "mémoire insuffisante" }, "undo refused");
  equal([history.pages, history.canUndo, history.canRedo], [pages(90, 0), true, false], "still to undo");
});

test("the document is modified when what it would save is not on disk, however it got there", async () => {
  const rust = new RustSide();
  const history = new PageHistory(pages(0, 0, 0), rust.rotator);
  equal([history.modified, history.unsaved], [false, false], "as opened");

  // Undone back to the file as opened: intact again.
  history.move([0], 2);
  equal([history.modified, history.unsaved], [true, true], "moved");
  await history.undo();
  equal([history.modified, history.unsaved], [false, false], "undone");

  // Turned back by the opposite rotation rather than by undo: the Rust
  // side rewrote the document twice, the pages are those of the file.
  let turned = history.rotate([1], 90);
  await settle();
  rust.answer(pages(0, 90, 0));
  await turned;
  equal(history.modified, true, "turned");
  turned = history.rotate([1], -90);
  await settle();
  rust.answer(pages(0, 0, 0));
  await turned;
  equal([history.modified, history.canUndo], [false, true], "turned back: intact, with two rotations to undo");

  // Not by way of undo either: an edit that brings the same pages back.
  history.remove([2]);
  equal(history.order, [0, 1], "deleted");
  await history.undo();
  history.move([2], 2);
  equal([history.order, history.modified], [[0, 1, 2], false], "a move that moves nothing is not an edit");
});

test("a rotation still running counts as unsaved, its result being unknown", async () => {
  const rust = new RustSide();
  const history = new PageHistory(pages(0, 0), rust.rotator);
  const turned = history.rotate([0], 90);
  equal([history.modified, history.unsaved], [false, true], "asked for, not answered");
  await settle();
  rust.answer(pages(90, 0));
  await turned;
  equal([history.modified, history.unsaved], [true, true], "answered");

  // Undoing it runs too: unsaved until the Rust side answers.
  const undone = history.undo();
  equal(history.unsaved, true, "undo running");
  await settle();
  rust.answer(pages(0, 0));
  await undone;
  equal([history.modified, history.unsaved], [false, false], "undone");

  // A refused rotation leaves the document as it was: intact.
  const refused = history.rotate([1], 90);
  await settle();
  rust.answer({ kind: "other", message: "page 1 : objet illisible" });
  await refused;
  equal([history.modified, history.unsaved], [false, false], "refused");
});

test("a merge appends the pages of other files, undone and redone like a move", async () => {
  const rust = new RustSide();
  const history = new PageHistory(pages(0, 0), rust.rotator);
  history.move([0], 2);
  const merged = history.merge(() => Promise.resolve(pages(0, 0, 90, 0)));
  equal([history.busy, history.unsaved], [true, true], "busy while the Rust side works");
  equal(await merged, { kind: "merged", added: [2, 3], at: 2 }, "outcome");
  equal(
    [history.order, history.pages, history.busy, history.modified],
    [[1, 0, 2, 3], pages(0, 0, 90, 0), false, true],
    "appended at the end, the pages as the Rust side describes them",
  );
  equal([0, 1, 2, 3].map((page) => history.ofFile(page)), [true, true, false, false], "the merged pages are not pages of the file");

  equal(await history.undo(), { kind: "order" }, "undo the merge");
  equal([history.order, history.pages.length, history.modified], [[1, 0], 4, true], "out of the order, still in the file");
  equal(await history.undo(), { kind: "order" }, "undo the move");
  equal([history.order, history.modified], [[0, 1], false], "as opened, whatever the file holds beyond");
  equal(await history.redo(), { kind: "order" }, "redo the move");
  equal(await history.redo(), { kind: "order" }, "redo the merge");
  equal(history.order, [1, 0, 2, 3], "merged again");
  equal(rust.calls, [], "the Rust side is not asked to undo or redo a merge");

  // Saving takes the merged pages: back before the merge differs from the
  // file written.
  history.saved(history.toSave);
  equal(history.modified, false, "saved");
  await history.undo();
  equal(history.modified, true, "before the merge");
});

test("a merge at a position puts the pages there, and out of range means the ends", async () => {
  const rust = new RustSide();
  const history = new PageHistory(pages(0, 0, 0), rust.rotator);
  history.move([0], 3);
  equal(await history.merge(() => Promise.resolve(pages(0, 0, 0, 0)), 1), { kind: "merged", added: [3], at: 1 }, "at 1");
  equal(history.order, [1, 3, 2, 0], "in front of the page at position 1");
  equal(await history.undo(), { kind: "order" }, "undone");
  equal(history.order, [1, 2, 0], "as before the merge");
  equal(await history.merge(() => Promise.resolve(pages(0, 0, 0, 0, 0, 0)), -4), { kind: "merged", added: [4, 5], at: 0 }, "below 0");
  equal(history.order, [4, 5, 1, 2, 0], "at the start");
  equal(await history.merge(() => Promise.resolve(pages(0, 0, 0, 0, 0, 0, 0)), 99), { kind: "merged", added: [6], at: 5 }, "past the end");
  equal(history.order, [4, 5, 1, 2, 0, 6], "at the end");
});

test("a merge that brings nothing, or that the Rust side refuses, is not an edit", async () => {
  const rust = new RustSide();
  const history = new PageHistory(pages(0), rust.rotator);
  equal(await history.merge(() => Promise.resolve(null)), { kind: "none" }, "no file merged");
  equal(await history.merge(() => Promise.resolve(pages(0))), { kind: "none" }, "no page added");
  const refused: AppError = { kind: "other", message: "disque plein" };
  equal(await history.merge(() => Promise.reject(refused)), { kind: "failed", message: "disque plein" }, "refused");
  equal(
    [history.order, history.pages, history.canUndo, history.modified, history.busy],
    [[0], pages(0), false, false, false],
    "nothing changed",
  );
});

test("a merge takes its turn among the rotations", async () => {
  const rust = new RustSide();
  const history = new PageHistory(pages(0), rust.rotator);
  const turned = history.rotate([0], 90);
  let asked = false;
  let answer: (pages: PageInfo[]) => void = () => {};
  const merged = history.merge(() => {
    asked = true;
    return new Promise((resolve) => {
      answer = resolve;
    });
  });
  const after = history.rotate([0], 90);
  await settle();
  equal([rust.calls.length, asked], [1, false], "the merge waits for the rotation before it");
  rust.answer(pages(90));
  await turned;
  await settle();
  equal([asked, rust.calls.length, history.busy], [true, 1, true], "the merge runs, the rotation after it waits");
  answer(pages(90, 0));
  equal(await merged, { kind: "merged", added: [1], at: 1 }, "merged");
  await settle();
  equal(rust.calls.length, 2, "then the rotation");
  rust.answer(pages(180, 0));
  await after;
  equal([history.order, history.pages, history.busy], [[0, 1], pages(180, 0), false], "done in order");
});

test("saving makes the document intact, until it differs from the file written", async () => {
  const rust = new RustSide();
  const history = new PageHistory(pages(0, 0, 0), rust.rotator);
  history.move([0], 2);
  const turned = history.rotate([1], 90);
  await settle();
  rust.answer(pages(0, 90, 0));
  await turned;
  equal(history.unsaved, true, "edited");

  const point = history.toSave;
  equal(point, { order: [1, 2, 0], pages: pages(0, 90, 0) }, "what saving writes");
  history.saved(point);
  equal([history.modified, history.unsaved, history.canUndo], [false, false, true], "saved: intact, history kept");

  // An edit after saving, or undoing one made before, differs from the file.
  history.remove([0]);
  equal(history.modified, true, "edited after saving");
  await history.undo();
  equal(history.modified, false, "back to the file written");
  const undone = history.undo();
  await settle();
  rust.answer(pages(0, 0, 0));
  await undone;
  equal([history.pages, history.modified], [pages(0, 0, 0), true], "undone past the file written");
  const redone = history.redo();
  await settle();
  rust.answer(pages(0, 90, 0));
  await redone;
  equal(history.modified, false, "redone up to it");

  // What is written is what `toSave` gave, not what came after: an edit
  // made while the file was being written keeps the document modified.
  const written = history.toSave;
  history.move([0], 1);
  history.saved(written);
  equal([history.order, history.modified], [[2, 1, 0], true], "edited while writing");
});

await run();
