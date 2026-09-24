// « Découper… » : le document partagé en plusieurs fichiers, écrits dans
// un dossier choisi. Ce que cela demande sans DOM est ici, et
// `tests/split.test.ts` le vérifie ; `main.ts` dessine le bandeau de
// saisie (jamais une boîte modale, ADR 0004) et appelle `split_document`.
//
// Les parties sont des tranches de l'ordre affiché, pas des plages du
// fichier : une page déplacée, supprimée ou tournée va là où la grille la
// montre. `ops::split` du noyau, lui, prend des plages du fichier ; le
// côté Rust reçoit donc une liste d'indices de pages par partie et les
// écrit une à une par `ops::extract_pages` (`session.rs`, `Session::split`).
//
// Deux façons de poser les coupures : toutes les N pages, comme
// `ops::ranges_every`, ou devant chaque page sélectionnée.

import type { SplitReport } from "./api.js";

/// What the field « toutes les N pages » holds, once read.
export type Parsed = { kind: "ok"; every: number } | { kind: "error"; message: string };

/// Read the number of pages per file. Refused, with a sentence for the
/// banner: nothing, something that is not a whole number, zero, or more
/// pages than the document shows — none of which could be cut, and none of
/// which is written before being said.
export function parseEvery(text: string, pageCount: number): Parsed {
  const typed = text.trim();
  if (typed === "") {
    return { kind: "error", message: "Indiquez combien de pages par fichier." };
  }
  if (!/^\d+$/.test(typed)) {
    return { kind: "error", message: `« ${typed} » n'est pas un nombre de pages.` };
  }
  const every = Number(typed);
  if (every < 1) {
    return { kind: "error", message: "Il faut au moins une page par fichier." };
  }
  if (every > pageCount) {
    return {
      kind: "error",
      message: `Le document n'a que ${pageCount} page${pageCount > 1 ? "s" : ""} : indiquez un nombre plus petit.`,
    };
  }
  return { kind: "ok", every };
}

/// Where the cuts fall for « avant chaque page sélectionnée »: the
/// selected positions, in order and without repeats, except the first
/// page, before which a cut would open an empty file, and anything beyond
/// the last page.
export function cutPoints(positions: Iterable<number>, pageCount: number): number[] {
  return [...new Set(positions)].filter((p) => p > 0 && p < pageCount).sort((a, b) => a - b);
}

/// The pages on screen shared out into parts of `every` pages, the last
/// one shorter when the count is not a multiple (`ops::ranges_every`,
/// applied to the order shown).
export function splitEvery(order: readonly number[], every: number): number[][] {
  if (every < 1) {
    return [];
  }
  const parts: number[][] = [];
  for (let start = 0; start < order.length; start += every) {
    parts.push(order.slice(start, start + every));
  }
  return parts;
}

/// The pages on screen shared out at `cuts`, places in the order where a
/// new file starts (`cutPoints`): one part before the first cut, one
/// between each pair, one after the last.
export function splitAt(order: readonly number[], cuts: readonly number[]): number[][] {
  const parts: number[][] = [];
  let start = 0;
  for (const cut of [...cuts, order.length]) {
    // A cut that would open an empty file is no cut at all: it changes
    // nothing, here or in the files written.
    const part = order.slice(start, cut);
    if (part.length > 0) {
      parts.push(part);
      start = cut;
    }
  }
  return parts;
}

/// What the banner says a cut would produce, before it runs: an outline of
/// the effect rather than a bare number to try out (ADR 0004, point 6).
/// Ten parts at most are counted out; the rest is an ellipsis.
export function describeParts(parts: readonly (readonly number[])[]): string {
  if (parts.length === 0) {
    return "Rien à découper.";
  }
  const counts = parts.map((part) => part.length);
  const shown = counts.length > 10 ? [...counts.slice(0, 9).map(String), "…"] : counts.map(String);
  const files = `${parts.length} fichier${parts.length > 1 ? "s" : ""}`;
  return `${files} : ${shown.join(" + ")} page${Math.max(...counts) > 1 ? "s" : ""}.`;
}

/// What the banner and the status bar say once the files are written: how
/// many, and where.
export function splitDone(report: SplitReport): string {
  const files = report.files;
  const first = files[0]?.name ?? "";
  const last = files[files.length - 1]?.name ?? "";
  const which = files.length > 1 ? `de « ${first} » à « ${last} »` : `« ${first} »`;
  return `${files.length} fichier${files.length > 1 ? "s écrits" : " écrit"} dans ${report.dir} : ${which}.`;
}
