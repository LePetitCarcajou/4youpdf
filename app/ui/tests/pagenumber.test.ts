// The page numbers of the page view: the number typed is a position in the
// order, the one the caption gives first, not the number of a page in the
// file; a number that names no page goes nowhere and says what is expected.

import type { PageInfo } from "../src/api.js";
import { PageHistory } from "../src/history.js";
import { pageCaption, pageNumberHelp, pageNumberRefusal, readPageNumber } from "../src/pagenumber.js";
import { equal, run, test } from "./check.js";

/// `count` A4 pages.
function pages(count: number): PageInfo[] {
  return Array.from({ length: count }, () => ({ width: 595, height: 842, rotate: 0 }));
}

test("a number is a position in the order, from 1", () => {
  equal(readPageNumber("1", 12), { kind: "page", position: 0 }, "first page");
  equal(readPageNumber("12", 12), { kind: "page", position: 11 }, "last page");
  equal(readPageNumber(" 7 ", 12), { kind: "page", position: 6 }, "spaces around");
  equal(readPageNumber("007", 12), { kind: "page", position: 6 }, "leading zeros");
});

test("a number that names no page is out of range, however long", () => {
  for (const text of ["0", "000", "13", "120", "99999999999999999999999"]) {
    equal(readPageNumber(text, 12), { kind: "out_of_range" }, `« ${text} »`);
  }
  equal(readPageNumber("2", 1), { kind: "out_of_range" }, "« 2 » in a single page");
});

test("anything but the digits 0 to 9 is not a number", () => {
  for (const text of ["", "   ", "abc", "3.5", "3,5", "-2", "+2", "1e1", "1 2", "３"]) {
    equal(readPageNumber(text, 12), { kind: "not_a_number" }, `« ${text} »`);
  }
});

test("after moves and deletions, the number is a position, not a page of the file", () => {
  const history = new PageHistory(pages(5), () => Promise.resolve([]));
  equal(history.move([4], 0), true, "the last page moved first");
  equal(history.remove([2]), true, "the page at position 3 deleted");
  equal(history.order, [4, 0, 2, 3], "order");
  const count = history.order.length;

  equal(readPageNumber("1", count), { kind: "page", position: 0 }, "« 1 » is the first page of the order");
  equal(
    pageCaption(0, history.order[0] ?? -1, count),
    { after: "sur 4 (page 5 du fichier)", spoken: "Page 1 sur 4 (page 5 du fichier)" },
    "whose caption gives its page in the file second",
  );
  equal(readPageNumber("5", count), { kind: "out_of_range" }, "« 5 » names no page, though page 5 of the file is there");
  equal(
    pageCaption(2, history.order[2] ?? -1, count),
    { after: "sur 4", spoken: "Page 3 sur 4" },
    "a page at its place in the file has a single number",
  );
});

test("while a number is typed, the view says it is a position in the order, and why it went nowhere", () => {
  equal(pageNumberHelp(12), "Numéro dans l'ordre actuel (1 à 12) · Entrée : y aller · Échap : annuler", "help");
  equal(
    pageNumberRefusal({ kind: "out_of_range" }, 12),
    "Aucune page ne porte ce numéro dans l'ordre actuel (1 à 12)",
    "out of range",
  );
  equal(pageNumberRefusal({ kind: "not_a_number" }, 12), "Tapez un numéro dans l'ordre actuel (1 à 12)", "not a number");
  equal(pageNumberHelp(1), "Numéro dans l'ordre actuel (1) · Entrée : y aller · Échap : annuler", "a single page");
});

await run();
