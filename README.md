# 4YouPDF

**Le VLC du PDF.** Un logiciel PDF libre, gratuit, sans compte, sans pub, sans
envoi de fichiers sur Internet, et hors du contrôle de tout organisme lucratif.

- Ouvre tout, même les PDF cassés — et explique pourquoi quand il ne peut pas.
- Un noyau en Rust (`#![forbid(unsafe_code)]`), fuzzé en continu.
- Des modules sandboxés (WebAssembly) avec permissions explicites : un module de
  fusion ne peut pas parler au réseau.
- Conformité visible en permanence : PDF/A, PDF/X, PDF/E, PDF/UA, PDF/VT, PAdES,
  avec correction assistée.
- Écriture PDF 2.0 (ISO 32000-2:2020) native.

## État

Jalon 0.1 en cours : noyau syntaxique (lexer, parseur d'objets, détection de
version) et contrat des modules. Pas encore d'interface graphique — elle
arrive quand le noyau réécrit 100 % du corpus de test sans perte.

```
cargo build --workspace
cargo test --workspace
cargo run -p fyp-cli -- info tests/fixtures/minimal.pdf
cargo run -p fyp-cli -- modules plugins --trusted
python tools/build_modules.py      # modules de plugins/ -> plugins/<nom>/module.wasm
cargo run -p fyp-cli -- run merge tests/fixtures/minimal.pdf tests/fixtures/objstm.pdf -o fusion.pdf
python tools/fetch_ui_tools.py && python tools/fetch_pdfium.py && python tools/build_ui.py
cargo run -p fyp-app
```

## Structure

| Dossier | Rôle |
|---|---|
| `crates/fyp-core` | syntaxe PDF, modèle objet, xref, filtres, écriture |
| `crates/fyp-crypto` | chiffrement standard (RC4, AES, révision 6) |
| `crates/fyp-conformance` | moteur de règles PDF/A, X, E, UA, VT |
| `crates/fyp-plugin-api` | **contrat des modules** — versionné séparément |
| `crates/fyp-host` | chargement des modules, permissions, limites |
| `crates/fyp-cli` | binaire `fyp` |
| `plugins/` | modules officiels |
| `app/` | application desktop Tauri (jalon 0.3) : voir `app/README.md` |
| `tests/` | fixtures et corpus (git-lfs) |
| `fuzz/` | cibles cargo-fuzz |
| `docs/` | architecture, ADR, format de manifeste |

## Licence

AGPL-3.0-or-later. Contributions sous DCO (`git commit -s`), pas de CLA :
personne ne peut changer la licence de ce projet sans l'accord de chaque
contributeur. Voir `CONTRIBUTING.md`.
