// The page numbers of the page view, as data: `viewer.ts` shows them, and
// `tests/pagenumber.test.ts` checks them without a DOM.
//
// A page is numbered by its position in the order, from 1: the numbering of
// the document as it will be saved, which the caption of the view and the
// label of each tile give first. Its number in the file as opened comes
// second, and only when it differs; after a deletion, some of those numbers
// name no page any more. The number typed to go to a page is therefore a
// position too, and the view says so while it is typed.
//
// A number that names no page changes nothing: the view stays on its page
// and says, in place, what it expects (ADR 0004).

/// The caption of a page: what follows its number (`sur 12`), and the whole
/// caption, read out when the page changes.
export interface PageCaption {
  after: string;
  spoken: string;
}

/// What was typed to go to a page: the page at `position` in the order, or
/// why it names none.
export type TypedNumber =
  | { kind: "page"; position: number }
  | { kind: "not_a_number" }
  | { kind: "out_of_range" };

/// The caption of the page at `position` of an order of `count` pages, page
/// `page` of the file; both from 0.
export function pageCaption(position: number, page: number, count: number): PageCaption {
  const after = page === position ? `sur ${count}` : `sur ${count} (page ${page + 1} du fichier)`;
  return { after, spoken: `Page ${position + 1} ${after}` };
}

/// Read `text`, typed to go to one of the `count` pages of the order.
/// Spaces around it do not count; anything but the digits 0 to 9 is not a
/// number.
export function readPageNumber(text: string, count: number): TypedNumber {
  const typed = text.trim();
  if (!/^[0-9]+$/.test(typed)) {
    return { kind: "not_a_number" };
  }
  // Compared by length first: a number longer than `count` is out of range
  // however many digits it has, and is never rounded into range.
  const digits = typed.replace(/^0+/, "");
  if (digits === "" || digits.length > String(count).length || Number(digits) > count) {
    return { kind: "out_of_range" };
  }
  return { kind: "page", position: Number(digits) - 1 };
}

/// What the number expects while it is typed, among `count` pages.
export function pageNumberHelp(count: number): string {
  return `Numéro dans l'ordre actuel (${bounds(count)}) · Entrée : y aller · Échap : annuler`;
}

/// Why `typed` goes to none of the `count` pages.
export function pageNumberRefusal(typed: Exclude<TypedNumber, { kind: "page" }>, count: number): string {
  return typed.kind === "out_of_range"
    ? `Aucune page ne porte ce numéro dans l'ordre actuel (${bounds(count)})`
    : `Tapez un numéro dans l'ordre actuel (${bounds(count)})`;
}

function bounds(count: number): string {
  return count > 1 ? `1 à ${count}` : "1";
}
