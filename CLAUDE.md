# 4YouPDF — consignes pour Claude Code

Lis d'abord : `README.md`, `docs/architecture.md`, `docs/diagrams.md`,
`docs/adr/*.md`.

## Invariants à ne jamais casser
- `unsafe` interdit partout (`[workspace.lints.rust] unsafe_code = "forbid"`).
- `fyp-core` ne panique jamais sur une entrée. Pas de `unwrap`/`expect`/`panic`
  hors `#[cfg(test)]`. Toute erreur est un `fyp_core::Error`.
- Dépendances : `fyp-core` ne dépend d'aucun plugin. Les plugins ne dépendent
  que de `fyp-plugin-api`. Vérifie le sens des flèches avant d'ajouter une
  dépendance.
- `crates/fyp-plugin-api` est versionné séparément. Un changement cassant y
  exige un bump de version et une mention dans la PR.
- Un module tiers est WebAssembly, jamais natif.

## Avant de proposer une PR
```
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

## Conventions
- Commits Conventional Commits, signés (`-s`).
- Code et commits en anglais, docs utilisateur en français.
- Tout cas de PDF malformé nouvellement géré = une fixture dans
  `tests/fixtures/` + un test.
- Références à la norme sous la forme `ISO 32000-2, 7.3.8` dans les commentaires.

## Prochaine étape (jalon 0.1)
Voir `docs/architecture.md`, section « Feuille de route ».
