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
- [ ] **Deux vérifications à la main laissées par le palier v0.4.1**
  (consigné le 3 octobre 2026, en condensant le palier v0.4.1 dans
  `docs/feuille-de-route.md`, qui les portait seule depuis le 20 septembre
  2026). Hors du dépôt : installer l'installeur publié de v0.4.0 et
  vérifier son titre et sa version ; annoter la Release v0.3.4 (« les
  fichiers portent 0.3.3 par erreur »). À retirer une fois faites.
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
  dense »). La vue ne peut pas abandonner un rendu dont elle ne veut plus :
  `pdfium-render` n'expose pas le rendu progressif de PDFium, et le seul
  moyen d'interrompre un dessin est d'arrêter le processus de rendu, ce que
  la fenêtre ne fait qu'après 30 s, au-delà du plafond de mémoire ou à la
  fermeture (ADR 0008). La vue s'en tient donc à une demande à la fois,
  200 ms après le dernier cran.
  - [ ] **Réduire le coût du transfert des pixels du processus de rendu à
    la fenêtre** (consigné le 4 octobre 2026, session v0.5.1-B, décision
    de Martin). Mesuré en build release (ADR 0008, « Mesures ») : depuis
    que PDFium tourne dans un processus à part, une page de 1400 pixels
    que PDFium dessine en quelques millisecondes coûte de 4 à 7 ms de plus
    qu'en v0.5.0, une vignette un dixième de milliseconde ; une page lente
    rien de mesurable. Hypothèse non vérifiée : la traversée du tuyau par
    les pixels bruts, 11 Mio pour une page A4 à 1400 pixels. C'est sous le
    critère du palier (10 ms au plus) et bien moins que le PNG puis le
    base64 de l'entrée ci-dessus. Pistes, à profiler d'abord : des
    écritures plus grosses dans le tuyau ; une copie de moins côté
    travailleur (`into_rgba8` puis la trame) ; la lecture directe dans le
    tampon final côté fenêtre ; une mémoire partagée seulement si elle
    vient sans `unsafe` dans notre code ni dépendance que Martin n'a pas
    acceptée.
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
- [ ] **Corriger le renvoi à la section « Protocole des moteurs » de
  `docs/banc-rendu.md`** (consigné le 14 septembre 2026, en ajoutant le moteur
  hayro). `tools/render_bench/engines.toml`, `tools/render_bench/src/protocol.rs`
  et `tools/render_bench/engines/pdfium/src/main.rs` y renvoient, mais la
  section s'appelle « Les moteurs ».
- [ ] **L'application n'affiche pas les champs de formulaire d'un document
  sans `/AcroForm`** (consigné le 14 septembre 2026, par le banc de fidélité,
  PDFium contre hayro). Les annotations `/Widget` de ces documents ont une
  apparence (`/AP /N`), que PDFium ne dessine pas tel que
  `app/src/render/pdfium.rs` l'appelle : pas d'environnement de formulaire
  sans `/AcroForm`, donc pas de dessin des widgets. Vu sur
  `pdfjs/issue12963.pdf`, page 1 (le nom rempli « СУВОРОВ » manque) et
  `qpdf/annotations-no-acroform-with-p.pdf`, page 1 (textes des deux
  champs) ; hayro les dessine. La norme demande de dessiner l'apparence
  d'une annotation visible (ISO 32000-2, 12.5.5), qu'il y ait un formulaire
  ou non.
- [ ] **Vérifier l'interface dans la CI** (consigné le 14 septembre 2026, en
  ajoutant le panneau de vignettes). `ci.yml` ne lance ni
  `tools/fetch_ui_tools.py` ni `tools/build_ui.py` : la vérification des
  types de `app/ui/` (tsgo) et ses tests QuickJS-ng (`app/ui/tests/`) ne
  tournent sur aucun push ni aucune pull request, et `cargo test --workspace`
  y compile l'application avec la page d'attente qu'écrit `app/build.rs`
  quand `app/dist/` manque. Seul `release.yml` les exécute, sur un tag, par
  `tools/package_app.py`.
- [ ] **Restreindre les droits du processus de rendu, reste de « Isoler le
  rendu dans un processus séparé »** (consigné le 14 septembre 2026,
  décision prise hors session ; réduit le 3 octobre 2026, sessions
  v0.5.1-A puis v0.5.1-B). Fait le 3 octobre 2026 (ADR 0008) : PDFium
  tourne dans un processus à part, `fyp-app --fyp-render-worker`, qui ne
  sait que recevoir des octets et rendre des pixels ; un plantage, une pile
  dépassée ou un dessin de plus de 30 s arrêtent ce processus, pas
  l'application, qui dit que la page n'a pas été rendue et le relance ; la
  fenêtre vérifie chaque réponse et encode elle-même le PNG. C'est la forme
  que demandait la deuxième condition de bascule de `docs/mesure-hayro.md`,
  « un rendu qui ne fait ni tomber ni figer l'application », quel que soit
  le moteur qui tourne dans ce processus. Fait en session B : sous Windows,
  un Job Object « tuer à la fermeture » arrête le travailleur avec la
  fenêtre, même tuée pendant un dessin ; un gardien de la fenêtre lit la
  mémoire du travailleur toutes les 50 ms et l'arrête au-delà de 1 Gio
  plus trois fois la taille du document, sous Windows et sous Linux.
  Reste :
  - **ses droits sont ceux de l'utilisateur** : un défaut de PDFium
    exploité ne donne la main que sur ce processus, mais ce processus peut
    encore lire et écrire les fichiers de l'utilisateur et atteindre celui
    de la fenêtre. Chrome, lui, isole PDFium sous son bac à sable. À faire
    (jeton restreint, niveau d'intégrité bas ou espace de noms selon le
    système) avant d'annoncer un rendu sandboxé, ou sur un rapport de
    vulnérabilité de PDFium exploitable ;
  - **les bouts du canal sont héritables le temps du lancement** : un
    processus enfant lancé au même instant par un autre code (WebView2)
    pourrait en hériter, et une demande échouerait alors au bout du délai
    au lieu d'aussitôt (ADR 0008, « Limites connues ») ;
  - **le travailleur dessine l'image avant de la comparer aux plafonds** :
    une page dont l'image dépasserait 4096 × 8192 pixels est d'abord
    allouée, puis refusée. Le gardien borne ce que cela coûte ; refuser
    d'après les dimensions de la page, avant de dessiner, l'éviterait.
- [ ] **Proposer en amont à `win32job` un plafond
  `JOB_OBJECT_LIMIT_PROCESS_MEMORY`, et l'ajouter comme seconde barrière
  quand il existera** (consigné le 3 octobre 2026, session v0.5.1-B,
  décision de Martin). `win32job` 2.0.3 n'expose pas le plafond de mémoire
  engagée d'un Job (`JOB_OBJECT_LIMIT_PROCESS_MEMORY`), seulement un
  plafond de mémoire physique, qui ne tue pas, et l'appel direct
  demanderait `unsafe`. Le plafond de mémoire du travailleur est donc tenu
  par un gardien de la fenêtre, qui lit toutes les 50 ms : entre deux
  lectures, rien ne retient le travailleur (ADR 0008, « Le gardien de la
  mémoire »). Un plafond posé sur le Job
  refuserait l'allocation elle-même. À faire : une demande de fusion en
  amont (`ExtendedLimitInfo::limit_process_memory`), puis, une fois
  publiée, la limite dans `tie` (`app/src/render.rs`), le gardien restant
  pour le seuil qui suit la taille du document.
- [ ] **Borner la mémoire du processus de rendu sous macOS, avant de livrer
  macOS** (consigné le 3 octobre 2026, session v0.5.1-B, décision de
  Martin). `app/src/render/memory.rs` ne sait lire la mémoire d'un
  processus que sous Windows et sous Linux ; ailleurs le gardien ne lit
  rien et le travailleur n'a pas de plafond. `RLIMIT_AS` n'est pas
  appliqué par macOS ; pistes : `proc_pid_rusage` ou `ps -o rss=`, sans
  `unsafe` dans notre code.
- [ ] **Sous Linux, attacher le processus de rendu à la fenêtre, avant de
  livrer l'application sous Linux (bloc E)** (consigné le 3 octobre 2026,
  session v0.5.1-B, décision de Martin). Sous Linux, un travailleur dont la
  fenêtre meurt (plantage, tuée) ne s'arrête qu'en lisant la fin de son
  entrée standard, donc après le dessin en cours, sans borne de temps : la
  fenêtre n'est plus là pour appliquer le délai de 30 s ni le plafond de
  mémoire. Sous Windows, le Job Object l'arrête aussitôt (ADR 0008). À
  faire : `PR_SET_PDEATHSIG`, ou un équivalent, sans `unsafe` dans notre
  code.
- [ ] **Vérifier le gardien de la mémoire sous Linux avec PDFium, avant de
  livrer Linux** (consigné le 3 octobre 2026, session v0.5.1-B). Sous
  Linux, la CI exécute la lecture de `/proc/<pid>/status` sur un vrai
  travailleur sans PDFium et les tests du gardien contre de faux
  travailleurs ; le test qui fait dépasser le plafond à PDFium
  (`a_worker_over_its_ceiling_is_stopped_and_the_next_page_is_drawn`) n'y
  tourne pas, PDFium n'y étant pas téléchargé, et la session n'a été menée
  que sous Windows. Le pic de mémoire d'une page A4 à 4096 pixels n'a pas
  été mesuré sous Linux, où ce qui est compté (`RssAnon` + `VmSwap`) n'est
  pas la mémoire engagée de Windows.
- [ ] **Un `FYP_PDFIUM_DIR` vide fait chercher PDFium dans le dossier
  courant** (consigné le 3 octobre 2026, session v0.5.1-B, à la lecture du
  code ; non reproduit). `library_candidates` (`app/src/render.rs`) prend
  la variable telle quelle : vide, elle donne un chemin relatif, donc
  `pdfium.dll` dans le dossier courant. Depuis la session B, seul un build
  compilé depuis le dépôt lit cette variable ; un paquet l'ignore. À
  faire : ignorer une valeur vide ou relative.
- [ ] **Durcir le chargeur de DLL du processus de rendu sous Windows**
  (consigné le 3 octobre 2026, session v0.5.1-B, hors périmètre du brief).
  `pdfium.dll` est chargée par son chemin complet, mais les bibliothèques
  dont elle dépend sont cherchées par Windows dans l'ordre par défaut, qui
  commence par le dossier de l'exécutable. `SetDefaultDllDirectories`
  (`LOAD_LIBRARY_SEARCH_SYSTEM32`) restreindrait cette recherche ; l'appel
  demande `unsafe`, ou une crate qui le fait pour nous. À décider avec la
  restriction des droits du travailleur.
- [ ] **Neutraliser la surcharge de WebView2 par le registre et les autres
  variables `WEBVIEW2_*`, reste de « garder les outils de développement
  coupés en release »** (consigné le 14 septembre 2026, réduit le
  19 septembre 2026). Fait le
  19 septembre 2026 : un test de `app/src/main.rs` garde la fonctionnalité
  Cargo `devtools` de `tauri` hors de `app/Cargo.toml` et `open_devtools`
  hors de `main.rs`, `render.rs`, `session.rs` et, depuis l'ADR 0008,
  `lib.rs` et les fichiers de `render/` ; en release seulement,
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
  `crates/fyp-cli/src/main.rs` en porte deux de plus (« Milestone 0.1
  commands », « Milestone 0.2 »), relevés le 20 septembre 2026 en ajoutant
  la sélection de pages à la fusion ; la session a laissé cette ligne
  intacte, sa règle de périmètre interdisant la correction de passage.
  La session de documentation ne touchait pas au code de production.
  `docs/architecture.md`, « Feuille de route », garde la clé de lecture de
  ces numéros : à reprendre quand l'un de ces fichiers sera modifié pour
  autre chose.
- [ ] **La lecture par blocs et le budget mémoire de l'ADR 0004**
  (consigné le 20 septembre 2026 comme « … n'ont ni jalon ni bloc », en
  datant les numéros de jalons des ADR ; réduit le 3 octobre 2026). L'ADR
  0004 les voulait avant le jalon 0.3 ; l'application est sortie sans,
  chaque document ouvert gardant son fichier entier en mémoire, deux fois
  avec PDFium (`docs/architecture.md`, « Application desktop »), et l'ADR
  0005 y adosse la reprise du rendu. Depuis la clôture de la rampe v0.5.0,
  ils ont un palier à eux, « Palier mémoire » au § 6 de
  `docs/feuille-de-route.md`, après le bloc A et avant tout travail sur
  plusieurs documents ouverts. Reste à le découper en sessions quand il
  entrera dans l'horizon détaillé.
- [ ] **Deux commits de la rampe v0.4.0 ont un sujet en français**
  (consigné le 20 septembre 2026, en rendant la documentation vraie).
  `acaca26` (« feat(app): suffixer le titre de la fenêtre… ») et `ec7a290`
  (« fix(ui): aligner le numéro des vignettes… »), alors que `CLAUDE.md` et
  `CONTRIBUTING.md` demandent l'anglais pour le code et les commits ;
  git-cliff les reprend tels quels dans les notes de la Release v0.4.0, où
  ils voisinent avec des sujets anglais. Rien ne le vérifie dans la CI.
- [ ] **Un champ de formulaire dont la page part reste dans `/Fields`**
  (consigné le 20 septembre 2026, en ajoutant la sélection de pages à la
  fusion). Quand `ops` laisse tomber une page — suppression, extraction, ou
  désormais une page qu'une sélection de fusion ne prend pas — le widget
  qui était sur elle n'est plus dans aucun `/Annots`, mais reste listé dans
  `/AcroForm /Fields` avec un `/P` à `null` (ISO 32000-2, 12.7.4.1). Rien
  n'est faux et aucun lecteur ne l'affiche : c'est du poids mort, et un
  `/P` qui ne mène nulle part. `clean_dead_destinations` décide déjà sur un
  instantané ; il pourrait retirer ces entrées en descendant l'arbre des
  champs par `/Kids` et en supprimant un nœud devenu sans enfant.
- [ ] **Une page qu'une fusion prend deux fois partage ses annotations**
  (consigné le 20 septembre 2026, en ajoutant la sélection de pages à la
  fusion). `ops::merge_selected` écrit un objet page par occurrence mais
  partage tout ce que la page référence : c'est ce qu'il faut pour le flux
  de contenu et les ressources, c'est discutable pour `/Annots`, une
  annotation appartenant à une seule page (ISO 32000-2, 12.5.2), et le
  `/P` des annotations partagées désigne la dernière copie. qpdf fait la
  même copie superficielle pour `--pages a.pdf 1,1`. À reprendre si un
  lecteur s'en plaint, ou avant les annotations de balisage (bloc B) :
  il faudrait copier en profondeur les annotations d'une occurrence
  répétée.
- [ ] **Deux liens physiques vers le fichier ouvert** (consigné le
  23 septembre 2026, en empêchant l'extraction de le remplacer).
  L'extraction refuse le fichier du document ouvert en comparant les chemins
  rendus canoniques (`fs::canonicalize`), ce qui reconnaît d'autres
  majuscules, un détour par `dossier\..` ou un lien symbolique ; deux liens
  physiques (`mklink /H`) vers un même fichier restent deux chemins : le
  document ouvert par l'un, une extraction vers l'autre remplacerait son
  contenu. Il faudrait l'identité du fichier (volume et index de fichier),
  que la bibliothèque standard ne donne qu'en instable
  (`MetadataExt::volume_serial_number`, `file_index`, fonctionnalité
  `windows_by_handle`), ou une crate comme `same-file`, dépendance nouvelle.
  Cas rare : à reprendre quand ces fonctions seront stables.
- [ ] **Deux modules de l'interface sont commentés en français** (consigné
  le 23 septembre 2026, en choisissant les pages de chaque fichier
  fusionné). Les en-têtes de `app/ui/src/extract.ts` et
  `app/ui/src/split.ts` (rampe v0.5.0, session B) sont en français, alors
  que `CLAUDE.md` veut les commentaires en anglais ; le reste de ces deux
  fichiers l'est.
- [ ] **Le refus d'extraire sur le fichier ouvert cède quand un chemin ne
  se rend pas canonique** (consigné le 23 septembre 2026, en relisant la
  rampe v0.5.0, session C). `same_file` (`app/src/session.rs:462-467`)
  répond « autre fichier » dès que `fs::canonicalize` échoue sur le chemin
  du document ouvert, et l'extraction écrit. Deux cas possibles, non
  vérifiés : un volume où Windows ne rend pas le chemin final d'un fichier
  (disques virtuels ou en mémoire) ; un document ouvert par un chemin
  relatif passé en ligne de commande (`initial_file`,
  `app/src/main.rs:437`), gardé tel quel (`session.rs:214`) et résolu
  contre le répertoire courant du moment de l'extraction, qui a pu
  changer : `rfd` remplace les options du sélecteur par
  `FOS_ALLOWMULTISELECT` ou `FOS_PICKFOLDERS` au lieu de les ajouter
  (rfd 0.16.0, `backend/win_cid/file_dialog/dialog_ffi.rs:319-348`), ce
  qui ôte `FOS_NOCHANGEDIR` s'il est parmi celles par défaut, pour
  `Fusionner…` et `Découper…`. Windows demande toujours avant de
  remplacer, rien n'est donc silencieux, mais le refus que décrit
  `app/README.md` ne tient plus. Piste : garder à l'ouverture
  `std::path::absolute(path)` (stable, MSRV 1.95) et, quand la cible
  existe mais qu'un des deux chemins ne se rend pas canonique, comparer
  les chemins absolus sans tenir compte de la casse, ou refuser.
- [ ] **Enregistrer et extraire écrivent en place** (consigné le
  23 septembre 2026, en relisant la rampe v0.5.0, session C).
  `Session::save`, qu'une extraction appelle aussi, écrit par
  `std::fs::write` directement sur le fichier choisi
  (`app/src/session.rs:240`) : une erreur au milieu (disque plein, support
  retiré) le laisse tronqué, y compris un fichier existant que
  l'utilisateur a accepté de remplacer, ou le fichier du document ouvert
  sous `Enregistrer sous…`. Le document en mémoire reste intact et peut
  être enregistré ailleurs, mais l'ancien contenu du fichier est perdu.
  Piste : écrire dans un fichier temporaire du même dossier, puis le
  renommer par-dessus.
- [ ] **Aucun script de `tools/ui_smoke/` ne couvre l'extraction de la
  sélection ni le découpage** (consigné le 3 octobre 2026, à la clôture de
  la rampe v0.5.0). Le seul script, `merge_pages.py` (session C), pilote la
  fusion par pages ; de l'extraction, il ne vérifie que le refus d'écrire
  sur le fichier ouvert et le remplacement d'un autre fichier, et du
  découpage, que son bandeau cède la place à celui de la fusion. Les
  dix-sept vérifications CDP de l'extraction et du découpage (session B,
  `docs/sessions/v0.5.0-B-rapport.md`, § 6) ont été faites par un script
  que le dépôt ne garde pas : ces deux gestes ne se revérifient qu'à la
  main, par la checklist de la clôture.
- [ ] **`tools/bench_host` retient Wasmtime en 48.0.2** (consigné le
  3 octobre 2026, en montant Wasmtime à 48.0.5 pour cinq avis RustSec).
  Son `Cargo.toml` épingle `wasmparser = "=0.254.0"`, « Wasmtime's
  version », pour que `inspect` lise un module comme le valideur de
  l'hôte ; Wasmtime 48.0.4 et 48.0.5 demandent `wasmparser` 0.254.1 au
  moins, si bien que le banc ne peut pas les prendre. Son `Cargo.lock`
  local, non suivi, reste en 48.0.2 ; une résolution neuve prendrait
  48.0.3, qui corrige RUSTSEC-2026-0315 et -0316 mais reste touchée par
  -0325, -0326 et -0327. Le banc est hors de l'espace de travail et ignoré
  par la CI : rien n'échoue, mais il ne mesure plus la version que l'hôte
  embarque. Épingler la version de `wasmparser` que tire Wasmtime, ou la
  lire de `Cargo.lock`.
- [ ] **Le bac à sable accepte une partie de la proposition GC**
  (consigné le 3 octobre 2026, à la clôture de la rampe v0.5.0, en
  vérifiant Wasmtime 48.0.5). Sans la fonction `gc` de Wasmtime, les types
  `struct`, `array`, `anyref`, `eqref` et `i31ref` déclarés sont refusés au
  chargement, mais un module qui déclare un groupe `rec`, un sous-type
  `sub final`, ou qui exécute `ref.i31`, `i31.get_s` et `ref.eq` se charge
  et tourne : le valideur garde la proposition GC activée. L'ADR 0003 dit
  le GC écarté. Piste : `config.wasm_gc(false)` dans `Host::with_limits`
  (refuse `rec`, `ref.i31`, `i31.get_s`, `ref.eq`, pas `sub final` ; le
  module merge fonctionne toujours) et un test
  `modules_using_gc_exceptions_or_components_are_refused_at_load` dans
  `crates/fyp-host/tests/sandbox.rs` (sondes du testeur,
  `docs/sessions/v0.5.0-cloture-tests.md`, D2).
- [ ] **Les gardes d'écriture ignorent un `startxref` relocalisé**
  (consigné le 3 octobre 2026, à la clôture de la rampe v0.5.0).
  `write_result` et `fyp rewrite` dans la CLI, `Session::save`,
  `Session::split`, `rotate` et la fusion de l'application, `revalidate` de
  l'hôte ne refusent qu'un résultat reconstruit (`reconstructed()`) ; un
  writer qui décalerait `startxref` de moins de 512 octets écrirait un
  fichier « not sound » (`Document::relocated_startxref`) sans un mot. Les
  tests de round-trip de la rampe le voient depuis sa clôture ; le code,
  non. À joindre au palier v0.5.1, session C, qui touche déjà ces
  écritures.
- [ ] **`fyp-host` demande toujours `wasmtime = "48.0.2"`** (consigné le
  3 octobre 2026, en montant Wasmtime à 48.0.5 pour cinq avis RustSec).
  Seul `Cargo.lock` écarte les versions touchées : `cargo update -p
  wasmtime --precise 48.0.2` reviendrait en arrière sans erreur de Cargo
  (`cargo deny` le verrait). Monter l'exigence de
  `crates/fyp-host/Cargo.toml` à `48.0.5` ferait porter la décision par le
  manifeste.
- [ ] **L'en-tête du CHANGELOG dit ses groupes ceux de git-cliff**
  (consigné le 3 octobre 2026, à la clôture de la rampe v0.5.0). « les
  groupes ci-dessous sont les siens » (`CHANGELOG.md`, en-tête), mais
  « Tests » (depuis 0.4.1) et « Sécurité » (0.5.0) ne sont pas des groupes
  de `cliff.toml` : les notes de la Release v0.5.0 rangeront
  `fix(deps): update wasmtime…` sous « Corrections ». Ajouter un groupe
  `Sécurité` à `cliff.toml` (par exemple sur `^fix\(deps\)`) ou
  reformuler l'en-tête.
- [ ] **Le brief v0.5.1-cloture, décision 5, oublie la grille de sortie**
  (consigné le 3 octobre 2026, à la clôture de la rampe v0.5.0). Quand
  `check_tag` exigera un tag égal au workspace, la case « Versions
  cohérentes » de la grille de sortie de `docs/paliers.md`, qui prévoit un
  workspace resté sous le patch du tag (« le dire dans la PR »), deviendra
  sans objet : à réécrire avec « Nommage ». Par ailleurs l'en-tête du brief
  admet `tools/check_version.py` et `.github/workflows/`, tandis que son
  « Hors périmètre » garde « Tout changement de code de production » sans
  exception.
- [ ] **`fyp` remplace sans rien dire un fichier de sortie existant**
  (consigné le 3 octobre 2026, à la clôture de la rampe v0.5.0, par le
  testeur). `fyp run merge … -o x.pdf` lancé deux fois de suite réussit
  les deux fois, la seconde remplaçant la première sans un mot ; `fyp
  merge` et les autres commandes qui passent par `write_result` font de
  même, alors que `CLAUDE.md` veut qu'aucune opération qui écrit sur le
  disque ne remplace un fichier existant en silence. L'écriture atomique
  du palier v0.5.1, session C, rend le remplacement sûr, pas annoncé :
  refuser sans une option explicite (`--force`), ou le dire.
- [ ] **La recherche d'une ligne de commentaire coûte jusqu'à 1 024 octets
  par candidat de la reconstruction, hors budget** (consigné le 3 octobre
  2026, session v0.5.1-recover). `recover::on_comment_line` relit
  `COMMENT_LOOKBACK` octets avant chaque en-tête `n g obj`, sans débit du
  budget du scan. Le coût reste linéaire, mais sa constante dépend de la
  période des en-têtes : 63,2 fois la taille sur `1 0 obj << endobj `
  répété (18 octets), 65,2 fois sur le fichier valide `1 0 obj 1 endobj `
  répété, 116,8 fois sur `%1 0 obj ` répété, mesurés à 3 Mo avec le
  compteur `EXAMINED`. La borne `LINEAR` de 64 du test
  `hostile_megabytes_fail_fast` ne tient donc que pour des motifs dont la
  période dépasse 17 octets environ : le motif du test n'y passe qu'avec
  1,3 % de marge, et le même objet vide écrit plus serré la dépasse
  (`1 0 obj <<endobj ` 66,4, `1 0 obj<<endobj ` 70,1, `1 0 obj endobj `
  74,2, mesures du testeur). Arrêter la marche arrière au premier saut de
  ligne ou au premier `%`, mémoriser le début de la ligne courante d'un
  candidat au suivant, ou débiter ce parcours du budget ; puis ajouter ces
  écritures au test.
- [ ] **Les object streams d'un fichier reconstruit sont tous décodés à
  l'ouverture, sans débit du budget du scan** (consigné le 3 octobre 2026,
  session v0.5.1-recover, testeur D2). `recover::Scan::load_object_stream`
  décode chaque `/Type /ObjStm` sous la seule limite
  `DecodeLimits::max_output` (256 Mio par étage) : un flux de 983 octets
  compressé deux fois coûte 0,33 s, un fichier de 43 Ko à 64 flux 22,6 s à
  `fyp info`. Le coût reste linéaire en la taille du fichier, avec une
  constante de 270 000 par octet. Débiter les octets décodés du budget du
  scan ferait perdre ses objets à un fichier sain dont les object streams
  se compressent beaucoup (de l'ordre de sept fois, estimation non
  mesurée) : le remède est à choisir avec un
  plafond global de décodage par document, car le chemin normal (table
  déclarée) décode ses object streams à la demande et reste exposé à une
  arborescence de pages répartie sur des milliers de flux. La recherche du
  catalogue lit en plus jusqu'à trois fois les données décodées d'un flux
  (recherche de `/Catalog`, analyses sur les coupes, relecture entière) :
  0,77 s au lieu de 0,49 s sur un flux décodé à 200 Mio.
- [ ] **La reconstruction ne reconnaît pas un catalogue rangé dans un
  object stream quand un autre objet y est déclaré dans ses octets et
  qu'un objet listé avant lui dans l'en-tête ne se parse pas non plus sur
  sa coupe** (consigné le 3 octobre 2026, session v0.5.1-recover, testeur
  D7). Ce dernier prend la seule relecture entière du flux
  (`recover::is_catalog_at`, `whole_data_left`), et un fichier sans
  trailer utilisable perd son `/Root` ; avant cette session, le catalogue
  était reconnu. Reproduction : en-tête `2 0 1 2 3 7` sur
  `] << /Type /Catalog /Pages 2 0 R >>`. Ce sont des offsets qui se
  chevauchent, contraires à ISO 32000-2, 7.5.7, et aucun fichier du corpus
  n'est concerné. Correction et test décrits par le testeur
  (`docs/sessions/v0.5.1-recover-tests.md`, tour 2, D7 et U16) : relire
  chaque objet qui échoue sur sa coupe, sur des plages disjointes bornées
  par `Parser::pos()` ; elle suppose que `pos()` après une erreur ne
  recule jamais sous ce que le lexeur a lu, ce qui reste à établir par
  une lecture du lexeur. Test :
  `catalog_cut_after_another_object_that_does_not_parse`.
- [ ] **`fyp info … | head` panique quand le tube se ferme avant la fin de
  la sortie** (consigné le 3 octobre 2026, session v0.5.1-recover,
  testeur). `println!` sur un tube fermé : « failed printing to stdout »
  (os error 232). Écrire par `writeln!` sur `stdout().lock()` et traiter
  `BrokenPipe` comme une fin normale.
- [ ] **`docs/architecture.md` dit le budget du scan « huit fois la taille
  du fichier » sans son plancher de 1 Mio** (consigné le 3 octobre 2026,
  session v0.5.1-recover, relecture). `BUDGET_FLOOR` dans `recover.rs` ;
  `docs/diagrams.md`, ligne 110, parle aussi d'un budget « linéaire en la
  taille du fichier ».
- [ ] **Le banc cite encore `app/src/render.rs` comme le code qu'il
  mesure** (consigné le 3 octobre 2026, session v0.5.1-A).
  `tools/render_bench/engines.toml`, ligne 13 (la description du moteur
  PDFium, reprise par le rapport du banc), le commentaire de
  `tools/render_bench/src/pageset.rs`, ligne 17, celui de tête de
  `tools/render_bench/engines/hayro/src/render.rs`, ligne 3, et celui des
  membres de l'espace de travail, `Cargo.toml`, ligne 13 (« the PDFium
  engine compiles app/src/render.rs »). Depuis l'ADR 0008, le moteur
  compile `app/src/render/pdfium.rs` et `app/src/render/png.rs` ; la
  session ne touchait au banc que pour qu'il compile et que ses tests
  passent.
- [ ] **`docs/mesure-hayro.md` et `docs/feuille-de-route.md` décrivent
  encore le rendu dans le processus de la fenêtre** (consigné le 3 octobre
  2026, session v0.5.1-A). La mesure dit, sous sa deuxième condition de
  bascule, « Dans l'application, le fil de rendu vit dans le processus de
  la fenêtre » ; la feuille de route garde en tête de ses dettes connues
  « PDFium dans le processus principal […] un crash tue l'application ».
  Faux depuis l'ADR 0008 ; le brief de la session ne nommait pas ces deux
  documents. À reprendre à la clôture du palier v0.5.1.
- [ ] **Des démarrages ratés lents relancent le travailleur sans fin**
  (consigné le 3 octobre 2026, session v0.5.1-A, testeur D3). Une poignée
  de main muette coûte 10 s : six relances par minute au plus, sous la
  limite de huit par 60 s (`Limits::default()`, `app/src/render.rs`), et
  les chutes par page ne s'appliquent pas à la poignée de main. Chaque
  vignette d'un grand document lance alors un processus et attend 10 s, le
  rendu restant « disponible ». Piste : compter aussi les échecs de
  démarrage consécutifs, quel que soit leur rythme (trois de suite coupent
  le rendu). Test :
  `handshakes_that_keep_failing_turn_rendering_off_whatever_their_pace`.
- [ ] **Rien ne vérifie `Stdio::null()` ni `CREATE_NO_WINDOW` au lancement
  du travailleur** (consigné le 3 octobre 2026, session v0.5.1-A, testeur
  R2). `Executable::launch`, `app/src/render.rs`. La mutation « sortie
  d'erreur héritée » survit à tous les tests ; seule une sonde release du
  testeur a montré qu'aucune fenêtre n'apparaît. Piste : un script de
  `tools/ui_smoke/` qui énumère les fenêtres du travailleur, en dev et en
  release.
- [ ] **Tester le délai d'une demande contre un vrai dessin sans fin de
  PDFium** (consigné le 3 octobre 2026, session v0.5.1-A, testeur). Le
  délai n'est testé qu'avec de faux travailleurs
  (`a_silent_worker_is_killed_when_the_delay_is_over`). Fixture proposée :
  `tests/fixtures/render-endless-forms.pdf` (formulaires imbriqués, 10⁶
  remplissages, 3,4 Kio ; aujourd'hui `target/agents/v0.5.1-A/endless6.pdf`,
  que `endless.py`, à côté, fabrique). Test :
  `an_endless_drawing_is_stopped_at_the_delay_and_refused_after_two` dans
  `app/tests/render_worker.rs`. Depuis la session v0.5.1-B, ce fichier
  fabrique une page du même genre (`endless_page`, un formulaire de mille
  courbes dessiné mille fois) pour « la fenêtre tuée pendant un dessin ne
  laisse aucun travailleur » : elle peut servir à ce test sans fixture.
- [ ] **Une inondation de réponses non demandées coûte à la fenêtre trois
  images, pas une** (consigné le 3 octobre 2026, session v0.5.1-A, testeur
  R1). Environ 400 Mio au pic : une image dans la file d'une place, une en
  lecture, plus la croissance du tampon de `read_payload`
  (`app/src/render/protocol.rs`). C'est borné, et l'ADR 0008 le dit dans
  ses limites connues. Piste : ne lire la réponse suivante qu'une fois la
  précédente consommée.
- [ ] **Les chutes d'une page repartent de zéro à chaque rotation ou
  fusion** (consigné le 3 octobre 2026, session v0.5.1-A, relecture).
  `Strikes::concern` (`app/src/render.rs`) remet le compte à zéro dès que
  l'identifiant change, et une rotation ou une fusion en donne un nouveau.
  Une page qui arrête le moteur le fera donc tomber deux fois de plus après
  chaque réécriture, là où le brief disait « jusqu'à ce qu'un autre
  document soit ouvert ». C'est documenté (ADR 0008, `app/README.md`) et
  borné par la limite de relances. À décider : garder le compte à travers
  les réécritures d'un même document ouvert.
- [ ] **Le scénario du sélecteur natif de `tools/ui_smoke/merge_pages.py`
  échoue parfois** (consigné le 4 octobre 2026, session v0.5.1-B). Une
  exécution sur trois de la session a donné 40/41 : « the native picker:
  pick_merge_files answers each file with its page count » sans réponse
  (`…`) au bout de 20 s ; les deux suivantes, 41/41, sur le même build. Le
  sélecteur est piloté par des messages de fenêtre (`picker`, `choose`) ;
  la session n'a pas touché à ce chemin. Piste : attendre que la boîte de
  dialogue ait pris le nom tapé avant de valider, et dire dans le détail
  du cas laquelle des étapes a manqué.
- [ ] **Une page de groupes de transparence imbriqués dépasse le plafond de
  mémoire au zoom maximal** (consigné le 4 octobre 2026, session v0.5.1-B,
  testeur, O1). Une page de 4,9 Kio faite de 10 groupes de transparence
  imbriqués, chacun avec un masque doux en luminosité, est tuée par le
  gardien à 4096 pixels de large ; 8 niveaux passent à 980 Mio, juste sous
  1 Gio ; à 1400 pixels, 16 niveaux prennent 192 Mio. D'après la mesure,
  environ 80 Mio par niveau à 4096 pixels ; vraisemblablement un bitmap de
  la page par groupe, ce que rien n'a profilé. Un fichier de mise en page
  honnête, avec des effets imbriqués, peut être refusé au
  zoom maximal alors qu'il passe à la taille de la vue. La limite est
  écrite dans l'ADR 0008 (« Limites connues ») ; le seuil est une décision
  de Martin. À décider : un plafond qui suit aussi la largeur demandée, ou
  une page refusée au zoom maximal puis redemandée plus étroite.
  Fabrication : `target/agents/v0.5.1-B/make_groups.py` (testeur, hors du
  dépôt ; à reprendre en fixture si la question est rouverte).
- [ ] **Ce que lit le gardien sous Windows n'est tenu par aucun test**
  (consigné le 4 octobre 2026, session v0.5.1-B, testeur, O3 ; mutation
  B12 survivante). Lire l'ensemble de travail au lieu de la mémoire
  engagée (`app/src/render/memory.rs`) passe tous les tests : le vrai
  travailleur ne peut pas engager de mémoire sans y écrire sur commande, et
  un test demanderait un faux processus natif.
- [ ] **Des tests du service de rendu échouent sous charge** (consigné le
  4 octobre 2026, session v0.5.1-B, testeur, O4). Sous 16 processus qui
  tournent à vide, deux tests de la session v0.5.1-A, aux délais courts,
  échouent de temps en temps, sur `390eeec` comme sur l'arbre de la
  session : `a_silent_worker_is_killed_when_the_delay_is_over` et
  `a_first_start_that_fails_is_tried_again_at_the_next_request` (sa
  poignée de main de 300 ms expire avant la raison attendue). Mesures :
  les tests de `render::` alternés 15 fois entre `390eeec` et l'arbre de
  la session, 3 échecs de chaque côté ; 25 passes de la relecture sur
  l'arbre de la session, 4 échecs, dont ces deux tests. Une CI sur deux
  cœurs peut les rencontrer. Piste : des délais qui ne dépendent pas
  de l'ordonnanceur (un faux travailleur qui attend un signal du test).
- [ ] **Sous Linux, le gardien peut lire le `/proc/<pid>/status` d'un autre
  processus** (consigné le 4 octobre 2026, session v0.5.1-B, relecture, à
  la lecture du code ; non reproduit). Quand le gardien tue un travailleur,
  `Process::kill` l'attend, ce qui libère son PID ; le processus mort reste
  pourtant dans `Watch::process` jusqu'à la demande suivante, et le
  gardien relit `/proc/<pid>/status` toutes les 50 ms. Si le PID est
  réattribué à un gros processus, il repose `passed` et rappelle un `kill`
  sans effet. À faire : ne plus lire la mémoire d'un processus déjà
  attendu (`guard`, `app/src/render.rs`).
- [ ] **Sous Linux, la mémoire que lit le gardien ignore la mémoire
  partagée anonyme** (consigné le 4 octobre 2026, session v0.5.1-B,
  relecture, à la lecture du code). `RssAnon` + `VmSwap`
  (`app/src/render/memory.rs`) ne compte pas `RssShmem` : un travailleur
  compromis qui écrirait dans un `memfd` ou un tmpfs échapperait au
  plafond, que `VmRSS` aurait compté. À décider avant de livrer Linux :
  ajouter `RssShmem`.
