# ADR 0006 — Fonctionnement local : le cœur ne touche jamais au réseau

**Statut** : accepté — 2026-09 ; précise l'ADR 0003 (permission `network`)
et l'ADR 0004 (présentation des modules)

## Contexte
Le README promet un logiciel « sans envoi de fichiers sur Internet », mais
aucun ADR ne formalise ce principe. L'ADR 0003 prévoit au contraire pour
les modules une permission `network`, limitée à des hôtes exacts et
confirmée par l'utilisateur ; l'hôte la refuse encore au chargement
(`PermissionUnavailable`), mais le format du manifeste la décrit déjà
(`docs/plugin-manifest.md`). Lus chacun de leur côté, ces textes se
contredisent : la fenêtre pourrait promettre que rien ne sort pendant qu'un
module envoie un document, ou un module compter sur un accès que le reste
du produit croit exclu. Il faut trancher avant que l'une ou l'autre lecture
ne s'installe dans le code.

Le réseau peut aussi être demandé par un document : un PDF peut contenir
des liens web, des soumissions de formulaire, du JavaScript ou des
références à des fichiers distants.

État au moment de la décision : aucun code du produit (`crates/`, `app/`,
`plugins/`) n'utilise `std::net` ; aucun client HTTP ou TLS n'est dans
l'arbre des dépendances des trois plateformes de bureau (Tauri 2 ne tire
`reqwest` que pour Android et iOS, afin de relayer le serveur de
développement d'une application mobile) ; la CSP de la fenêtre
(`app/tauri.conf.json`, `default-src 'self'`) interdit à l'interface de
charger une ressource ou d'ouvrir une connexion hors de l'application. Cet
ADR fait de cet état une règle et en fixe la seule exception.

## Décision
1. **Le cœur n'a jamais accès au réseau.** Le noyau et les services
   (`fyp-core`, `fyp-crypto`, `fyp-conformance`), les opérations
   (`ops::*`), l'hôte des modules, la ligne de commande, l'application
   desktop, côté Rust comme interface, et les modules natifs officiels
   (OCR, rendu), qui s'exécutent dans l'hôte sans sandbox, n'ouvrent
   aucune connexion pour leur propre compte. Sans exception, dans tous les
   modes, mode développeur de l'ADR 0003 compris, et sans réglage pour le
   réactiver.
2. **Seul un module WebAssembly peut obtenir le réseau**, qu'il soit tiers
   ou officiel, par la permission `network` de l'ADR 0003 : vers les hôtes
   exacts qu'il déclare dans son manifeste, et rien d'autre. Les
   connexions sont ouvertes par l'hôte lui-même, pour cette exécution
   (ADR 0003, point 2 : une permission n'est jamais un accès direct au
   système). La permission et ses hôtes sont affichés en orange ; elle
   n'est jamais active par défaut et jamais silencieuse.
3. **Le consentement est explicite et ne vaut que pour ce qu'il nomme.**
   Avant la première exécution qui ouvrirait le réseau, l'utilisateur
   confirme activement, selon l'arrêt pour l'irréversible de l'ADR 0004.
   L'accord est gardé pour ce module (son identifiant et l'empreinte de son
   binaire), cette version et ces hôtes exacts ; un changement de l'un
   d'eux le redemande, et l'utilisateur peut le retirer à tout moment. Il
   ne s'étend ni à un autre module ni au logiciel en général : il n'existe
   ni interrupteur « réseau » global, ni « faire confiance à tous les
   modules ».

## Alternatives écartées
- **Aucun réseau, modules compris**, lecture littérale du README : elle
  interdirait des services qu'un utilisateur peut vouloir, comme
  l'horodatage d'une signature par une autorité, alors que la sandbox de
  l'ADR 0003 permet un accès borné, visible et consenti.
- **Un interrupteur global « autoriser le réseau »** : un seul accord
  couvrirait des modules et des hôtes que l'utilisateur n'a jamais vus.
- **Une confirmation à chaque exécution** : répétée, elle devient un
  réflexe, ce que l'ADR 0004 exclut pour l'irréversible. L'accord gardé est
  borné à la place par le module, son binaire, sa version et ses hôtes.

## Conséquences

### Exclu du cœur, définitivement
- **Aucune télémétrie** : ni statistiques d'usage, ni rapport de plantage
  envoyé, ni mesure « anonyme » à activer. L'historique d'usage de
  l'ADR 0004 reste sur la machine ; un rapport de plantage, s'il en existe
  un, est un fichier que l'utilisateur transmet lui-même s'il le veut.
- **Aucune mise à jour qui téléphone à la maison** : l'application ne
  cherche pas si une nouvelle version existe et ne télécharge rien. On la
  met à jour en installant soi-même une version téléchargée, ou par le
  gestionnaire de paquets du système, qui répond de ses propres
  connexions ; le module de mise à jour de Tauri (`tauri-plugin-updater`)
  n'entre pas dans le produit.
- **Aucune fonction en ligne native** : ni IA distante, ni signature ou
  horodatage en ligne, ni collaboration en temps réel, ni stockage, partage
  ou conversion sur un serveur, ni compte. Un tel service ne peut venir que
  d'un module, dans les conditions des points 2 et 3.
- **Rien n'est téléchargé à la demande** : polices, données de langue de
  l'OCR, profils de couleur ou règles de conformité sont livrés avec le
  logiciel, ou installés depuis un fichier choisi par l'utilisateur.
- **Un document ne déclenche jamais de connexion** : soumissions de
  formulaire, JavaScript et références à des fichiers ou ressources
  distants ne sont jamais exécutés ni chargés. Un lien web n'est jamais
  suivi par l'application ; si l'interface propose un jour de l'ouvrir,
  c'est le navigateur du système qui le fait, sur un clic explicite, après
  avoir montré l'adresse.
- **La feuille de route s'y plie.** Le catalogue signé (jalon 1.0,
  ADR 0003, point 5) se consulte et se télécharge hors de l'application,
  qui installe un module depuis un fichier et vérifie sa signature sans
  réseau. Pour PAdES (jalon 0.5), le noyau signe et valide avec ce que
  contiennent le document et la machine ; l'horodatage par une autorité et
  la vérification de révocation en ligne passent par un module WebAssembly
  qui déclare les hôtes du service, et une vérification impossible hors
  ligne est présentée comme telle.
- **Hors du champ de cet ADR** : le moteur web du système (WebView2 sous
  Windows, WebKitGTK sous Linux, WKWebView sous macOS) et le système
  lui-même, mis à jour et réglés par leur éditeur ; les scripts de
  développement (`tools/fetch_*.py`), qui téléchargent outils, PDFium et
  corpus pour construire et tester, et ne font pas partie de ce qui est
  distribué.

### Permis aux modules, dans les garde-fous de l'ADR 0003
- Un module WebAssembly, tiers ou officiel, peut rendre un service qui a
  besoin d'un serveur précis : traduction, horodatage, vérification de
  révocation. Il reste soumis à tout l'ADR 0003 : aucune autorité ambiante,
  limites de temps, de mémoire et de sortie, re-validation de tout document
  renvoyé, document source jamais modifié.
- **Hôtes exacts, tenus par l'hôte.** Le module n'a jamais de socket :
  l'hôte ouvre la connexion à sa place et refuse tout ce qui n'est pas un
  hôte déclaré, redirection vers un autre hôte comprise. Un manifeste sans
  hôte ou avec un joker reste refusé (`BadNetworkHosts`).
- **Un seul code réseau dans le produit** : ce relais de l'hôte. Il ne sert
  qu'une exécution munie de `network` et d'un accord valide ; ni le noyau
  ni l'application ne l'appellent pour eux-mêmes, et c'est le seul endroit
  où une dépendance réseau est admise (voir « Vérification »).
- **Tant que ce relais n'existe pas**, l'hôte refuse au chargement un
  module qui demande `network` (`PermissionUnavailable`), comme au
  jalon 0.2.
- **Pas de réseau sans interface pour consentir.** La ligne de commande n'a
  pas de quoi présenter ni retenir un accord : elle refuse les modules qui
  demandent `network`.
- **Des accords locaux** : gardés dans le profil de l'utilisateur, jamais
  synchronisés, jamais lisibles par un module, comme l'historique d'usage
  de l'ADR 0004.
- **L'empreinte écarte les imposteurs.** Tant que les modules ne sont pas
  signés (ADR 0003, « Limites connues »), un module peut reprendre
  l'identifiant d'un autre ; lier l'accord à l'empreinte du binaire
  l'empêche d'hériter du consentement donné au module qu'il imite.
- **Aperçu** : calculer un aperçu ne déclenche jamais la confirmation
  (ADR 0004) ; un module réseau n'a donc d'aperçu en direct qu'une fois
  l'accord donné, et chaque calcul qui passe par le réseau est signalé
  comme une exécution.

### Ce que l'interface montre
L'utilisateur sait toujours s'il se sert du cœur, qui n'envoie jamais rien,
ou d'un module qu'il a autorisé, dont le réseau est limité et visible.

- **Le cœur sans marque, les modules nommés.** Une action du cœur ne porte
  aucune mention de module. Une action fournie par un module nomme son
  module partout où elle apparaît : palette, barre latérale, menu
  contextuel, panneau d'options.
- **Le réseau toujours en évidence.** L'action d'un module qui détient
  `network` porte en plus la marque orange et ses hôtes, jamais en
  information secondaire : sur ce point, cet ADR précise l'ADR 0004, qui ne
  montre le module d'un réglage qu'au second plan.
- **L'arrêt de consentement** se tient dans le panneau latéral, avec les
  exigences de l'ADR 0004 : la demande ne disparaît pas quand on clique
  ailleurs, seul le bouton qui nomme l'action la lance, et ni Entrée ni la
  fermeture du panneau ne valent accord. Il montre le module, sa version et
  sa provenance (officiel, catalogue, mode développeur), les hôtes exacts et
  les documents qui seront envoyés, et dit que l'accord sera gardé pour
  cette version et ces hôtes, révocable dans les réglages.
- **Pendant l'exécution**, un témoin permanent dans la barre d'état nomme
  le module et les hôtes, tant qu'elle dure ; une exécution sans réseau
  n'en a pas.
- **Les accords se retrouvent** dans la liste unique des réglages
  (ADR 0004) : module, version, hôtes, date. Chacun se retire en un geste,
  effectif dès l'exécution suivante.
- **Des mots justes.** L'interface ne dit jamais « rien ne quitte votre
  machine » sans réserve dès qu'un accord réseau existe : elle dit que le
  cœur n'envoie rien et que tel module envoie à tels hôtes. La promesse du
  README vaut pour le cœur ; sa formulation devra le dire.

### Vérification
- **CSP de la fenêtre** : `default-src 'self'` et `img-src 'self' data:`
  restent la règle, et les assouplir contredirait cet ADR. La CSP ne couvre
  pas la navigation de la fenêtre elle-même : qu'elle soit refusée vers
  toute adresse extérieure est à vérifier et, au besoin, à imposer côté
  Rust.
- **Dépendances** : `deny.toml` interdira les clients HTTP, TLS et
  WebSocket (section `bans`), avec pour seule exception nommée le relais de
  l'hôte, le jour où il existera. Les cibles vérifiées par `cargo deny`
  (les trois plateformes de bureau et wasm32-wasip1) n'en contiennent aucun
  aujourd'hui, et la CI exécute déjà `cargo deny`.
- **Code** : aucun code du produit n'utilise `std::net` ; clippy
  (`disallowed-types`) l'interdira hors du relais.
- **Tests** : le refus au chargement d'un module qui demande `network` est
  déjà testé (`crates/fyp-host/tests/sandbox.rs`) ; le relais aura les
  siens, hôte non déclaré et redirection refusés compris.
