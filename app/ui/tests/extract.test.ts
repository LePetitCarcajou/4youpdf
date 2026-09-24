// « Extraire la sélection… » : quand l'action est disponible, quelles
// pages elle écrit, et sous quel nom le sélecteur les propose.

import { canExtract, extractName, selectedPages } from "../src/extract.js";
import { equal, run, test } from "./check.js";

test("extracting needs a document, the grid in charge, and a selection", () => {
  const grid = { hasDocument: true, viewerOpen: false, selected: 2 };
  equal(canExtract(grid), true, "pages selected in the grid");
  equal(canExtract({ ...grid, selected: 1 }), true, "one page is enough");
  equal(canExtract({ ...grid, selected: 0 }), false, "nothing selected: unavailable, not an error");
  equal(canExtract({ ...grid, viewerOpen: true }), false, "the page view edits nothing");
  equal(canExtract({ ...grid, hasDocument: false }), false, "no document");
});

test("the pages extracted are those of the selection, in the order of the grid", () => {
  // Page 3 of the file first, page 1 deleted: the order on screen.
  const order = [2, 0, 3, 1];
  equal(selectedPages(order, [0, 2]), [2, 3], "positions read as places in the order");
  equal(selectedPages(order, [2, 0]), [2, 3], "selected backwards: written in the order shown");
  equal(selectedPages(order, [3]), [1], "one page");
  equal(selectedPages(order, [1, 1, 1]), [0], "a position counted once");
  equal(selectedPages(order, []), [], "nothing selected");
  equal(selectedPages(order, [7]), [], "a position the order has not");
  equal(selectedPages([], [0]), [], "no page at all");
});

test("the name suggested is that of the document, followed by -extrait", () => {
  equal(extractName("rapport.pdf"), "rapport-extrait.pdf", "the extension dropped");
  equal(extractName("RAPPORT.PDF"), "RAPPORT-extrait.pdf", "whatever its case");
  equal(extractName("rapport.final.pdf"), "rapport.final-extrait.pdf", "the last extension only");
  equal(extractName("sans-extension"), "sans-extension-extrait.pdf", "no extension to drop");
  equal(extractName("été 2026.pdf"), "été 2026-extrait.pdf", "accents and spaces kept");
  equal(extractName(""), "document-extrait.pdf", "nothing left to name it");
});

await run();
