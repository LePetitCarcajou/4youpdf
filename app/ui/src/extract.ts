// « Extraire la sélection… » : les pages sélectionnées, telles que la
// grille les montre, dans un nouveau fichier. Ce que cela demande sans
// DOM est ici, et `tests/extract.test.ts` le vérifie ; `main.ts` ouvre le
// sélecteur d'enregistrement et appelle `save_document`.
//
// Le document ouvert ne change pas : rien n'entre dans l'historique, et
// le point d'enregistrement reste où il est (`history.ts`). Extraire
// n'est donc pas enregistrer, même si le côté Rust écrit de la même
// façon : `ops::extract_pages` sur une liste d'indices de pages.

/// What the grid holds when the action is asked for: a document on
/// screen, whether the page view has taken over (the panel edits
/// nothing), and how many pages are selected.
export interface GridState {
  readonly hasDocument: boolean;
  readonly viewerOpen: boolean;
  readonly selected: number;
}

/// Whether « Extraire la sélection… » may run. An empty selection makes it
/// unavailable, and never an error: there is nothing to extract, which is
/// not a mistake to report.
export function canExtract(grid: GridState): boolean {
  return grid.hasDocument && !grid.viewerOpen && grid.selected > 0;
}

/// The pages the selection stands for: `positions` are places in the order
/// on screen, and the file knows pages. Taken in the order of the grid,
/// whatever the order they were selected in, so that the file written
/// reads like what is shown.
export function selectedPages(order: readonly number[], positions: Iterable<number>): number[] {
  return [...new Set(positions)]
    .sort((a, b) => a - b)
    .map((position) => order[position])
    .filter((page): page is number => page !== undefined);
}

/// The name the save dialog suggests for an extraction: that of the
/// document, without its extension, followed by `-extrait`, as saving
/// suggests `-modifié`.
export function extractName(documentName: string): string {
  const stem = documentName.replace(/\.pdf$/i, "").trim();
  return `${stem === "" ? "document" : stem}-extrait.pdf`;
}
