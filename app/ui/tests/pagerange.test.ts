// The page lists typed in the window: read as the command line reads them,
// case by case from the file both go through, then with what the window
// adds, an empty field for every page and a page typed twice refused.

import cases from "../../../tests/fixtures/page-ranges.tsv";
import { parsePages, readPages } from "../src/pagerange.js";
import { equal, run, test } from "./check.js";

test("a page list is read as the cases shared with the command line say", () => {
  let read = 0;
  for (const line of cases.split(/\r?\n/)) {
    if (line === "" || line.startsWith("#")) {
      continue;
    }
    const fields = line.split("\t");
    if (fields.length !== 3) {
      throw new Error(`${JSON.stringify(line)}: three fields separated by tabs`);
    }
    const [typed = "", count = "", expected = ""] = fields;
    const wanted = expected.startsWith("! ")
      ? { kind: "error", message: expected.slice(2) }
      : { kind: "ok", pages: expected.split(",").map((n) => Number(n) - 1) };
    equal(parsePages(typed, Number(count)), wanted, JSON.stringify(typed));
    read += 1;
  }
  equal(read, 43, "cases read");
});

test("an empty field takes every page of the file", () => {
  equal(readPages("", 12), { kind: "all" }, "nothing typed");
  equal(readPages("   ", 12), { kind: "all" }, "spaces only");
});

test("a list is taken in the order typed, each page once", () => {
  equal(readPages("1,3,5-8", 12), { kind: "pages", pages: [0, 2, 4, 5, 6, 7] }, "pages and a range");
  equal(readPages("8-5", 12), { kind: "pages", pages: [7, 6, 5, 4] }, "reversed");
  equal(readPages(" 12 ", 12), { kind: "pages", pages: [11] }, "the last page");
});

test("a page typed twice is refused, however it comes twice", () => {
  const twice = (page: number) => ({ kind: "error", message: `la page ${page} est demandée deux fois` });
  equal(readPages("3,3", 12), twice(3), "the same number");
  equal(readPages("1-4,2", 12), twice(2), "within a range");
  equal(readPages("5-1,1-5", 12), twice(1), "two ranges that overlap");
  equal(readPages("007,7", 12), twice(7), "written differently");
});

test("what the command line refuses, the field refuses in the same words", () => {
  equal(
    readPages("13", 12),
    { kind: "error", message: "page 13 hors limites : le document a 12 pages, numérotées de 1 à 12" },
    "beyond the file",
  );
  equal(readPages("0", 12).kind, "error", "zero");
  equal(readPages("abc", 12), { kind: "error", message: "numéro de page invalide : « abc »" }, "not a number");
  equal(readPages(",", 12), { kind: "error", message: "aucune page indiquée (exemple : 1,3,5-8)" }, "commas only");
});

await run();
