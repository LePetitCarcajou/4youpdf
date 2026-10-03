// The page lists typed in the window (`1,3,5-8`, `8-5` for the reverse
// order), read as the command line reads them: `parsePages` follows
// `parse_pages` of `crates/fyp-cli/src/main.rs` rule for rule and message
// for message, and both go through the cases of
// `tests/fixtures/page-ranges.tsv` (`tests/pagerange.test.ts`, and the test
// `page_lists_are_read_as_the_shared_cases_say` of the command line). The
// Rust reader cannot be called from here without a round trip per key
// typed; the two are aligned rather than shared.
//
// What the window adds lives in `readPages`: an empty field means every
// page, and a page typed twice is refused. `fyp merge --pages 3,3` takes
// such a page twice; the window does not offer that for now
// (`docs/backlog-ui.md`).

/// What reading a page list gives: the pages, 0-based, in the order typed;
/// or the refusal, in the words of the command line.
export type Parsed = { kind: "ok"; pages: number[] } | { kind: "error"; message: string };

/// What the field of a file gives in the window: every page, when nothing
/// is typed; these pages, 0-based, each once; or why not.
export type Read = { kind: "all" } | { kind: "pages"; pages: number[] } | { kind: "error"; message: string };

/// The largest page number the command line reads (`usize` on the 64-bit
/// systems it is built for): beyond, a number is not one.
const USIZE_MAX = "18446744073709551615";

/// Read a 1-based page list such as `1,3,5-8` or `8-5` into 0-based
/// indices, in the order written, every number within `count`, as
/// `parse_pages` does: parts separated by commas, empty parts skipped, a
/// range `a-b` split at its first hyphen and read backwards when `a` is
/// the larger; the first number that is not one, or not a page of the
/// document, refuses the whole list.
export function parsePages(spec: string, count: number): Parsed {
  const pages: number[] = [];
  for (const part of spec.split(",").map((p) => p.trim())) {
    if (part === "") {
      continue;
    }
    const hyphen = part.indexOf("-");
    if (hyphen < 0) {
      const page = pageNumber(part, count);
      if (page.kind === "error") {
        return page;
      }
      pages.push(page.index);
      continue;
    }
    const from = pageNumber(part.slice(0, hyphen), count);
    if (from.kind === "error") {
      return from;
    }
    const to = pageNumber(part.slice(hyphen + 1), count);
    if (to.kind === "error") {
      return to;
    }
    const step = from.index <= to.index ? 1 : -1;
    for (let index = from.index; index !== to.index + step; index += step) {
      pages.push(index);
    }
  }
  if (pages.length === 0) {
    return { kind: "error", message: "aucune page indiquée (exemple : 1,3,5-8)" };
  }
  return { kind: "ok", pages };
}

/// Read what is typed in the field of a file that has `count` pages: an
/// empty field takes every page; otherwise the list, read by `parsePages`,
/// where a page may appear only once.
export function readPages(typed: string, count: number): Read {
  if (typed.trim() === "") {
    return { kind: "all" };
  }
  const parsed = parsePages(typed, count);
  if (parsed.kind === "error") {
    return parsed;
  }
  const seen = new Set<number>();
  for (const index of parsed.pages) {
    if (seen.has(index)) {
      return { kind: "error", message: `la page ${index + 1} est demandée deux fois` };
    }
    seen.add(index);
  }
  return { kind: "pages", pages: parsed.pages };
}

/// One page number, as Rust reads a `usize` (an optional `+`, then ASCII
/// digits, no more than it holds), then checked against `count`.
function pageNumber(text: string, count: number): { kind: "ok"; index: number } | { kind: "error"; message: string } {
  const typed = text.trim();
  if (!/^\+?[0-9]+$/.test(typed)) {
    return invalid(typed);
  }
  // The number as Rust prints it: no sign, no leading zero.
  const digits = typed.replace(/^\+/, "").replace(/^0+(?=[0-9])/, "");
  if (digits.length > USIZE_MAX.length || (digits.length === USIZE_MAX.length && digits > USIZE_MAX)) {
    return invalid(typed);
  }
  // Past fifteen digits a number is beyond any page count, and beyond what
  // a `number` holds exactly: it is kept as written for the message.
  const number = digits.length > 15 ? Infinity : Number(digits);
  if (number === 0 || number > count) {
    return {
      kind: "error",
      message: `page ${digits} hors limites : le document a ${count} page${count > 1 ? "s" : ""}, numérotées de 1 à ${count}`,
    };
  }
  return { kind: "ok", index: number - 1 };
}

function invalid(typed: string): { kind: "error"; message: string } {
  return { kind: "error", message: `numéro de page invalide : « ${typed} »` };
}
