// The notices above the grid, as data: `main.ts` draws them, and
// `tests/notices.test.ts` checks them without a DOM.
//
// Some describe the document on screen (repaired, table found elsewhere,
// encrypted) and last as long as it is shown. The others report something
// that happened (a refused deletion, a save, a failed opening) and stay
// until closed. Only a successful opening clears them: an attempt that
// fails leaves the document on screen and every notice in place, and adds
// a single error, which replaces that of an earlier failed attempt. A file
// that needs a password is asked for in a notice too, one file at a time.
//
// The question asked before work is lost, when the window is closed or
// another file opened while the document is modified, is a notice as well,
// never a modal: an explicit stop in place, as ADR 0004 wants for the
// irreversible. It offers three ways out, save, go on without saving,
// cancel, and only its buttons answer it. The window itself is held open
// by the Rust side until the interface asks it to close (`main.ts`).

import { asAppError, type DocumentInfo } from "./api.js";

export type NoticeKind = "info" | "warn" | "error";

/// What the question stands in the way of: closing the window, or opening
/// the file at `path`.
export type Leaving = { kind: "close" } | { kind: "open"; path: string };

/// One of the three ways out of the question.
export interface Choice {
  readonly action: "save" | "discard" | "cancel";
  readonly label: string;
}

export interface Notice {
  /// Unique for the life of the board: tells a notice from its replacement.
  readonly id: number;
  readonly kind: NoticeKind;
  readonly text: string;
  /// `document`: about the document on screen; `event`: something that
  /// happened; `failure`: the last opening that failed; `password`: a
  /// password asked for the file at `path`; `question`: the question asked
  /// before `leaving` loses the modifications.
  readonly role: "document" | "event" | "failure" | "password" | "question";
  /// The file a failure or a password request is about.
  readonly path: string | null;
  /// What a question stands in the way of.
  readonly leaving: Leaving | null;
}

/// How an attempt to open a file ended.
export type Opening = { kind: "opened"; info: DocumentInfo } | { kind: "password" } | { kind: "failed" };

export class NoticeBoard {
  private notices: Notice[] = [];
  private nextId = 1;

  /// In display order.
  get list(): readonly Notice[] {
    return this.notices;
  }

  /// A document was opened: its notices replace all the others.
  documentOpened(info: DocumentInfo): void {
    this.notices = documentNotices(info).map(([kind, text]) => this.make(kind, text, "document", null, null));
  }

  /// Something happened: reported until closed or until a document opens.
  event(kind: NoticeKind, text: string): void {
    this.notices.push(this.make(kind, text, "event", null, null));
  }

  /// Opening `path` failed with `message`.
  openFailed(path: string, message: string): void {
    this.replace(this.make("error", `Impossible d'ouvrir ${path} : ${message}`, "failure", path, null));
  }

  /// `path` needs a password; `wrong` when the one given does not open it.
  passwordNeeded(path: string, wrong: boolean): void {
    const name = fileName(path);
    const text = wrong
      ? `Ce mot de passe n'ouvre pas « ${name} ». Essayez le mot de passe utilisateur ou propriétaire.`
      : `« ${name} » est protégé par un mot de passe.`;
    this.replace(this.make("warn", text, "password", path, null));
  }

  /// Whether a password is being asked for the file at `path`: typing it
  /// goes on with an opening the user already chose, and is not asked
  /// again whether to leave the modified document.
  asksPasswordFor(path: string): boolean {
    return this.notices.some((n) => n.role === "password" && n.path === path);
  }

  /// The document `name` is modified and `leaving` would lose that: ask
  /// what to do, in place of the question asked before, if any.
  ask(name: string, leaving: Leaving): void {
    const what = `« ${name} » a des modifications non enregistrées.`;
    const text =
      leaving.kind === "close"
        ? `${what} Les enregistrer avant de fermer 4YouPDF ?`
        : `${what} Les enregistrer avant d'ouvrir « ${fileName(leaving.path)} » ?`;
    this.replace(this.make("warn", text, "question", null, leaving));
  }

  /// The question was answered, by one of its buttons.
  answered(): void {
    this.notices = this.notices.filter((n) => n.role !== "question");
  }

  close(id: number): void {
    this.notices = this.notices.filter((n) => n.id !== id);
  }

  /// Add `notice` at the end, in place of the notice of the same role.
  private replace(notice: Notice): void {
    this.notices = [...this.notices.filter((n) => n.role !== notice.role), notice];
  }

  private make(
    kind: NoticeKind,
    text: string,
    role: Notice["role"],
    path: string | null,
    leaving: Leaving | null,
  ): Notice {
    const id = this.nextId;
    this.nextId += 1;
    return { id, kind, text, role, path, leaving };
  }
}

/// The three ways out of the question asked before `leaving`, in the order
/// shown: save first, then go on without saving, named after what that
/// does, then cancel. Never fewer.
export function choices(leaving: Leaving): readonly Choice[] {
  return [
    { action: "save", label: "Enregistrer sous…" },
    { action: "discard", label: leaving.kind === "close" ? "Fermer sans enregistrer" : "Ouvrir sans enregistrer" },
    { action: "cancel", label: "Annuler" },
  ];
}

/// Try to open `path` with `open`, the command of the Rust side, and bring
/// the notices up to date: those of the new document once it is open;
/// otherwise the same notices, plus the password request or the error.
export async function attemptOpen(
  notices: NoticeBoard,
  open: (path: string, password?: string) => Promise<DocumentInfo>,
  path: string,
  password?: string,
): Promise<Opening> {
  let info: DocumentInfo;
  try {
    info = await open(path, password);
  } catch (e: unknown) {
    const error = asAppError(e);
    if (error.kind === "wrong_password") {
      notices.passwordNeeded(path, password !== undefined);
      return { kind: "password" };
    }
    notices.openFailed(path, error.message);
    return { kind: "failed" };
  }
  notices.documentOpened(info);
  return { kind: "opened", info };
}

/// What the notices say about a document.
function documentNotices(info: DocumentInfo): [NoticeKind, string][] {
  const notices: [NoticeKind, string][] = [];
  if (info.reconstructed !== null) {
    notices.push([
      "warn",
      `Fichier endommagé, table des objets reconstruite par analyse du fichier (${info.reconstructed}). ` +
        "L'enregistrement produira un fichier sain.",
    ]);
  }
  if (info.relocated_startxref !== null) {
    notices.push([
      "info",
      `Le fichier annonce sa table à un mauvais endroit ; elle a été retrouvée à l'offset ${info.relocated_startxref}.`,
    ]);
  }
  if (info.encryption !== null) {
    notices.push([
      "warn",
      `Fichier chiffré (${info.encryption}). L'enregistrement produira un fichier EN CLAIR, sans protection.`,
    ]);
  }
  return notices;
}

/// Last component of a path, Windows or POSIX separators.
function fileName(path: string): string {
  return path.split(/[/\\]/).pop() ?? path;
}
