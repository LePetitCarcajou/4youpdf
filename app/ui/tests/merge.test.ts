// What the window says after a merge: a notice for each file skipped, and
// for each file merged with a caveat; a status line that sums up.

import type { SourceOutcome, SourceReport } from "../src/api.js";
import { mergeNotices, mergeStatus } from "../src/merge.js";
import { equal, run, test } from "./check.js";

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
