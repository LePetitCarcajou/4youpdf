# 4YouPDF — consignes pour Claude Code

Lis d'abord : `README.md`, `docs/architecture.md`, `docs/diagrams.md`,
`docs/adr/*.md`.

## Invariants à ne jamais casser
- `unsafe` interdit partout (`[workspace.lints.rust] unsafe_code = "forbid"`).
- `fyp-core` ne panique jamais sur une entrée. Pas de `unwrap`/`expect`/`panic`
  hors `#[cfg(test)]`. Toute erreur est un `fyp_core::Error`.
- Dépendances : `fyp-core` ne dépend d'aucun plugin. Un plugin ne dépend
  jamais de l'hôte (`fyp-host`) ni d'une interface : il parle à l'hôte par le
  seul contrat `fyp-plugin-api`. Il peut embarquer `fyp-core` à la
  compilation, dans son propre binaire WebAssembly : cela ne lui donne aucune
  autorité à l'exécution, le sandbox ne fait confiance à aucun code qu'il
  exécute. Vérifie le sens des flèches avant d'ajouter une dépendance
  (`docs/diagrams.md`, diagramme 1).
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
