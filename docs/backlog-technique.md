# Backlog technique

Travaux sans effet visible pour l'utilisateur, consignés le 12 septembre
2026 : les garde-fous qui font tenir les décisions des ADR. Rien n'est
commencé.

- [ ] **Faire tenir l'ADR 0006 par l'outillage.** L'ADR 0006 réserve le
  réseau au relais de l'hôte qui sert un module autorisé, mais seules la CSP
  de la fenêtre et l'absence actuelle de code réseau le garantissent, sans
  qu'aucun contrôle de la CI n'empêche que cela change.
  - [ ] **Bannir les clients HTTP, TLS et WebSocket dans `deny.toml`, hors
    du crate qui implémente le relais.** La section `bans` n'interdit encore
    aucun crate et aucun client de ce genre n'est dans l'arbre des cibles que
    vérifie `cargo deny` (Tauri 2 ne tire `reqwest` et `hyper` que pour
    Android et iOS), donc l'interdiction peut entrer tout de suite ; le
    relais, qui n'existe pas encore, gagnera à vivre dans un crate à lui
    plutôt que dans tout `fyp-host`, car l'exception (`wrappers`) ne tolère
    que des dépendants directs nommés, et sa liste exacte dépendra du client
    qu'il choisira.
  - [ ] **Refuser `std::net` hors de ce même crate.** Aucun code du produit
    ne l'utilise ; un `clippy.toml`, absent du dépôt, qui liste ses types et
    fonctions dans `disallowed-types` et `disallowed-methods` en ferait une
    erreur dans la CI, qui passe déjà clippy avec `-D warnings`, seul le
    crate du relais portant l'autorisation, ce qui résiste mieux qu'une
    recherche textuelle aux alias d'import.
  - [ ] **Vérifier que la fenêtre ne peut pas naviguer vers une adresse
    extérieure, et sinon l'imposer côté Rust.** La CSP `default-src 'self'`
    ne régit que les ressources et les connexions, pas la navigation de la
    page elle-même, et Tauri 2.11 n'en bloque aucune tant qu'aucun
    `on_navigation` n'est fourni : à confirmer dans la fenêtre réelle, puis
    à imposer par un `on_navigation` (celui d'un plugin couvre aussi la
    fenêtre déclarée dans `tauri.conf.json`) qui n'accepte que l'origine de
    l'application, `tauri://localhost` sous Linux et macOS mais
    `http://tauri.localhost` sous Windows, si bien que l'exemple de la
    documentation de Tauri, qui ne teste que le schéma `tauri`, bloquerait
    l'application sous Windows.
- [ ] **Vérifier et figer les workflows, en première étape de la session
  consacrée au workflow de release** (consigné le 13 septembre 2026).
  - [ ] **Passer les workflows à actionlint.** Aucun outil n'a validé
    `release.yml` : un parseur YAML ne vérifierait que sa syntaxe, alors
    qu'actionlint vérifie aussi les expressions de GitHub (`${{ … }}`,
    `needs`, sorties des étapes) et les scripts `run`. Le faire avant de
    compléter le workflow, pour ne rien bâtir sur une erreur qui ne se
    verrait qu'au premier tag poussé. Les jobs `msrv-product` et
    `msrv-plugin-api` de `ci.yml`, ajoutés le même jour, n'ont pas été
    validés non plus.
  - [ ] **Épingler chaque action par SHA de commit.** Un projet qui vérifie
    le SHA-256 de `pdfium.dll` ne peut pas laisser un tag ou une branche
    choisir le code qui tourne dans sa CI. `ci.yml` n'utilise plus de
    branche : `dtolnay/rust-toolchain` y est épinglé au commit `02cb101e`
    du 12 septembre 2026, le toolchain passant par l'entrée `toolchain`.
    Restent sur une référence mobile les branches
    `dtolnay/rust-toolchain@stable` (`release.yml`, 2 fois) et
    `dtolnay/rust-toolchain@nightly` (`fuzz.yml`), et les tags
    `actions/checkout@v4` (`ci.yml` 6 fois, `release.yml` 4, `fuzz.yml` 1),
    `Swatinem/rust-cache@v2` (`ci.yml`, 4 fois),
    `EmbarkStudios/cargo-deny-action@v2` (`ci.yml`),
    `actions/upload-artifact@v4` (`release.yml` 2 fois, `fuzz.yml` 1),
    `actions/download-artifact@v4`, `orhun/git-cliff-action@v4`,
    `actions/attest-build-provenance@v2` et `softprops/action-gh-release@v2`
    (`release.yml`). Prévoir en même temps leur mise à jour : Dependabot
    (`github-actions`) suit un SHA accompagné de son tag en commentaire,
    mais `dtolnay/rust-toolchain`, qui n'a pas de tag de version, se
    remonte à la main. Même famille : `fuzz.yml` installe `cargo-fuzz` sans
    version ni `--locked`.
- [ ] **Faire couvrir par le fuzzing ce que le README annonce, dans la
  session consacrée au fuzz** (consigné le 13 septembre 2026). Le README
  présente « un noyau en Rust (`#![forbid(unsafe_code)]`), fuzzé en
  continu », l'ADR 0003 range le « fuzzing continu » parmi les défenses en
  profondeur, et la feuille de route promet au jalon 0.1 un « fuzzing sans
  crash ». `fuzz.yml` tourne bien chaque nuit, mais ses deux cibles ne
  voient qu'une petite partie du noyau : `parse_object`, le lexer et le
  parseur d'un objet isolé (`parse_object`, `parse_indirect`), et
  `quick_info`, l'en-tête puis la recherche de `startxref`, `/Encrypt` et
  `%%EOF` dans les 2 derniers Kio, sans construire de document. La
  troisième cible, `host_wasi` (les fonctions WASI de l'hôte confrontées à
  un modèle de référence), n'est pas dans la CI (ADR 0003, « Limites
  connues »). Aucune cible n'atteint :
  - l'ouverture d'un document (`Document::open`) : tables et flux de
    références croisées, chaîne `/Prev`, `startxref` décalé, flux d'objets ;
  - la reconstruction d'une table inutilisable (`recover`) ;
  - les filtres `FlateDecode`, `ASCIIHexDecode`, `ASCII85Decode` et
    `RunLengthDecode`, les prédicteurs TIFF et PNG, et les limites de
    décodage ;
  - le chiffrement : la lecture de `/Encrypt` (`encryption`) et tout
    `fyp-crypto` (dérivation des clés, RC4, AES) ;
  - l'écriture (`writer`) et les opérations de pages (`ops`) ;
  - les règles de `fyp-conformance` ;
  - dans le contrat des modules, la lecture de `manifest.toml` et le
    décodage des échanges (`fyp_plugin_api::exchange`), que l'hôte applique
    pourtant à ce que produisent des modules hostiles.

  Constaté en vérifiant les corrections de lints du MSRV 1.95
  (`docs/verification-differentielle.md`) : ces zones ne s'atteignent qu'à
  partir de `Document::open`, et même une cible qui l'appelait, amorcée par
  `tests/fixtures` et `tests/corpus`, n'a jamais exercé le prédicteur TIFF
  sur 16 bits.
