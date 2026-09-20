# 4YouPDF

**The VLC of PDF.** Free software for PDF, at no cost, with no account, no
ads, no files sent over the Internet by its core, and outside the control of
any for-profit organisation.

- Opens PDFs, even broken ones — and explains why when it cannot.
- A core written in Rust (`#![forbid(unsafe_code)]`). Every night, CI fuzzes
  the complete opening of a document on arbitrary bytes, seeded by the
  fixtures and the public corpus: cross-reference tables, reconstruction,
  filters, decryption, page operations and writing; then the filters alone,
  the lexer and the object parser, the quick read of the header, the module
  contract and the host's WASI functions (`fuzz/README.md` (in French); what
  remains out of reach: `docs/backlog-technique.md` (in French)).
- A core — everything that runs outside the module sandbox — that never
  reaches the network: no telemetry, no automatic update, no setting to turn
  one on.
- Sandboxed modules (WebAssembly) with explicit permissions: a merge module
  cannot talk to the network, and a module that needs to will only be able to
  reach the exact servers it declares, shown to you and subject to your
  agreement (see `docs/adr/0006-fonctionnement-local.md` (in French)).
- Aimed at, not there yet: conformance visible at all times (PDF/A, PDF/X,
  PDF/E, PDF/UA, PDF/VT, PAdES) with assisted correction, and native writing
  of PDF 2.0 (ISO 32000-2:2020).

## Install

Since v0.3.4, every release attaches to the
[Releases](https://github.com/LePetitCarcajou/4youpdf/releases) page two
files for Windows 10 and 11 (x64), whichever you prefer:

- **`4YouPDF_<version>_x64-setup.exe`**, the installer. It installs 4YouPDF
  for your account only, without administrator rights, in
  `%LOCALAPPDATA%\4YouPDF`, with a shortcut in the Start menu and, if you
  leave the box ticked, on the desktop. Uninstalling, from "Installed apps",
  removes everything it put in place. If WebView2, the display engine of
  Windows, is missing (it is part of Windows 11), the installer has Microsoft
  install it, which requires an Internet connection.
- **`4YouPDF_<version>_x64_portable.zip`**, the portable version. Extract the
  archive wherever you want, USB stick included, then run
  `4YouPDF\4YouPDF.exe`. Nothing is installed and 4YouPDF writes nothing to
  the registry: what it does write, the display engine's cache, stays in
  `4YouPDF\data`, and deleting the folder deletes everything. WebView2 must
  already be present.

`SHA256SUMS.txt`, on the same page, gives the hash of each file. The
releases up to v0.3.3 have no file; the two files of v0.3.4 are named 0.3.3,
the version of the workspace not having been raised before its tag, and
those of the following releases carry the number of their release. macOS and
Linux have no package yet: see `app/README.md` (in French) to build the
application.

### "Windows protected your PC"

On the first run of a downloaded file, the installer as well as the
`4YouPDF.exe` from the archive, Microsoft Defender SmartScreen shows
"Windows protected your PC": it "prevented an unrecognized app from
starting". This is expected: the files are not signed. Signing requires a
code signing certificate, paid for and renewed every year, which the project
does not have; without a signature and without an established reputation with
Microsoft, SmartScreen warns on principle, having detected nothing. To carry
on: "More info", then "Run anyway".

This is not blind trust: the code is public, and these files are built
publicly by the repository's CI, from the version tag and without manual
intervention (`.github/workflows/release.yml`; each run and its log are
visible in the Actions tab). To check that the downloaded file is indeed that
one, compare its hash (`Get-FileHash <file>` in PowerShell) with
`SHA256SUMS.txt`, or verify the provenance attestation that GitHub signs at
build time: `gh attestation verify <file> --repo LePetitCarcajou/4youpdf`.

If Windows 11's Smart App Control is on, it blocks unsigned programs without
offering a way past: 4YouPDF does not start under it as long as it is not
signed.

## State

Version 0.4.1. What exists:

- **Core** (`fyp-core`, `fyp-crypto`): reading of cross-reference tables
  (classic, in streams, hybrid, `/Prev` chain), of object streams and of the
  Flate filters with predictors, ASCIIHex, ASCII85 and RunLength; repair by
  scan of an unusable table; decryption of the standard handler (RC4 and AES,
  revisions 2 to 6); writing of a clean single-section file, always in the
  clear. Measured on 13 September 2026 on the public corpus of 4,529 files:
  4,472 complete round-trips (opening, rewriting, reading back, comparison),
  44 refusals at opening and 13 failures, each one classified in
  `docs/architecture.md` (in French), no panic.
- **Page operations**: merge, extract, split, rotate and delete, through the
  `fyp` command line.
- **Modules**: WebAssembly modules run in a Wasmtime sandbox, with limits on
  time, memory and output, and re-validation by the core of what they return;
  one module, merge, which `fyp run` launches. Only the document read and
  write permissions exist, and modules are not signed (ADR 0003, "Limites
  connues", in French).
- **Desktop application** (`app/`, Tauri 2): open a PDF, see its pages as
  thumbnails or one at a time in full size, reorder them, rotate them, delete
  them, append the pages of other files, undo and redo, save the result. The
  pages are drawn by PDFium (ADR 0005, in French).
- **Windows packaging**: an NSIS installer and a portable archive, unsigned,
  built by `tools/package_app.py`.

Not yet: encryption on writing, PDF 2.0 writing, conformance
(`fyp-conformance` only defines types), modules in the application, command
palette, packages for macOS and Linux. What comes next:
`docs/feuille-de-route.md` (in French).

## Build

Rust 1.95 at minimum for the product (`rust-version` of `Cargo.toml`, imposed
by Wasmtime 48), installed by rustup, and Python 3.11 or later for the
scripts in `tools/`. Inside the repository, rustup takes the stable toolchain
of `rust-toolchain.toml`, with the `wasm32-wasip1` target of the modules;
with another toolchain, Rust 1.95 included, add that target
(`rustup target add wasm32-wasip1 --toolchain 1.95`). On Linux, the
application first asks for the system libraries of `app/README.md`
(in French), "Prérequis système". The module contract, `fyp-plugin-api`,
which module authors compile, makes do with Rust 1.85.

In this order, from the root of the repository (on Linux and macOS, `python`
is often called `python3`):

```
cargo build --workspace
cargo test --workspace
cargo run -p fyp-cli -- info tests/fixtures/minimal.pdf
cargo run -p fyp-cli -- modules plugins --trusted
python tools/build_modules.py      # modules of plugins/ -> plugins/<name>/module.wasm
cargo run --release --manifest-path tools/bench_host/Cargo.toml -- hog 8   # loader measurements (ADR 0003, "Limites connues", in French); without an argument: the list of commands
cargo run -p fyp-cli -- run merge tests/fixtures/minimal.pdf tests/fixtures/objstm.pdf -o fusion.pdf
python tools/fetch_ui_tools.py
python tools/fetch_pdfium.py
python tools/build_ui.py
cargo run -p fyp-app
cargo run --release -p fyp-render-bench   # rendering fidelity bench, after fetch_pdfium.py and tools/fetch_corpus.py -> target/render-bench/ (docs/banc-rendu.md, in French)
python tools/package_app.py        # Windows: installer and portable archive -> target/release/bundle/
```

## Layout

| Folder | Role |
|---|---|
| `crates/fyp-core` | PDF syntax, object model, xref, filters, writing |
| `crates/fyp-crypto` | standard encryption (RC4, AES, revisions 2 to 6) |
| `crates/fyp-conformance` | types of the future PDF/A, X, E, UA, VT rule engine: no rule yet |
| `crates/fyp-plugin-api` | **module contract** — versioned separately |
| `crates/fyp-host` | module loading, permissions, limits |
| `crates/fyp-cli` | the `fyp` binary |
| `plugins/` | official modules |
| `app/` | Tauri 2 desktop application: see `app/README.md` (in French) |
| `tests/` | fixtures; public corpus fetched by `tools/fetch_corpus.py`, ignored by Git |
| `fuzz/` | cargo-fuzz targets |
| `docs/` | architecture, ADRs, manifest format (in French) |

## Licence

AGPL-3.0-or-later. Contributions under the DCO (`git commit -s`), no CLA:
nobody can change the licence of this project without the agreement of every
contributor. See `CONTRIBUTING.md`.
