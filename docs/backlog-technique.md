# Backlog technique

Travaux sans effet visible dans l'application : les garde-fous qui font
tenir les décisions des ADR, et la dette de l'outillage et du dépôt.
Consignés à partir du 12 septembre 2026, chacun avec sa date quand elle
diffère, et la date de sa réduction quand une session en a soldé une part.

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
    extérieure, et sinon l'imposer côté Rust.** La CSP (`default-src 'none'`
    depuis le 19 septembre 2026, ADR 0007) ne régit que les ressources et les
    connexions, pas la navigation de la page elle-même, et Tauri 2.11 n'en
    bloque aucune tant qu'aucun `on_navigation` n'est fourni : à confirmer
    dans la fenêtre réelle, puis à imposer par un `on_navigation` (celui d'un
    plugin couvre aussi la fenêtre déclarée dans `tauri.conf.json`) qui
    n'accepte que l'origine de l'application, `tauri://localhost` sous Linux
    et macOS mais `http://tauri.localhost` sous Windows, si bien que
    l'exemple de la documentation de Tauri, qui ne teste que le schéma
    `tauri`, bloquerait l'application sous Windows.
- [ ] **Tenir les workflows figés et vérifiés** (consigné le 13 septembre
  2026, réduit le 16). Fait le 16 septembre 2026 : actionlint 1.7.12, avec
  shellcheck 0.11.0 pour les scripts `run`, ne signale plus rien sur
  `ci.yml`, `release.yml` et `fuzz.yml`, `.github/actionlint.yaml` écartant
  son seul avis, le `if: false` voulu du job `render-fidelity` ; chaque
  action y est épinglée par SHA de commit, le tag qu'il porte en
  commentaire ; `fuzz.yml` installe cargo-fuzz `=0.13.2 --locked`. Reste :
  - aucun job ne lance actionlint : la vérification ne tient que tant
    qu'on la refait à la main avant de toucher un workflow ;
  - la mise à jour des épinglages. Dependabot (`github-actions`) suit un
    SHA accompagné de son tag en commentaire, mais ses commits ne portent
    pas le `Signed-off-by` que le job `dco` exige de chaque commit d'une
    pull request : l'exempter dans le job, ou signer ses commits à la main,
    est à trancher avant de l'activer ; `dtolnay/rust-toolchain`, sans tag
    de version, se remonte à la main dans tous les cas. Les commits
    épinglés d'`actions/checkout` (v4.4.0), `actions/upload-artifact`
    (v4.6.2), `actions/download-artifact` (v4.3.0) et
    `softprops/action-gh-release` (v2.6.2) déclarent encore Node 20, dont
    GitHub organise la fin ; `actions/checkout` v6.1.0 et
    `actions/upload-artifact` v6.0.0 sont passés à Node 24,
    `actions/download-artifact` v6.0.0 pas encore.
- [ ] **Le job `build` de `release.yml` compile `fyp` pour trois plateformes
  sans rien en attacher à la Release** (consigné le 16 septembre 2026, en
  réparant `release.yml`). Ses trois artefacts `fyp-<cible>` ne vivent que
  dans l'exécution, 90 jours, et aucun document ne promet la ligne de
  commande en téléchargement : la Release n'attache que l'installeur et
  l'archive portable de Windows. Décider : attacher `fyp` aux releases, avec
  ses empreintes et son attestation, ou retirer le job, que le job `test` de
  `ci.yml` double déjà comme compilation de `fyp-cli` sur les trois
  systèmes.
- [ ] **Aucun test automatique ne couvre `tools/check_version.py`** (consigné
  le 17 septembre 2026, en ouvrant `version-check` aux tags de palier). La
  règle des rampes et des paliers, les deux lignées de versions et celle des
  MSRV ne sont vérifiées que par le script lancé sur le dépôt réel, où la
  version du workspace est une seule valeur à la fois : les quinze cas de la
  règle des tags ont été passés par un banc jetable, hors dépôt, qui fabrique
  des workspaces à d'autres versions. La CI n'exécute aucun test Python.
  Pistes : un dossier de cas et un job, ou un `--self-test` dans le script.
  Même manque que pour le reste de `tools/`, dont `build_ui.py` que `ci.yml`
  ne lance pas non plus (« Vérifier l'interface dans la CI », plus bas).
- [ ] **Ce que le fuzz n'atteint pas encore** (consigné le 13 septembre 2026
  comme « Étendre le fuzzing au reste du noyau et du contrat », réduit le
  16). Depuis le 16 septembre 2026, `fuzz.yml` lance chaque nuit six cibles
  (`fuzz/README.md`). `document` ouvre des octets arbitraires par
  `Document::open`, amorcée par `tests/fixtures` et `tests/corpus` : tables
  et flux de références croisées, chaîne `/Prev`, object streams,
  reconstruction, `/Encrypt` et fyp-crypto avec le mot de passe vide puis
  celui des fixtures ; elle lit chaque objet, décode chaque flux, parcourt
  l'arbre des pages, appelle `ops::rotate` et `ops::merge`, écrit dans les
  deux styles et relit chaque sortie. `filters` appelle les filtres et les
  prédicteurs seuls, `plugin_api` lit et valide un manifeste et décode les
  deux messages de l'échange, `host_wasi` entre dans la CI ; `parse_object`
  et `quick_info` restent. Ne sont toujours pas atteints, ou pas
  directement :
  - `ops::extract_pages`, `ops::delete_pages` et `ops::split`, que la cible
    n'appelle pas ; ils partagent le `Builder` de `merge` et `rotate`, ce
    qui ne vaut pas couverture ;
  - `fyp-crypto` appelé seul, avec des clés, des sels et des vecteurs
    d'initialisation arbitraires : ses primitives ne tournent que sur les
    dictionnaires `/Encrypt` que le fuzz dérive des amorces ;
  - les règles de `fyp-conformance`, qui n'existent pas ;
  - l'hôte hors des fonctions WASI : découverte des dossiers de modules,
    chargement d'un `module.wasm` hostile, dont la validation est celle de
    Wasmtime.

  Deux limites de l'exécution en CI : ce qu'une nuit trouve n'est pas gardé
  pour la suivante, chaque nuit repartant des seules amorces (un cache par
  `actions/cache`, action à épingler, y remédierait) ; et aucune mesure de
  couverture ne dit quelles lignes les amorces et le fuzz atteignent, le
  prédicteur TIFF sur 16 bits compris, que `filters` peut désormais
  appeler directement (`docs/verification-differentielle.md`).
- [ ] **Le rendu d'une page agrandie passe par un PNG en base64, lourd et
  lent** (consigné le 16 septembre 2026, en posant le zoom de la vue d'une
  page). Mesuré par script sur une page A4 de texte et de lignes, build de
  développement, `render_page` de bout en bout (PDFium, PNG, base64, IPC) :
  0,1 s à 600 pixels de large, 0,4 s à 1400, 2,1 s à 4096, le plafond du
  moteur, où l'URL de données fait 6,9 millions de caractères pour un PNG de
  5,2 Mio, et où le bitmap RGBA de PDFium, 4096 × 5800 pixels, occupe
  95 Mio. Le geste de zoom reste immédiat, l'image affichée étant étirée en
  attendant (`app/README.md`, « Zoom »), mais le rendu net arrive après plus
  de deux secondes au plafond, et `viewer.ts` garde jusqu'à cinq images de
  ce genre, une par page à deux positions de la page affichée, à la plus
  grande largeur demandée, sans mesure de la mémoire réellement occupée.
  Pistes, à mesurer : répondre en octets bruts (`tauri::ipc::Response`)
  plutôt qu'en base64 ; un encodage plus rapide que PNG, ou un bitmap sans
  encodage ; ne garder d'une page quittée que son image de page entière ;
  un rendu par tuiles, qui lèverait aussi le plafond de 4096 pixels
  (`docs/backlog-ui.md`, « Le plafond du zoom vient vite sur un écran
  dense »). Un rendu ne peut pas être interrompu : `pdfium-render` n'expose
  pas le rendu progressif de PDFium, et la vue s'en tient à une demande à
  la fois, 200 ms après le dernier cran.
- [ ] **Activer le signalement privé des vulnérabilités sur le dépôt, avant
  la première release publique** (consigné le 13 septembre 2026, réduit le
  16). Choix fait le 16 septembre 2026 : le signalement privé de GitHub
  plutôt qu'une adresse, et `SECURITY.md` et `MAINTAINERS.md` sont écrits
  pour lui. Ce qu'ils disent n'est vrai qu'une fois le réglage activé
  (Settings, Code security, « Private vulnerability reporting »), ce que
  seul un administrateur du dépôt peut faire : l'API
  `private-vulnerability-reporting` répondait encore `"enabled": false` le
  16 septembre 2026. Vérifier ensuite que « Report a vulnerability »
  apparaît dans l'onglet Security, puis retirer cette entrée.
- [ ] **`CHANGELOG.md` : les versions d'avant 0.4.0, et qui l'écrit**
  (consigné le 13 septembre 2026 comme « Générer `CHANGELOG.md` ou le
  retirer », réduit le 17). Depuis le 17 septembre 2026, le fichier est
  rédigé à la main, en français, une section par version à partir de la
  0.4.0, d'après `git log` depuis le tag précédent ; `release.yml` continue
  de produire les notes de chaque Release par git-cliff (`--latest
  --strip header`), depuis les sujets des commits, sans écrire le fichier.
  Reste : les 17 versions taguées avant, v0.0.1 à v0.3.4, n'ont pas de
  section, et rien ne vérifie qu'une section précède chaque tag.
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
- [ ] **Neutraliser la surcharge de WebView2 par le registre et les autres
  variables `WEBVIEW2_*`, reste de « garder les outils de développement
  coupés en release »** (consigné le 14 septembre 2026, réduit le
  19 septembre 2026). Fait le
  19 septembre 2026 : un test de `app/src/main.rs` garde la fonctionnalité
  Cargo `devtools` de `tauri` hors de `app/Cargo.toml` et `open_devtools`
  hors de `main.rs`, `render.rs` et `session.rs` ; en release seulement,
  `main()` retire `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` de son
  environnement avant que WebView2 ne démarre : le port de débogage que
  cette variable ouvrait sur `target/release/fyp-app.exe` est fermé, mesuré
  avant et après. Reste, non neutralisé : le registre
  (`Software\Policies\Microsoft\Edge\WebView2\AdditionalBrowserArguments`,
  sous `HKLM` puis `HKCU`, valeur nommée d'après l'AppId : AUMID, puis nom
  de l'exécutable, puis `*`), dont WebView2 ajoute les arguments à ceux de
  l'application ; `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER`, qui charge un runtime
  depuis un dossier arbitraire ; `WEBVIEW2_USER_DATA_FOLDER`, qui déplace le
  profil et écraserait le dossier `data` d'une copie portable. À traiter pour
  une distribution signée, ou sur un rapport de vulnérabilité qui les vise.
- [ ] **Décider si `tauri` garde sa fonctionnalité Cargo par défaut
  `dynamic-acl`** (consigné le 19 septembre 2026, en durcissant la WebView).
  Elle permet d'ajouter des capabilities à l'exécution, sans usage ici.
- [ ] **Faire tenir la règle des couleurs par l'outillage** (consigné le
  15 septembre 2026, en posant la palette ambre). `docs/couleurs.md`
  n'admet de valeur de couleur que dans le premier bloc `:root` de
  `app/ui/styles.css`, et de couleur brute que dans le bloc des rôles, mais
  rien ne le vérifie : la session l'a contrôlé par un script jetable. Un
  contrôle dans `tools/build_ui.py` refuserait, hors de ces blocs, un code
  hexadécimal, une fonction de couleur (`rgb()`, `hsl()`…), un nom de
  couleur ou le `var()` d'une couleur brute, ainsi qu'un `var()` qui ne
  nomme rien. Il n'agirait sur les pushes qu'une fois `build_ui.py` lancé
  par `ci.yml` (« Vérifier l'interface dans la CI », plus haut).
- [ ] **Accorder la date du relevé de corpus entre le README et
  `docs/architecture.md`** (consigné le 17 septembre 2026, en traduisant le
  README et `CONTRIBUTING.md` en anglais). Les deux annoncent les mêmes
  chiffres, 4 529 fichiers, 4 472 round-trips, 44 refus et 13 échecs, mais
  le README les date du 13 septembre 2026 et `docs/architecture.md` du 12 :
  l'une des deux dates est fausse, et rien ne les tient ensemble. À trancher
  en relançant le parcours du corpus, qui redonnera la date et vérifiera du
  même coup que les chiffres tiennent toujours.
- [ ] **Les commentaires du code nomment encore les « jalons » de la
  première feuille de route** (consigné le 20 septembre 2026, en rendant la
  documentation vraie). `crates/fyp-core/src/ops.rs` (« Page operations
  (milestone 0.2) ») et `app/src/main.rs` (« desktop application
  (milestone 0.3, first step) ») portent des numéros que
  `docs/feuille-de-route.md` ne connaît pas ; `parser.rs` et `writer.rs`
  parlent d'un « next milestone » et d'un « a later milestone » sans numéro.
  La session de documentation ne touchait pas au code de production.
  `docs/architecture.md`, « Feuille de route », garde la clé de lecture de
  ces numéros : à reprendre quand l'un de ces fichiers sera modifié pour
  autre chose.
- [ ] **La lecture par blocs et le budget mémoire de l'ADR 0004 n'ont ni
  jalon ni bloc** (consigné le 20 septembre 2026, en datant les numéros de
  jalons des ADR). L'ADR 0004 les voulait avant le jalon 0.3 ; l'application
  est sortie sans, chaque document ouvert gardant son fichier entier en
  mémoire, deux fois avec PDFium (`docs/architecture.md`, « Application
  desktop »). `docs/feuille-de-route.md` ne les place nulle part, et l'ADR
  0005 y adosse la reprise du rendu. À cadrer avec l'isolation du rendu
  (palier v0.5.1), qui déplace l'une des deux copies.
- [ ] **Deux commits de la rampe v0.4.0 ont un sujet en français**
  (consigné le 20 septembre 2026, en rendant la documentation vraie).
  `acaca26` (« feat(app): suffixer le titre de la fenêtre… ») et `ec7a290`
  (« fix(ui): aligner le numéro des vignettes… »), alors que `CLAUDE.md` et
  `CONTRIBUTING.md` demandent l'anglais pour le code et les commits ;
  git-cliff les reprend tels quels dans les notes de la Release v0.4.0, où
  ils voisinent avec des sujets anglais. Rien ne le vérifie dans la CI.
