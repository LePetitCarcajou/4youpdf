# Contributing to 4YouPDF

## Rule no. 1: DCO, no CLA

Every commit must be signed off with `git commit -s`, which adds a
`Signed-off-by: Name <email>` line. It means that you certify the
[Developer Certificate of Origin](https://developercertificate.org/): you
have the right to submit this code under AGPL-3.0-or-later.

We do **not** use a CLA. A CLA transfers rights to an entity that can then
change the licence. With the DCO, every contributor keeps their rights;
relicensing would require everyone's agreement. That is the guarantee that
the project stays free.

## Building on Linux

The workspace contains the desktop application (`app/`, crate `fyp-app`),
built on Tauri 2 and linked against WebKitGTK. `cargo clippy --workspace` and
`cargo test --workspace` compile it, so its system libraries have to be
installed first. On Debian or Ubuntu (CI runs on Ubuntu 24.04):

```
sudo apt-get update
sudo apt-get install libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libsoup-3.0-dev pkg-config
```

Without these packages, the build stops on a `pkg-config` error (`glib-2.0`,
`gobject-2.0`…). For other distributions, see the
[Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) and
`app/README.md` (in French). To work on the core or the CLI without
installing them, `cargo test --workspace --exclude fyp-app` is enough
locally; CI, for its part, compiles everything.

Windows (WebView2, present on Windows 11) and macOS need nothing more.

## Workflow

1. Open an issue before any non-trivial change.
2. Branch from `main`: `feat/<subject>` or `fix/<subject>`.
3. Commits in [Conventional Commits](https://www.conventionalcommits.org/):
   `feat(core): parse xref streams`, `fix(cli): ...`, `docs(adr): ...`.
   A breaking change carries a `!`: `refactor(plugin-api)!: ...`.
4. PR to `main`, merged as a *squash*. CI must be green:
   `cargo fmt --all --check`,
   `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo test --workspace`, `cargo deny check`.
5. A PR that touches `crates/fyp-plugin-api` must say whether the change is
   compatible or breaking and adjust the version of that crate accordingly.

## Rampes and paliers

The project alternates *rampes*, which add features, and *paliers*, which add
nothing and settle the debt; a finding made during a piece of work goes to
the backlog, unless it stands in the way of that work's objective. The PR
that closes a palier copies and ticks the exit grid of `docs/paliers.md`
(in French), which describes the method.

## Code rules

- `unsafe` is forbidden throughout the workspace.
- The core never panics on an input: every error is a value. No `unwrap()` /
  `expect()` outside tests.
- Every new pathological file case gives a fixture + a test.
- English for the code and the commits; French welcome in issues and in the
  user documentation.

## Architecture decisions

The structuring choices are recorded in `docs/adr/` (in French). Proposing an
architecture change = proposing a new ADR.
