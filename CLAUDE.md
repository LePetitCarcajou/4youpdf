# 4YouPDF — consignes pour Claude Code

Lis d'abord : `README.md`, `docs/architecture.md`, `docs/diagrams.md`,
`docs/adr/*.md`, `docs/paliers.md`.

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
- Deux lignées de versions, vérifiées par `tools/check_version.py` :
  - `fyp-core`, `fyp-crypto`, `fyp-conformance`, `fyp-cli` et `fyp-app`
    prennent `[workspace.package] version` (`version.workspace = true`), que
    `[workspace.dependencies]` demande exactement et que nomme le tag de
    release (`v<version>`, job `version-check` de `release.yml`) ;
  - `fyp-plugin-api` et `fyp-host` gardent chacun leur propre version, parce
    que le contrat des modules est versionné à part (ADR 0002, ADR 0003) : ne
    jamais les aligner sur le workspace. Un module de `plugins/` a aussi la
    sienne, celle de son `manifest.toml`.
- Un MSRV pour le produit, `[workspace.package] rust-version`, sans
  surcharge par crate ; le plus bas possible pour le contrat,
  `fyp-plugin-api`, qui garde le sien. Même logique que les deux lignées de
  versions : `tools/check_version.py --rust-version` refuse une surcharge,
  et les jobs `msrv-product` et `msrv-plugin-api` de la CI compilent et
  testent chacun avec exactement sa valeur.
- L'identifiant de l'application, `org.fouryoupdf.desktop`, ne change plus :
  il nomme le dossier de ses données (`%LOCALAPPDATA%\org.fouryoupdf.desktop`
  sous Windows), et le changer les déplacerait. Les modules du dépôt et les
  réglages utilisent le même préfixe, `org.fouryoupdf.`.

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

## État et prochaine étape
Version 0.3.3 : noyau (lecture, réparation, déchiffrement, écriture),
opérations de pages, modules WebAssembly exécutés par `fyp run`, application
Tauri et empaquetage Windows ; aucune release n'a encore de fichier à
télécharger. Prochaine version : le palier v0.3.4, première release publique.
Feuille de route : `docs/architecture.md`, « Feuille de route ».

Chaque session suit `docs/paliers.md` : un objectif écrit, et toute
trouvaille qui n'empêche pas de l'atteindre va au backlog
(`docs/backlog-technique.md`, `docs/backlog-ui.md`), même si la correction
tient en deux lignes.
