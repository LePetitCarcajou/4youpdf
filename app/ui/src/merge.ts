// What the window says after a merge, as data: `main.ts` shows it, and
// `tests/merge.test.ts` checks it without a DOM. The Rust side reports what
// became of each file asked for (`api.ts`, `SourceReport`): merged, its
// pages after those of the document; protected by a password, which merging
// does not ask for; or refused, not read or not opened even after repair.
// A file skipped gets its own notice, so does a file merged after a repair
// or in the clear, and the status bar sums up.

import type { SourceReport } from "./api.js";
import type { NoticeKind } from "./notices.js";

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
