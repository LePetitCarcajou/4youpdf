// What the window says about a merge, as data: `main.ts` shows it, and
// `tests/merge.test.ts` checks it without a DOM.
//
// Before: once the files are chosen, a banner asks which pages of each to
// take, one field per file (`pagerange.ts`: `1,3,5-8`, `8-5` for the
// reverse order, nothing for every page), and says what the merge would add
// and where. The Rust side said, when the files were chosen, how many pages
// each holds or why it will be skipped (`api.ts`, `Candidate`).
//
// After: the Rust side reports what became of each file asked for (`api.ts`,
// `SourceReport`): merged, the pages chosen after those of the document;
// protected by a password, which merging does not ask for; or refused, not
// read or not opened even after repair. A file skipped gets its own notice,
// so does a file merged after a repair or in the clear, and the status bar
// sums up.

import type { Candidate, SourceReport } from "./api.js";
import type { NoticeKind } from "./notices.js";
import { readPages } from "./pagerange.js";

/// A file of the banner, once its field is read: every page, `pages` of
/// them; these pages, 0-based, in this order; a list the file cannot give,
/// and why; or a file the merge will skip, and why.
export type Row =
  | { kind: "all"; name: string; pages: number }
  | { kind: "pages"; name: string; pages: number[] }
  | { kind: "refused"; name: string; message: string }
  | { kind: "skipped"; name: string; reason: string };

/// Read the field of `candidate`, where `typed` is what it holds (nothing
/// for a file the merge will skip, which has no field).
export function readRow(candidate: Candidate, typed: string): Row {
  const name = candidate.name;
  const status = candidate.status;
  switch (status.kind) {
    case "ready": {
      const read = readPages(typed, status.pages);
      if (read.kind === "all") {
        return { kind: "all", name, pages: status.pages };
      }
      return read.kind === "pages" ? { kind: "pages", name, pages: read.pages } : { kind: "refused", name, message: read.message };
    }
    case "protected":
      return { kind: "skipped", name, reason: "protégé par un mot de passe, il sera ignoré" };
    case "refused":
      return { kind: "skipped", name, reason: `impossible à fusionner (${status.message}), il sera ignoré` };
    default:
      return { kind: "skipped", name, reason: "il sera ignoré" };
  }
}

/// What a row says beside its field: what it takes, or why not.
export function rowNote(row: Row): string {
  switch (row.kind) {
    case "all":
      return "toutes";
    case "pages":
      return `${row.pages.length} page${row.pages.length > 1 ? "s" : ""}`;
    case "refused":
      return row.message;
    case "skipped":
      return row.reason;
    default:
      return "";
  }
}

/// The lists of pages to ask the Rust side for, one per file in the order
/// of the rows: `null` for every page, and for a file the merge will skip,
/// which the Rust side finds out again. `null` altogether while a row is
/// refused: nothing is asked then.
export function mergePages(rows: readonly Row[]): (number[] | null)[] | null {
  if (rows.some((row) => row.kind === "refused")) {
    return null;
  }
  return rows.map((row) => (row.kind === "pages" ? row.pages : null));
}

/// Where the pages merged would go in `order`, the pages of the grid as it
/// now stands: the position of `before`, the page of the file they go in
/// front of; `null` for the end of the grid; `undefined` when that page has
/// left the grid since it was meant.
export function mergePosition(order: readonly number[], before: number | null): number | null | undefined {
  if (before === null) {
    return null;
  }
  const position = order.indexOf(before);
  return position < 0 ? undefined : position;
}

/// What the banner says the merge would do, before it runs (ADR 0004, point
/// 6): how many pages, from which files, where (`at`, the position the
/// first of them would take, `null` for the end of a document of
/// `documentPages` pages, `undefined` when the page they would go in front
/// of has left the grid), and how many pages the document would then have.
/// Three files at most are named, the others counted.
export function describeMerge(rows: readonly Row[], documentPages: number, at: number | null | undefined): string {
  if (at === undefined) {
    return "La page devant laquelle fusionner n'est plus dans le document.";
  }
  if (rows.some((row) => row.kind === "refused")) {
    return "Rien n'est fusionné tant qu'une liste de pages est refusée.";
  }
  const taken: { name: string; count: number }[] = [];
  for (const row of rows) {
    if (row.kind === "all") {
      taken.push({ name: row.name, count: row.pages });
    } else if (row.kind === "pages") {
      taken.push({ name: row.name, count: row.pages.length });
    }
  }
  if (taken.length === 0) {
    return "Aucun des fichiers choisis ne peut être fusionné.";
  }
  const total = taken.reduce((sum, file) => sum + file.count, 0);
  const plural = total > 1 ? "s" : "";
  const where = at === null ? "à la fin" : `devant la page ${at + 1}`;
  const after = `le document en aura ${documentPages + total}.`;
  const [only] = taken;
  if (taken.length === 1 && only !== undefined) {
    return `${total} page${plural} de « ${only.name} » ajoutée${plural} ${where} ; ${after}`;
  }
  if (taken.length > 3) {
    return `${total} pages de ${taken.length} fichiers ajoutées ${where} ; ${after}`;
  }
  const parts = taken.map((file) => `${file.count} de « ${file.name} »`);
  return `${total} pages ajoutées ${where} : ${parts.join(", ")} ; ${after}`;
}

/// What asking for the merge comes to: the keyboard to the first row
/// refused, which says why beside its field; a refusal said in the banner,
/// when no file can give a page or the page meant has left the grid; or the
/// lists to ask the Rust side for, and the position the first page merged
/// would take, `null` for the end.
export type MergePlan =
  | { kind: "refused-row"; row: number }
  | { kind: "refused"; message: string }
  | { kind: "merge"; pages: (number[] | null)[]; at: number | null };

/// What asking for the merge comes to, from the rows and from `at`, where
/// the pages would go (`mergePosition`). The rows are looked at first: a
/// list refused is corrected before anything else is said.
export function planMerge(rows: readonly Row[], at: number | null | undefined): MergePlan {
  const pages = mergePages(rows);
  if (pages === null) {
    return { kind: "refused-row", row: rows.findIndex((row) => row.kind === "refused") };
  }
  if (!rows.some((row) => row.kind === "all" || row.kind === "pages")) {
    return { kind: "refused", message: "Aucun des fichiers choisis ne peut être fusionné : il n'y a rien à ajouter." };
  }
  if (at === undefined) {
    return {
      kind: "refused",
      message: "La page devant laquelle fusionner a été supprimée : relancez « Fusionner ici… » sur une autre page.",
    };
  }
  return { kind: "merge", pages, at };
}

/// One notice per file that deserves one, in the order of the files: a
/// file skipped, with why; a file merged after being repaired, or
/// deciphered, on the way in. A clean file merged needs none.
export function mergeNotices(sources: readonly SourceReport[]): [NoticeKind, string][] {
  const notices: [NoticeKind, string][] = [];
  for (const source of sources) {
    const name = `« ${source.name} »`;
    const outcome = source.outcome;
    switch (outcome.kind) {
      case "merged":
        if (outcome.reconstructed !== null) {
          notices.push([
            "warn",
            `${name} est endommagé, sa table des objets a été reconstruite par analyse du fichier (${outcome.reconstructed}). ` +
              "Ses pages sont fusionnées.",
          ]);
        }
        if (outcome.encryption !== null) {
          notices.push([
            "warn",
            `${name} est chiffré (${outcome.encryption}). Ses pages sont fusionnées EN CLAIR, sans protection.`,
          ]);
        }
        break;
      case "protected":
        notices.push([
          "warn",
          `${name} est protégé par un mot de passe et n'a pas été fusionné. ` +
            "Ouvrez-le seul avec son mot de passe, enregistrez-le, puis fusionnez le fichier enregistré.",
        ]);
        break;
      case "refused":
        notices.push(["error", `Impossible de fusionner ${name} : ${outcome.message}`]);
        break;
      default:
        break;
    }
  }
  return notices;
}

/// The status bar after a merge: how many pages came, from which file or
/// how many, how many files were left out, and how to undo. When no file
/// merged, that there is nothing to undo.
export function mergeStatus(sources: readonly SourceReport[]): string {
  const merged = sources.filter((s) => s.outcome.kind === "merged");
  const skipped = sources.length - merged.length;
  if (merged.length === 0) {
    return skipped > 1 ? `Aucun fichier fusionné : les ${skipped} fichiers ont été ignorés.` : "Aucun fichier fusionné.";
  }
  let pages = 0;
  for (const source of merged) {
    if (source.outcome.kind === "merged") {
      pages += source.outcome.pages;
    }
  }
  const plural = pages > 1 ? "s" : "";
  const from = merged.length === 1 ? `de « ${merged[0]?.name ?? ""} »` : `de ${merged.length} fichiers`;
  const left = skipped === 0 ? "" : ` ; ${skipped} fichier${skipped > 1 ? "s ignorés" : " ignoré"}`;
  return `${pages} page${plural} ${from} ajoutée${plural}${left} (Ctrl+Z pour annuler).`;
}
