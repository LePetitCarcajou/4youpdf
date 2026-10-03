// What the window says about a merge. Before: the field of each file read,
// what the Rust side is asked for, and the preview of what would be added.
// After: a notice for each file skipped, and for each file merged with a
// caveat; a status line that sums up.

import type { Candidate, CandidateStatus, SourceOutcome, SourceReport } from "../src/api.js";
import { describeMerge, mergeNotices, mergePages, mergeStatus, readRow, rowNote, type Row } from "../src/merge.js";
import { equal, run, test } from "./check.js";

/// A file chosen, as the Rust side describes it once chosen.
function chosen(name: string, status: CandidateStatus): Candidate {
  return { path: `C:\\docs\\${name}`, name, status };
}

function ready(name: string, pages: number): Candidate {
  return chosen(name, { kind: "ready", pages });
}

test("the field of a file takes every page when empty, else the pages typed, in their order", () => {
  equal(readRow(ready("a.pdf", 12), ""), { kind: "all", name: "a.pdf", pages: 12 }, "empty");
  equal(readRow(ready("a.pdf", 12), "  "), { kind: "all", name: "a.pdf", pages: 12 }, "spaces");
  equal(readRow(ready("a.pdf", 12), "8-5"), { kind: "pages", name: "a.pdf", pages: [7, 6, 5, 4] }, "reversed");
  equal(readRow(ready("a.pdf", 12), "1,3"), { kind: "pages", name: "a.pdf", pages: [0, 2] }, "two pages");
  equal(rowNote(readRow(ready("a.pdf", 12), "")), "toutes", "note for every page");
  equal(rowNote(readRow(ready("a.pdf", 12), "8-5")), "4 pages", "note for a list");
  equal(rowNote(readRow(ready("a.pdf", 12), "2")), "1 page", "note for one page");
});

test("a list the file cannot give is refused beside its field, in the words of the command line", () => {
  const cases: [string, string][] = [
    ["13", "page 13 hors limites : le document a 12 pages, numérotées de 1 à 12"],
    ["0", "page 0 hors limites : le document a 12 pages, numérotées de 1 à 12"],
    ["abc", "numéro de page invalide : « abc »"],
    ["3,3", "la page 3 est demandée deux fois"],
  ];
  for (const [typed, message] of cases) {
    const row = readRow(ready("a.pdf", 12), typed);
    equal(row, { kind: "refused", name: "a.pdf", message }, typed);
    equal(rowNote(row), message, `${typed}: note`);
  }
});

test("a file the merge will skip has no field, and says why", () => {
  equal(
    readRow(chosen("verrouillé.pdf", { kind: "protected" }), ""),
    { kind: "skipped", name: "verrouillé.pdf", reason: "protégé par un mot de passe, il sera ignoré" },
    "protected",
  );
  equal(
    rowNote(readRow(chosen("notes.txt", { kind: "refused", message: "en-tête %PDF absent" }), "")),
    "impossible à fusionner (en-tête %PDF absent), il sera ignoré",
    "refused",
  );
});

test("the Rust side is asked for one list per file, null for every page, nothing while a list is refused", () => {
  const rows: Row[] = [
    readRow(ready("a.pdf", 12), "8-5"),
    readRow(ready("b.pdf", 3), ""),
    readRow(chosen("c.pdf", { kind: "protected" }), ""),
  ];
  equal(mergePages(rows), [[7, 6, 5, 4], null, null], "aligned on the files");
  equal(mergePages([...rows, readRow(ready("d.pdf", 2), "3")]), null, "one list refused");
  equal(mergePages([]), [], "no file");
});

test("the banner says how many pages would come, from which files, where, and what the document would hold", () => {
  const a = readRow(ready("A.pdf", 12), "1-3");
  const b = readRow(ready("B.pdf", 11), "");
  equal(
    describeMerge([a, b], 20, null),
    "14 pages ajoutées à la fin : 3 de « A.pdf », 11 de « B.pdf » ; le document en aura 34.",
    "two files, at the end",
  );
  equal(
    describeMerge([a, b], 20, 4),
    "14 pages ajoutées devant la page 5 : 3 de « A.pdf », 11 de « B.pdf » ; le document en aura 34.",
    "in front of a page",
  );
  equal(describeMerge([a], 20, null), "3 pages de « A.pdf » ajoutées à la fin ; le document en aura 23.", "one file");
  equal(
    describeMerge([readRow(ready("A.pdf", 12), "7")], 1, 0),
    "1 page de « A.pdf » ajoutée devant la page 1 ; le document en aura 2.",
    "one page",
  );
  const skipped = readRow(chosen("verrouillé.pdf", { kind: "protected" }), "");
  equal(
    describeMerge([skipped, a], 20, null),
    "3 pages de « A.pdf » ajoutées à la fin ; le document en aura 23.",
    "a file skipped adds nothing",
  );
  const four = ["a", "b", "c", "d"].map((n) => readRow(ready(`${n}.pdf`, 2), ""));
  equal(describeMerge(four, 1, null), "8 pages de 4 fichiers ajoutées à la fin ; le document en aura 9.", "more than three files");
});

test("while a list is refused, or when no file can be merged, the banner says so", () => {
  equal(
    describeMerge([readRow(ready("a.pdf", 12), "13"), readRow(ready("b.pdf", 1), "")], 5, null),
    "Rien n'est fusionné tant qu'une liste de pages est refusée.",
    "a list refused",
  );
  equal(
    describeMerge([readRow(chosen("verrouillé.pdf", { kind: "protected" }), "")], 5, null),
    "Aucun des fichiers choisis ne peut être fusionné.",
    "every file skipped",
  );
  equal(describeMerge([], 5, null), "Aucun des fichiers choisis ne peut être fusionné.", "no file");
});

/// A file as the Rust side reports it.
function source(name: string, outcome: SourceOutcome): SourceReport {
  return { path: `C:\\docs\\${name}`, name, outcome };
}

function merged(name: string, pages: number, extra: Partial<Extract<SourceOutcome, { kind: "merged" }>> = {}): SourceReport {
  return source(name, { kind: "merged", pages, reconstructed: null, encryption: null, ...extra });
}

test("a clean file merged gets no notice, and the status says where its pages came from", () => {
  equal(mergeNotices([merged("annexe.pdf", 3)]), [], "nothing to say");
  equal(mergeStatus([merged("annexe.pdf", 3)]), "3 pages de « annexe.pdf » ajoutées (Ctrl+Z pour annuler).", "several pages");
  equal(mergeStatus([merged("page.pdf", 1)]), "1 page de « page.pdf » ajoutée (Ctrl+Z pour annuler).", "one page");
  equal(
    mergeStatus([merged("a.pdf", 2), merged("b.pdf", 1)]),
    "3 pages de 2 fichiers ajoutées (Ctrl+Z pour annuler).",
    "several files",
  );
});

test("a file merged after a repair, or deciphered, is said so", () => {
  const sources = [
    merged("abîmé.pdf", 2, { reconstructed: "startxref absent" }),
    merged("secret.pdf", 1, { encryption: "révision 3, RC4, clé 128 bits" }),
  ];
  equal(
    mergeNotices(sources),
    [
      [
        "warn",
        "« abîmé.pdf » est endommagé, sa table des objets a été reconstruite par analyse du fichier (startxref absent). " +
          "Ses pages sont fusionnées.",
      ],
      ["warn", "« secret.pdf » est chiffré (révision 3, RC4, clé 128 bits). Ses pages sont fusionnées EN CLAIR, sans protection."],
    ],
    "one notice each",
  );
  equal(mergeStatus(sources), "3 pages de 2 fichiers ajoutées (Ctrl+Z pour annuler).", "both merged");
});

test("a protected or refused file is said so, in its place, and the others merge", () => {
  const sources = [
    source("verrouillé.pdf", { kind: "protected" }),
    merged("annexe.pdf", 4),
    source("notes.txt", { kind: "refused", message: "en-tête %PDF absent" }),
  ];
  equal(
    mergeNotices(sources),
    [
      [
        "warn",
        "« verrouillé.pdf » est protégé par un mot de passe et n'a pas été fusionné. " +
          "Ouvrez-le seul avec son mot de passe, enregistrez-le, puis fusionnez le fichier enregistré.",
      ],
      ["error", "Impossible de fusionner « notes.txt » : en-tête %PDF absent"],
    ],
    "the files skipped, in order",
  );
  equal(
    mergeStatus(sources),
    "4 pages de « annexe.pdf » ajoutées ; 2 fichiers ignorés (Ctrl+Z pour annuler).",
    "the merged file named, the skipped ones counted",
  );
  equal(
    mergeStatus([merged("a.pdf", 1), merged("b.pdf", 1), source("c.pdf", { kind: "protected" })]),
    "2 pages de 2 fichiers ajoutées ; 1 fichier ignoré (Ctrl+Z pour annuler).",
    "one skipped",
  );
});

test("when no file merges, there is nothing to undo", () => {
  equal(mergeStatus([source("verrouillé.pdf", { kind: "protected" })]), "Aucun fichier fusionné.", "one file");
  equal(
    mergeStatus([source("a.pdf", { kind: "protected" }), source("b.pdf", { kind: "refused", message: "vide" })]),
    "Aucun fichier fusionné : les 2 fichiers ont été ignorés.",
    "several files",
  );
  equal(mergeStatus([]), "Aucun fichier fusionné.", "no file at all");
});

await run();
