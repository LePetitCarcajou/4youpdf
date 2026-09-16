// Notices when a file is opened: an attempt that fails leaves the document
// on screen and its notices as they were, and adds a single error; only a
// successful opening replaces them. The question asked before unsaved
// changes would be lost is a notice too, with three ways out.

import type { AppError, DocumentInfo } from "../src/api.js";
import { attemptOpen, choices, NoticeBoard } from "../src/notices.js";
import { equal, run, test } from "./check.js";

/// A document as the Rust side describes it.
function described(path: string, extra: Partial<DocumentInfo> = {}): DocumentInfo {
  return {
    document: 1,
    path,
    name: path.split("\\").pop() ?? path,
    size: 4096,
    version: "1.7",
    pages: [{ width: 595, height: 842, rotate: 0 }],
    reconstructed: null,
    relocated_startxref: null,
    encryption: null,
    ...extra,
  };
}

/// The open command of the Rust side, answering `result` for any file.
function answering(result: DocumentInfo | AppError): (path: string, password?: string) => Promise<DocumentInfo> {
  return () => ("kind" in result ? Promise.reject(result) : Promise.resolve(result));
}

const damaged = described("C:\\docs\\abîmé.pdf", {
  reconstructed: "startxref absent",
  encryption: "révision 3, RC4, clé 128 bits",
});
const notPdf: AppError = { kind: "other", message: "en-tête %PDF absent" };

/// A board as after opening `damaged`.
async function showingDamaged(): Promise<NoticeBoard> {
  const board = new NoticeBoard();
  equal(await attemptOpen(board, answering(damaged), damaged.path), { kind: "opened", info: damaged }, "opening");
  return board;
}

test("a failed opening keeps the notices of the document on screen and adds one error", async () => {
  const board = await showingDamaged();
  const before = [...board.list];
  equal(
    before.map((n) => [n.kind, n.role]),
    [
      ["warn", "document"],
      ["warn", "document"],
    ],
    "notices of the damaged, encrypted document",
  );

  const opening = await attemptOpen(board, answering(notPdf), "C:\\docs\\notes.txt");

  equal(opening, { kind: "failed" }, "outcome");
  equal(board.list.slice(0, before.length), before, "notices of the document after the failure");
  equal(
    board.list.slice(before.length).map((n) => [n.kind, n.text]),
    [["error", "Impossible d'ouvrir C:\\docs\\notes.txt : en-tête %PDF absent"]],
    "the failure, reported once",
  );
});

test("a second failure replaces the error of the first; other notices stay", async () => {
  const board = await showingDamaged();
  board.event("warn", "Un document doit garder au moins une page.");
  await attemptOpen(board, answering(notPdf), "C:\\docs\\notes.txt");
  await attemptOpen(board, answering({ kind: "other", message: "fichier introuvable" }), "C:\\docs\\absent.pdf");

  equal(board.list.map((n) => n.role), ["document", "document", "event", "failure"], "notices");
  equal(board.list[3]?.text, "Impossible d'ouvrir C:\\docs\\absent.pdf : fichier introuvable", "error of the last attempt");
});

test("a file that needs a password leaves the document on screen and its notices", async () => {
  const board = await showingDamaged();
  const before = [...board.list];
  const secret = "C:\\docs\\secret.pdf";

  equal(await attemptOpen(board, answering({ kind: "wrong_password" }), secret), { kind: "password" }, "outcome");
  equal(board.list.slice(0, 2), before, "notices of the document");
  equal(
    board.list.slice(2).map((n) => [n.role, n.path, n.text]),
    [["password", secret, "« secret.pdf » est protégé par un mot de passe."]],
    "password request",
  );

  await attemptOpen(board, answering({ kind: "wrong_password" }), secret, "1234");
  equal(board.list.slice(0, 2), before, "notices of the document after a wrong password");
  equal(
    board.list.slice(2).map((n) => n.text),
    ["Ce mot de passe n'ouvre pas « secret.pdf ». Essayez le mot de passe utilisateur ou propriétaire."],
    "a single request, which says the password is wrong",
  );
});

test("only a successful opening replaces the notices", async () => {
  const board = await showingDamaged();
  board.event("info", "Le fichier enregistré est en clair : la protection du fichier d'origine n'a pas été reportée.");
  await attemptOpen(board, answering(notPdf), "C:\\docs\\notes.txt");
  await attemptOpen(board, answering({ kind: "wrong_password" }), "C:\\docs\\secret.pdf");
  equal(board.list.length, 5, "notices before");

  const clean = described("C:\\docs\\propre.pdf");
  equal(await attemptOpen(board, answering(clean), clean.path), { kind: "opened", info: clean }, "outcome");
  equal(board.list, [], "notices of a document in good order");

  const relocated = described("C:\\docs\\décalé.pdf", { relocated_startxref: 209 });
  await attemptOpen(board, answering(relocated), relocated.path);
  equal(
    board.list.map((n) => [n.kind, n.role, n.text]),
    [["info", "document", "Le fichier annonce sa table à un mauvais endroit ; elle a été retrouvée à l'offset 209."]],
    "notices of the new document",
  );
});

test("the question before losing changes names the document, what it stands in the way of, and three ways out", async () => {
  const board = await showingDamaged();
  const before = [...board.list];

  board.ask("abîmé.pdf", { kind: "close" });
  equal(board.list.slice(0, 2), before, "notices of the document stay");
  equal(
    board.list.slice(2).map((n) => [n.kind, n.role, n.text, n.leaving]),
    [["warn", "question", "« abîmé.pdf » a des modifications non enregistrées. Les enregistrer avant de fermer 4YouPDF ?", { kind: "close" }]],
    "asked before closing",
  );
  equal(
    choices({ kind: "close" }),
    [
      { action: "save", label: "Enregistrer sous…" },
      { action: "discard", label: "Fermer sans enregistrer" },
      { action: "cancel", label: "Annuler" },
    ],
    "three ways out of closing",
  );

  // Asked again, for another file this time: one question at a time.
  const other = "C:\\docs\\autre.pdf";
  board.ask("abîmé.pdf", { kind: "open", path: other });
  equal(
    board.list.slice(2).map((n) => [n.role, n.text, n.leaving]),
    [
      [
        "question",
        "« abîmé.pdf » a des modifications non enregistrées. Les enregistrer avant d'ouvrir « autre.pdf » ?",
        { kind: "open", path: other },
      ],
    ],
    "asked before opening, in place of the first question",
  );
  equal(
    choices({ kind: "open", path: other }).map((c) => c.label),
    ["Enregistrer sous…", "Ouvrir sans enregistrer", "Annuler"],
    "three ways out of opening",
  );

  board.answered();
  equal(board.list, before, "answered: gone, the rest as it was");
});

test("a password being typed goes on with the opening already chosen", async () => {
  const board = await showingDamaged();
  const secret = "C:\\docs\\secret.pdf";
  equal(board.asksPasswordFor(secret), false, "no request yet");
  await attemptOpen(board, answering({ kind: "wrong_password" }), secret);
  equal([board.asksPasswordFor(secret), board.asksPasswordFor("C:\\docs\\autre.pdf")], [true, false], "asked for that file");

  // The question, if asked meanwhile, and the request live side by side;
  // a document opened clears both.
  board.ask("abîmé.pdf", { kind: "close" });
  equal(board.list.map((n) => n.role), ["document", "document", "password", "question"], "both");
  await attemptOpen(board, answering(described(secret)), secret, "1234");
  equal([board.list, board.asksPasswordFor(secret)], [[], false], "opened");
});

await run();
