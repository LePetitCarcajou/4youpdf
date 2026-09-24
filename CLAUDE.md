# 4YouPDF — règles de travail pour Claude Code

4YouPDF est un logiciel PDF libre (AGPL-3.0-or-later) : le « VLC du PDF ».
Aucun fichier ne quitte la machine, aucun tiers ne le contrôle, le public
peut écrire des modules. Martin est l'architecte : il décide du quoi et du
pourquoi, tu écris le code. Ce fichier vaut pour chaque session, même après
`/clear`.

## Rôles et déroulé d'une session

1. Une session = un brief, dans `docs/sessions/<id>.md`. Le brief fixe le
   périmètre ; rien d'autre n'est fait.
2. **Phase 1, état des lieux** : relis les fichiers nommés par le brief et
   ceux qu'ils touchent, puis présente l'existant et ta proposition
   (interface, commandes, découpage du code). **Arrête-toi et attends le
   « go » de Martin.** Toute décision d'architecture lui revient.
3. **Phase 2, réalisation** : code, tests, docs, puis le rapport de fin
   écrit dans `docs/sessions/<id>-rapport.md` (format plus bas).

## Règles strictes

- **Périmètre** : toute trouvaille hors sujet (bogue, dette, idée) devient
  une ligne datée dans `docs/backlog-ui.md` ou `docs/backlog-technique.md`,
  jamais un correctif, même s'il tient en deux lignes. Vérifie d'abord
  qu'elle n'y est pas déjà. Seule exception, une trouvaille qui empêche
  d'atteindre l'objectif du brief : arrête-toi et propose ; si Martin
  l'accepte, elle devient une tâche de la session (`docs/paliers.md`,
  « Règle de périmètre »).
- **Git** : jamais de `git commit`, `git push`, `git add`, `git stash`,
  `git checkout`, `git reset` ni `git rebase`. Martin committe à la main.
  Lecture seule (`status`, `diff`, `log`, `show`) autorisée.
- **Formatage** : `cargo fmt -p <crate>` sur les seuls crates modifiés,
  jamais `cargo fmt` sur tout l'espace de travail. Si un fichier hors
  périmètre change quand même, remets-le exactement dans son état d'avant
  et signale-le dans le rapport.
- **Lint** : `cargo clippy -p <crate> --all-targets -- -D warnings` passe
  sur chaque crate modifié, comme en CI.
- **Langues** : code, identifiants, noms de tests et commentaires en
  anglais ; documentation, textes d'interface et rapport en français.
- **Pas de dépendance nouvelle** (crate, paquet npm) sans l'avoir proposée
  en phase 1. Entre crates du dépôt, vérifie d'abord le sens des flèches
  (`docs/diagrams.md`, diagramme 1). Si `Cargo.lock` change,
  `cargo deny check` passe.
- **PDF malformé** : tout cas nouvellement géré a sa fixture dans
  `tests/fixtures/` et son test.
- **Norme** : les commentaires la citent sous la forme
  `ISO 32000-2, 7.3.8`.
- **Le noyau ne bouge pas** (`crates/fyp-core`) sauf si le brief le dit.
  S'il le faut quand même, arrête-toi et propose.

## Invariants du projet (ne jamais les casser)

- **`unsafe` interdit partout** : `[workspace.lints.rust] unsafe_code =
  "forbid"`, hérité par chaque crate, application comprise.
- `fyp-core` : aucun code natif, compile pour `wasm32-wasip1`. Lecture
  tolérante, écriture stricte et conforme. Ne panique jamais sur une
  entrée : pas de `unwrap`/`expect`/`panic` hors `#[cfg(test)]`, toute
  erreur est un `fyp_core::Error`.
- **Sens des dépendances** : `fyp-core` ne dépend d'aucun module. Un module
  ne dépend jamais de l'hôte (`fyp-host`) ni d'une interface : il parle à
  l'hôte par le seul contrat `fyp-plugin-api`. Il peut embarquer
  `fyp-core` dans son propre binaire WebAssembly, sans y gagner aucune
  autorité à l'exécution : le sandbox ne fait confiance à aucun code qu'il
  exécute. Un module tiers est WebAssembly, jamais natif.
- **Deux lignées de versions**, vérifiées par `tools/check_version.py` :
  `fyp-core`, `fyp-crypto`, `fyp-conformance`, `fyp-cli` et `fyp-app`
  prennent `[workspace.package] version` (`version.workspace = true`), que
  `[workspace.dependencies]` demande exactement (tags de rampe et de
  palier : `docs/paliers.md`, « Nommage ») ; `fyp-plugin-api`, `fyp-host`
  et chaque module de `plugins/` (son `manifest.toml`) gardent la leur
  (ADR 0002, ADR 0003), jamais alignée sur le workspace. Un changement
  cassant de `fyp-plugin-api` exige un bump de sa version, signalé dans le
  rapport.
- **MSRV** : `[workspace.package] rust-version` pour le produit, sans
  surcharge par crate ; `fyp-plugin-api` garde le sien, le plus bas
  possible. `tools/check_version.py --rust-version` refuse une surcharge ;
  les jobs CI `msrv-product` et `msrv-plugin-api` testent chacun sa valeur.
- **Identifiant de l'application** : `org.fouryoupdf.desktop` ne change
  plus, il nomme le dossier de ses données
  (`%LOCALAPPDATA%\org.fouryoupdf.desktop`). Modules du dépôt et réglages
  prennent le préfixe `org.fouryoupdf.`.
- **Aucun accès réseau** dans le cœur (noyau, hôte, CLI, application,
  modules natifs), sans réglage pour l'activer (ADR 0006).
- **Permissions de l'application** : toute commande Tauri nouvelle apparaît
  à l'identique dans `COMMANDS` (`app/src/main.rs`),
  `app/permissions/commands.toml` (avec sa raison) et `generate_handler!`,
  est accordée dans `app/capabilities/default.json`, et le test
  `the_window_is_allowed_the_commands_of_the_interface_and_nothing_else`
  est mis à jour. La page ne reçoit aucun plugin `fs` ni `dialog:default`.
- **Interface** (ADR 0004) : une seule fenêtre, pas de modale pour une
  opération courante, aperçu de l'effet avant application, arrêt explicite
  seulement pour l'irréversible. Formulaires traités par `submit` +
  `preventDefault` (`form-action 'none'`, ADR 0007). Aucune couleur en dur
  dans `styles.css`, seulement les rôles existants.
- **Aucun fichier existant remplacé en silence** par une opération qui
  écrit sur le disque.
- Une ligne de logique d'interface testable sans DOM va dans son module
  `app/ui/src/*.ts` avec ses tests `app/ui/tests/*.test.ts`, pas dans
  `main.ts`.

## Commandes

| But | Commande |
|---|---|
| Tests du noyau et des crates | `cargo test -p <crate>` |
| Tests de l'application | `cargo test -p fyp-app` |
| Lint, comme la CI | `cargo clippy -p <crate> --all-targets -- -D warnings` |
| Licences et avis de sécurité, si `Cargo.lock` change | `cargo deny check` |
| Tout l'espace de travail (clôture de rampe ou de palier) | `cargo test --workspace` |
| Types et tests de l'interface | `python tools/build_ui.py` |
| Build de dev (CLI Tauri non installé) | `python tools/build_ui.py` puis `cargo run -p fyp-app` |
| Build release | `cargo build --release -p fyp-app` |
| Inspecter un PDF produit | `cargo run -p fyp-cli -- info <fichier>` |

Fixtures dans `tests/fixtures/` (`mixed12.pdf`, `encrypted-aes256.pdf` avec
le mot de passe `owner`, etc.). Un script de vérification piloté par le port
CDP que tu écris est rangé dans `tools/ui_smoke/` et y reste, pour être
relancé aux sessions suivantes.

## Documents de référence

- `docs/feuille-de-route.md` : jalons et sessions, état du projet.
- `README.md` : présentation du projet.
- `docs/architecture.md` : noyau, opérations, hôte, état du corpus.
- `docs/diagrams.md` : diagramme 1, sens des dépendances entre crates.
- `docs/paliers.md` : rampes et paliers, nommage des versions, règle de
  périmètre, grille de sortie de palier.
- `docs/adr/` : décisions (0003 sécurité des modules, 0004 principes UI,
  0006 réseau, 0007 WebView).
- `docs/backlog-ui.md`, `docs/backlog-technique.md`.
- `app/README.md` : comportement de l'application, section par fonction.
- Moteur de rendu : PDFium. Ne pas rouvrir la question sans nouvelle mesure
  de `tools/render_bench/` (voir `docs/mesure-hayro.md`).

## Format du rapport de fin (`docs/sessions/<id>-rapport.md`)

1. **État des lieux et choix retenus**, avec l'argument en trois points au
   plus et l'écart éventuel avec ce qui a été validé en phase 1.
2. **Commandes et permissions** nouvelles ou modifiées : tableau commande,
   permission, pourquoi.
3. **Fichiers modifiés** : tableau fichier, une phrase. Puis « Hors de mon
   fait » pour tout changement involontaire.
4. **Découpage en commits proposé** : Conventional Commits en anglais,
   fichiers de chacun, un paragraphe « Pourquoi » en français. Chaque commit
   compile et passe ses tests seul.
5. **Lignes ajoutées aux backlogs**, citées, et entrées fermées.
6. **Ce qu'un script a déjà prouvé** : liste des vérifications passées,
   pour que Martin ne les refasse pas.
7. **Checklist manuelle** : cases à cocher, sur le build de dev puis le
   build release, limitées à ce qu'aucun script ne prouve. Laisse une ligne
   « Résultat : » vide sous chaque case, que Martin remplira.
