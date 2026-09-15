# Backlog technique

Travaux sans effet visible dans l'application : les garde-fous qui font
tenir les décisions des ADR, et la dette de l'outillage et du dépôt.
Consignés à partir du 12 septembre 2026, chacun avec sa date quand elle
diffère ; seul l'épinglage des actions par SHA est entamé (`ci.yml`).

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
    `actions/checkout@v4` (`ci.yml` 7 fois, `release.yml` 4, `fuzz.yml` 1),
    `Swatinem/rust-cache@v2` (`ci.yml`, 5 fois),
    `EmbarkStudios/cargo-deny-action@v2` (`ci.yml`),
    `actions/upload-artifact@v4` (`ci.yml` 1 fois, `release.yml` 2,
    `fuzz.yml` 1),
    `actions/download-artifact@v4`, `orhun/git-cliff-action@v4`,
    `actions/attest-build-provenance@v2` et `softprops/action-gh-release@v2`
    (`release.yml`). Prévoir en même temps leur mise à jour : Dependabot
    (`github-actions`) suit un SHA accompagné de son tag en commentaire,
    mais `dtolnay/rust-toolchain`, qui n'a pas de tag de version, se
    remonte à la main. Même famille : `fuzz.yml` installe `cargo-fuzz` sans
    version ni `--locked`.
- [ ] **Étendre le fuzzing au reste du noyau et du contrat, dans la session
  consacrée au fuzz** (consigné le 13 septembre 2026). Le README présentait
  « un noyau en Rust (`#![forbid(unsafe_code)]`), fuzzé en continu » ; il
  dit depuis le 13 septembre 2026 ce qui est fuzzé et ce qui ne l'est pas,
  comme `SECURITY.md`. L'ADR 0003 range toujours le « fuzzing continu »
  parmi les défenses en profondeur, et la première feuille de route
  promettait au jalon 0.1 un « fuzzing sans crash ». `fuzz.yml` tourne bien
  chaque nuit (trois exécutions, toutes réussies, du 11 au 13 septembre
  2026), mais ses deux cibles ne voient qu'une petite partie du noyau :
  `parse_object`, le lexer et le parseur d'un objet isolé (`parse_object`,
  `parse_indirect`), et
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
- [ ] **Réduire les permissions de la fenêtre à ce que l'interface utilise,
  dans la session de durcissement de la WebView** (consigné le 13 septembre
  2026). `app/capabilities/default.json` accorde aux scripts de la page
  `core:default` et `dialog:default` : les ensembles par défaut de
  `core:app`, `core:event`, `core:image`, `core:menu`, `core:path`,
  `core:resources`, `core:tray`, `core:webview` et `core:window`, et les
  dialogues `open`, `save` et `message` du plugin. L'interface n'appelle
  pourtant que ses propres commandes, écoute le dépôt de fichiers
  (`core:event`) et demande `setTitle`, que ces ensembles n'accordent pas
  (`docs/backlog-ui.md`) ; les sélecteurs de fichiers sont appelés depuis
  Rust. Relevé dans `app/gen/schemas/acl-manifests.json`, que génère
  tauri-build.
- [ ] **Ouvrir un canal privé de signalement des vulnérabilités, avant la
  première release publique** (consigné le 13 septembre 2026).
  `SECURITY.md` demande de ne pas ouvrir d'issue publique, mais
  `MAINTAINERS.md` n'indique aucune adresse (« à compléter ») et le
  signalement privé de GitHub est désactivé sur le dépôt (API
  `private-vulnerability-reporting` : `"enabled": false`). Choisir l'un ou
  l'autre, puis mettre `SECURITY.md` à jour.
- [ ] **Générer `CHANGELOG.md` ou le retirer** (consigné le 13 septembre
  2026). Le fichier ne contient aucune entrée alors que 16 tags existent :
  `release.yml` n'appelle git-cliff que pour les notes de la release en
  cours (`--latest --strip header`), et rien n'écrit le fichier. Décider
  s'il est produit et commité avant chaque tag, ou si les notes des releases
  suffisent.
- [ ] **Empêcher un build de développement en release de charger une
  `pdfium.dll` restée dans `target/release/`** (consigné le 13 septembre
  2026, en préparant le moteur PDFium du banc de fidélité). Il y en a une,
  identique aujourd'hui à `app/pdfium/pdfium.dll` (SHA-256 `04100c03…`, même
  date de modification) ; qu'elle vienne de l'empaquetage, qui copie les
  ressources de `tauri.bundle.json`, n'a pas été vérifié.
  `library_candidates` (`app/src/render.rs`) cherche à côté de l'exécutable
  avant `app/pdfium/` : après un changement de la version épinglée par
  `tools/fetch_pdfium.py`, `cargo run --release -p fyp-app` chargerait encore
  cette copie. Le banc, lui, ne cherche que dans `FYP_PDFIUM_DIR` puis dans
  `app/pdfium/`.
- [ ] **Préciser dans quel build l'encodage PNG coûte plus que le rendu
  d'une page** (consigné le 13 septembre 2026, par le banc de fidélité).
  `docs/architecture.md` (« Rendu ») affirme que, pour une page à la taille de
  la fenêtre, « le temps passe dans l'encodage PNG, pas dans PDFium ». La
  mesure que cite le commentaire de `app/src/render.rs` a été prise dans un
  build debug : 190 ms pour une page de 1400 pixels. En release, sur les 156
  pages comparées du jeu de référence à 1400 pixels, l'encodage prend 5,3 %
  du temps de rendu et d'encodage : 213 ms contre 3 779 ms, 11,6 ms au plus
  pour une page (`docs/banc-rendu.md`, « Temps de référence »). Or
  l'application empaquetée est un build release.
- [ ] **Corriger le renvoi à la section « Protocole des moteurs » de
  `docs/banc-rendu.md`** (consigné le 14 septembre 2026, en ajoutant le moteur
  hayro). `tools/render_bench/engines.toml`, `tools/render_bench/src/protocol.rs`
  et `tools/render_bench/engines/pdfium/src/main.rs` y renvoient, mais la
  section s'appelle « Les moteurs ».
- [ ] **L'application n'affiche pas les champs de formulaire d'un document
  sans `/AcroForm`** (consigné le 14 septembre 2026, par le banc de fidélité,
  PDFium contre hayro). Les annotations `/Widget` de ces documents ont une
  apparence (`/AP /N`), que PDFium ne dessine pas tel que `app/src/render.rs`
  l'appelle : pas d'environnement de formulaire sans `/AcroForm`, donc pas de
  dessin des widgets. Vu sur `pdfjs/issue12963.pdf`, page 1 (le nom rempli
  « СУВОРОВ » manque) et `qpdf/annotations-no-acroform-with-p.pdf`, page 1
  (textes des deux champs) ; hayro les dessine. La norme demande de dessiner
  l'apparence d'une annotation visible (ISO 32000-2, 12.5.5), qu'il y ait un
  formulaire ou non.
- [ ] **Vérifier l'interface dans la CI** (consigné le 14 septembre 2026, en
  ajoutant le panneau de vignettes). `ci.yml` ne lance ni
  `tools/fetch_ui_tools.py` ni `tools/build_ui.py` : la vérification des
  types de `app/ui/` (tsgo) et ses tests QuickJS-ng (`app/ui/tests/`) ne
  tournent sur aucun push ni aucune pull request, et `cargo test --workspace`
  y compile l'application avec la page d'attente qu'écrit `app/build.rs`
  quand `app/dist/` manque. Seul `release.yml` les exécute, sur un tag, par
  `tools/package_app.py`.
- [ ] **Mettre à jour le commentaire de tête de `app/src/render.rs`**
  (consigné le 14 septembre 2026, en ajoutant le panneau de vignettes). Il
  dit que l'interface ne met rien devant la page affichée parce que
  « thumbnails wait while the page view is open ». Depuis le panneau, les
  vignettes attendent que la vue n'ait plus rien à dessiner, puis passent
  une à une tant que le panneau est affiché (`thumbnailSlots`, dans
  `app/ui/src/thumbnails.ts`) : une page demandée en naviguant peut attendre
  une vignette. La session du panneau excluait toute modification de
  `render.rs`.
- [ ] **Isoler le rendu dans un processus séparé** (consigné le 14 septembre
  2026, décision prise hors session). Un processus à mémoire et à temps
  bornés, qui ne sait que recevoir des octets et rendre des pixels. Le rendu
  s'exécute aujourd'hui dans le processus de la fenêtre, sur le thread de
  `app/src/render.rs` : un PDF malveillant qui exploite un défaut de PDFium a
  la main sur l'application, et un moteur qui dépasse sa pile l'emporte avec
  lui, comme hayro sur `qpdf/issue-202.pdf` (`docs/mesure-hayro.md`). Chrome,
  lui, isole PDFium dans un processus séparé, sous son bac à sable. Quel que
  soit le moteur, l'isolation apporte :
  - un défaut exploité ne donne la main que sur ce processus, pas sur celui
    de la fenêtre et ses commandes, pour peu que le système restreigne ses
    droits comme le bac à sable de Chrome restreint les siens : un processus
    qui garde les droits de l'utilisateur peut encore atteindre celui de la
    fenêtre ;
  - un plantage, une pile dépassée ou une mémoire épuisée arrêtent ce
    processus, pas l'application, qui peut dire que la page n'a pas été
    rendue ;
  - un dessin trop long s'abandonne en arrêtant le processus, ce qu'un
    thread ne permet pas ;
  - la mémoire du rendu a sa propre limite, distincte de celle de la
    fenêtre.

  Elle répond à l'une des trois conditions de bascule vers hayro de
  `docs/mesure-hayro.md`, la deuxième, « un rendu qui ne fait ni tomber ni
  figer l'application », dont la mesure juge la meilleure forme un processus
  à part ou la sandbox WebAssembly, où hayro compile.
- [ ] **Garder les outils de développement coupés en release par la
  configuration de Tauri, dans la session de durcissement de la WebView**
  (consigné le 14 septembre 2026, en neutralisant les raccourcis du
  navigateur). L'interface neutralise F12 et Ctrl+Maj+I, J et C dans tous les
  builds (`app/README.md`, « Raccourcis du navigateur neutralisés »), mais ce
  n'est pas elle qui coupe les outils de développement en release : c'est une
  valeur par défaut. wry pose `devtools: false` hors `debug_assertions`, et
  tauri-runtime-wry 2.11 ne le change qu'en debug ou avec la fonctionnalité
  Cargo `devtools` de `tauri`, que `app/Cargo.toml` n'active pas. Rien dans
  le dépôt ne fixe ni ne vérifie cette absence. Le levier est la
  fonctionnalité Cargo, pas `tauri.conf.json` : la clé `devtools` d'une
  fenêtre n'y agit en release qu'avec cette fonctionnalité (documentation de
  `tauri-utils`), et la mettre à `false` couperait les outils des builds de
  développement, où le clic droit les ouvre. Dans ces builds, Tauri ajoute
  aussi à la page un script qui ouvre les outils sur Ctrl+Maj+I, par la
  position de la touche et quels que soient les autres modificateurs, par la
  commande `plugin:webview|internal_toggle_devtools`, qu'accorde
  `core:default` : l'interface arrête ces touches, et refuser cette commande
  dans les permissions de la fenêtre (`core:webview:deny-internal-toggle-devtools`)
  le ferait sans dépendre de la façon dont le script les reconnaît. À
  vérifier au même moment : si
  `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=…`, par
  lequel les essais pilotent l'application, ouvre aussi ce port sur une copie
  release.
- [ ] **Imposer `eol=lf` aux fichiers de l'interface dans `.gitattributes`**
  (consigné le 14 septembre 2026, en neutralisant les raccourcis du
  navigateur). `.gitattributes` fixe les fins de ligne des `*.rs`, `*.toml`
  et `*.md`, pas celles des `*.ts`, `*.html` et `*.css` de `app/ui/`. Avec
  `core.autocrlf=true`, Git avertit à chaque modification de ces fichiers, en
  LF dans l'index comme dans la copie de travail (`git ls-files --eol`),
  que « LF will be replaced by CRLF the next time Git touches it ».
