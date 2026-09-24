// « Découper… » : la lecture du nombre de pages par fichier, les coupures
// devant les pages sélectionnées, le partage de l'ordre affiché en
// parties, et ce que le bandeau en dit, avant et après.

import type { SplitReport } from "../src/api.js";
import { cutPoints, describeParts, parseEvery, splitAt, splitDone, splitEvery } from "../src/split.js";
import { equal, run, test } from "./check.js";

test("the number of pages per file is a whole number the document can give", () => {
  equal(parseEvery("5", 12), { kind: "ok", every: 5 }, "a number");
  equal(parseEvery("  5  ", 12), { kind: "ok", every: 5 }, "spaces around it");
  equal(parseEvery("1", 12), { kind: "ok", every: 1 }, "one page per file");
  equal(parseEvery("12", 12), { kind: "ok", every: 12 }, "as many as the document has");
  equal(
    parseEvery("", 12),
    { kind: "error", message: "Indiquez combien de pages par fichier." },
    "nothing typed",
  );
  equal(
    parseEvery("0", 12),
    { kind: "error", message: "Il faut au moins une page par fichier." },
    "zero",
  );
  equal(
    parseEvery("abc", 12),
    { kind: "error", message: "« abc » n'est pas un nombre de pages." },
    "not a number",
  );
  equal(parseEvery("-3", 12).kind, "error", "a negative number is not a number of pages");
  equal(parseEvery("2.5", 12).kind, "error", "not a whole number");
  equal(parseEvery("1e3", 12).kind, "error", "not written as a number of pages");
  equal(
    parseEvery("13", 12),
    {
      kind: "error",
      message: "Le document n'a que 12 pages : indiquez un nombre plus petit.",
    },
    "more pages than the document has",
  );
  equal(parseEvery("2", 1).kind, "error", "a document of one page");
  equal(parseEvery("99999999999999999999", 12).kind, "error", "far more than it has");
});

test("the cuts are the pages selected, in order, except the first page", () => {
  equal(cutPoints([3, 1, 6], 8), [1, 3, 6], "sorted");
  equal(cutPoints([2, 2, 2], 8), [2], "counted once");
  equal(cutPoints([0], 8), [], "a cut before the first page would open an empty file");
  equal(cutPoints([0, 4], 8), [4], "the first page dropped, the others kept");
  equal(cutPoints([8, 9], 8), [], "beyond the last page");
  equal(cutPoints([], 8), [], "nothing selected");
});

test("every N pages shares out the order on screen, the last file shorter", () => {
  const order = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
  equal(splitEvery(order, 5), [[0, 1, 2, 3, 4], [5, 6, 7, 8, 9], [10, 11]], "12 pages by 5");
  equal(splitEvery(order, 6), [[0, 1, 2, 3, 4, 5], [6, 7, 8, 9, 10, 11]], "a multiple: no short file");
  equal(splitEvery(order, 12), [order], "one file");
  equal(splitEvery([3, 0], 1), [[3], [0]], "one page each, in the order shown");
  equal(splitEvery([], 5), [], "no page");
  equal(splitEvery([0, 1], 0), [], "no file of no page");
});

test("cutting before the pages selected falls where the grid shows them", () => {
  // Page 4 of the file first, page 2 deleted: the order on screen.
  const order = [3, 0, 4, 1];
  equal(splitAt(order, [2]), [[3, 0], [4, 1]], "one cut, two files");
  equal(splitAt(order, [1, 3]), [[3], [0, 4], [1]], "two cuts, three files");
  equal(splitAt(order, []), [order], "no cut: the document as it stands");
  equal(splitAt([], [1]), [], "no page");
});

test("the banner says what the cut would write, then what it wrote", () => {
  equal(describeParts([]), "Rien à découper.", "nothing");
  equal(describeParts([[0, 1, 2]]), "1 fichier : 3 pages.", "one file");
  equal(describeParts([[0]]), "1 fichier : 1 page.", "one page");
  equal(
    describeParts([[0, 1, 2, 3, 4], [5, 6, 7, 8, 9], [10, 11]]),
    "3 fichiers : 5 + 5 + 2 pages.",
    "12 pages by 5",
  );
  equal(describeParts([[0], [1]]), "2 fichiers : 1 + 1 page.", "one page each");
  equal(
    describeParts(Array.from({ length: 12 }, (_, i) => [i])),
    "12 fichiers : 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + … page.",
    "counted out up to ten",
  );
});

/// What the Rust side answers once the files are written.
function report(dir: string, names: string[]): SplitReport {
  return { dir, files: names.map((name) => ({ name, pages: 2, size: 1024 })) };
}

test("the end of a cut says how many files, and where", () => {
  equal(
    splitDone(report("C:\\docs", ["a_partie-01.pdf", "a_partie-02.pdf", "a_partie-03.pdf"])),
    "3 fichiers écrits dans C:\\docs : de « a_partie-01.pdf » à « a_partie-03.pdf ».",
    "several files",
  );
  equal(
    splitDone(report("C:\\docs", ["a_partie-01.pdf"])),
    "1 fichier écrit dans C:\\docs : « a_partie-01.pdf ».",
    "one file",
  );
});

await run();
