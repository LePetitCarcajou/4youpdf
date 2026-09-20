# Backlog de l'interface

**Exclu par principe** (ADR 0006) : toute fonctionnalité du cœur qui
enverrait le contenu d'un document vers un service distant, comme une IA
dans le nuage, la signature en ligne ou la collaboration en temps réel. Le
cœur (noyau, opérations, hôte, ligne de commande, application, modules
natifs) n'a jamais accès au réseau, sans réglage pour l'activer ; un tel
service ne peut venir que d'un module WebAssembly qui déclare ses hôtes
exacts et obtient l'accord de l'utilisateur, et n'a donc pas sa place dans
ce backlog. Les garde-fous qui font tenir cette règle sont suivis dans
`docs/backlog-technique.md`.

Demandes pour l'application desktop (`app/`), consignées le 12 septembre
2026 sauf mention contraire ; plusieurs supposent d'abord du travail dans le
noyau. Relevé du 13 septembre 2026, à la version 0.3.3 : aucune n'est faite
ni commencée. Chaque point sera cadré au moment de le prendre, et sortira de
la liste une fois fait.

- [ ] **Recherche de texte dans le document.** Le noyau ne lit pas encore le
  contenu des pages : extraire le texte suppose d'interpréter les flux de
  contenu et de retrouver l'Unicode des glyphes par les polices et
  `/ToUnicode` (ISO 32000-2, 9.10), un chantier du noyau à cadrer et à
  tester comme un morceau à part, avant l'interface de recherche. Ctrl+F, F3
  et Ctrl+G attendent sa commande : l'interface empêche déjà leur action par
  défaut dans la page et les laisse libres (`app/README.md`, « Raccourcis du
  navigateur neutralisés »).
- [ ] **Imprimer le document** (consigné le 14 septembre 2026). L'application
  n'imprime rien. Ctrl+P attend cette commande : l'interface empêche son
  action par défaut dans la page et laisse la touche libre (`app/README.md`,
  « Raccourcis du navigateur neutralisés ») ; le menu contextuel de WebView2,
  qu'un build de développement garde, propose encore d'imprimer l'interface
  (« Neutraliser le menu contextuel par défaut de WebView2 », plus bas) ; en
  release ce menu ne s'ouvre plus.
- [ ] **Attribuer les touches neutralisées selon les conventions communes**
  (consigné le 14 septembre 2026). Quand elles recevront des commandes de
  l'application, se caler sur les conventions partagées par Acrobat, pdf.js
  et les navigateurs : Ctrl+F, puis F3 ou Ctrl+G, pour la recherche ;
  Ctrl+plus, Ctrl+moins et Ctrl+0 pour le zoom ; Ctrl+P pour l'impression.
  Pas sur les raccourcis propres à Acrobat, qui se chevauchent d'un outil à
  l'autre et dépendent d'une préférence
  ([raccourcis clavier d'Acrobat](https://helpx.adobe.com/fr/acrobat/desktop/get-started/preferences-and-settings/keyboard-shortcuts.html)).
  Ctrl+plus, Ctrl+moins et Ctrl+0 ont reçu le zoom de la vue d'une page le
  16 septembre 2026 (`app/README.md`, « Zoom ») ; Ctrl+F, F3, Ctrl+G et
  Ctrl+P attendent encore (`app/README.md`, « Raccourcis du navigateur
  neutralisés »).
- [ ] **Neutraliser le menu contextuel par défaut de WebView2** (consigné le
  14 septembre 2026, réduit le 19 septembre 2026). Hors des vignettes, un clic
  droit ouvrait le menu de WebView2, dont « Actualiser », qui rechargerait
  l'interface et perdrait le travail non enregistré, et « Imprimer », qui
  imprimerait l'interface. Fait en release par le script d'initialisation de
  `app/src/main.rs` (`NO_NATIVE_CONTEXT_MENU`), en phase de capture : ni le
  clic droit, ni la touche Menu, ni Maj+F10 n'ouvrent plus le menu natif, et
  celui des vignettes reste. Un build de développement garde le menu natif à
  dessein, « Inspecter » y ouvrant les outils de développement (ADR 0007).
  Reste, en release, où les champs de texte (mot de passe du bandeau, numéro
  de page de la vue) n'ont plus de menu couper/copier/coller mais gardent
  Ctrl+X/C/V : un menu d'édition de l'application, si le besoin apparaît.
- [ ] **Barre d'annotation sur une sélection de texte** (surligner,
  souligner, barrer, copier), qui apparaît au clic-glisser sur du texte,
  comme dans PDF24. C'est l'interaction de PDF24 que retient l'ADR 0004
  (point 2, actions contextuelles), mais la page affichée n'est aujourd'hui
  qu'une image rendue par PDFium : il faudra la position de chaque
  caractère, donc l'extraction du point « Recherche de texte dans le
  document », et des annotations de balisage de texte écrites par le noyau
  (ISO 32000-2, 12.5.6.10).
- [ ] **Ajustement automatique à la largeur ou à la hauteur de la page**,
  comme dans un navigateur, et zoom sans pourcentage affiché. La vue
  n'ajuste aujourd'hui que la page entière à la fenêtre ; depuis le
  16 septembre 2026, elle sait agrandir la page par paliers et la faire
  défiler à la molette avant de tourner la page (`app/README.md`,
  « Zoom ») : l'ajustement à la largeur serait un palier de plus, calculé
  d'après la fenêtre, à tenir d'une page à l'autre et au redimensionnement.
- [ ] **Aucun repère de position dans une page agrandie** (consigné le
  16 septembre 2026, en posant le zoom). La vue rogne la page au cadre et
  rien ne dit quelle part de la page est visible ni où l'on est : ni barre
  de défilement, ni vignette de position. Une barre native serait claire sur
  le fond sombre de la vue et resterait au moteur (voir « L'anneau de focus
  reste celui du moteur web ») ; un repère dessiné par l'interface est à
  concevoir.
- [ ] **Le plafond du zoom vient vite sur un écran dense** (consigné le
  16 septembre 2026, en posant le zoom). Le plafond est l'image de 4096
  pixels d'appareil que le moteur dessine au plus (`app/README.md`,
  « Zoom ») : 959 % pour une page A4 dans une fenêtre de 1100 × 760 à
  l'échelle 100 %, mais autour de 290 % pour la même page sur un écran 4K à
  l'échelle 200 %, où la page entière occupe déjà 1400 pixels d'appareil. Il
  dépend de la densité de l'écran, pas du détail qu'on veut regarder. Le
  lever passe par le chemin du rendu (`docs/backlog-technique.md`, « Le
  rendu d'une page agrandie passe par un PNG en base64 »).
- [ ] **L'indication des touches de la vue est tronquée dès la taille
  d'ouverture de la fenêtre** (consigné le 16 septembre 2026, en posant le
  zoom). Dans la barre de la vue, le texte qui nomme les touches est coupé
  par des points de suspension : à 1100 px de large, la taille d'ouverture,
  panneau affiché, il dispose de 334 px pour 968 px de texte (mesuré par
  DevTools) et ne dit plus que « ← → ou molette : page précédente ou
  suiv… ». C'était déjà le cas avant le zoom, qui a ajouté trois boutons à
  la barre et une mention au texte. À reprendre : un texte plus court, une
  seconde ligne, ou une aide sur demande.
- [ ] **Mode plein écran pour la lecture.** La vue d'une page garde
  volontairement visibles la barre d'outils, les bandeaux (fichier réparé ou
  chiffré) et la barre d'état, et Échap y ramène à la grille : le plein
  écran devra dire où passent ces avertissements et comment Échap se partage
  entre sortir du plein écran et quitter la vue.
- [ ] **OCR : reconnaissance du texte des pages scannées.** Priorité
  confirmée par plusieurs comparatifs d'éditeurs PDF ; la feuille de route
  le repousse en 1.x (`docs/feuille-de-route.md`, § 4 et § 7), comme module
  natif côté hôte (Tesseract ; ADR 0003, « Conséquences », qui réserve les
  traitements lourds à des modules officiels natifs), et non dans
  `fyp-core`, qui n'embarque ni code natif ni `unsafe` et compile pour
  wasm32-wasip1 ; rien n'est commencé, ni
  le chargement des modules natifs, ni l'écriture par le noyau de la couche
  de texte invisible (ISO 32000-2, 9.3.6).
- [ ] **Rédaction : masquage irréversible de zones sensibles** (texte ou
  image), utile en contexte professionnel. Un rectangle noir ou une
  annotation `/Redact` (ISO 32000-2, 12.5.6.23) ne retire rien : il faut que
  le noyau supprime des flux de contenu le texte situé sous la zone et
  efface ses pixels dans les images, ce qui suppose l'interprétation du
  contenu du point « Recherche de texte dans le document » et des filtres
  d'image encore absents, puis une vérification du fichier produit par
  ré-extraction de son texte.
- [ ] **Filigrane, numérotation des pages, rognage.** Opérations à ajouter à
  `fyp_core::ops`, qui ne sait aujourd'hui que fusionner, extraire,
  découper, faire pivoter et supprimer : rogner revient à écrire `/CropBox`
  (ISO 32000-2, 14.11.2), tandis que filigrane et numéros imprimés (à
  distinguer des étiquettes `/PageLabels`, 12.4.2) ajoutent aux pages du
  contenu dessiné avec une police, ce qu'aucune opération du noyau ne fait
  encore, avec l'aperçu en direct que l'ADR 0004 exige pour leurs réglages.
- [ ] **Bug : le titre de la fenêtre ne prend pas le nom du document**
  (consigné le 13 septembre 2026). À l'ouverture d'un fichier, `main.ts`
  demande le titre « nom — 4YouPDF » par `setTitle`, mais
  `capabilities/default.json` n'accordait alors que `core:default`, dont
  `core:window:default` permet de lire le titre sans le changer (il y
  faudrait `core:window:allow-set-title`) : la demande est refusée, et
  `main.ts` ignore le refus. Seule la barre d'outils nomme le document.
  Constaté par script : lancée avec `minimal.pdf` en argument, la fenêtre
  s'appelle encore « 4YouPDF » quinze secondes plus tard. Depuis le
  17 septembre 2026, un build de développement suffixe ce titre de
  « — DEV » côté Rust (`window_title`, `app/src/main.rs`) : la correction
  devra garder le suffixe quand le nom du document entrera dans le titre.
  Depuis le 19 septembre 2026, `capabilities/default.json` n'accorde plus
  `core:default` et, à dessein, pas `core:window:allow-set-title` (ADR 0007,
  capabilities minimales) : la correction devra l'ajouter à ce fichier et à
  la liste des permissions qu'attend le test
  `the_window_is_allowed_the_commands_of_the_interface_and_nothing_else` de
  `app/src/main.rs`.
- [ ] **Bug : les champs remplis d'un formulaire sans `/AcroForm` ne
  s'affichent pas** (consigné le 14 septembre 2026, par le banc de fidélité).
  PDFium contre hayro. Les annotations `/Widget` de ces documents ont une
  apparence (`/AP /N`), que PDFium ne dessine pas tel que `app/src/render.rs`
  l'appelle : pas d'environnement de formulaire sans `/AcroForm`, donc pas de
  dessin des widgets. Vu sur `pdfjs/issue12963.pdf`, page 1 (le nom rempli
  « СУВОРОВ » manque) et `qpdf/annotations-no-acroform-with-p.pdf`, page 1
  (textes des deux champs) ; hayro les dessine. La norme demande de dessiner
  l'apparence d'une annotation visible (ISO 32000-2, 12.5.5), qu'il y ait un
  formulaire ou non.
- [ ] **Les flèches de la grille ne regardent ni Ctrl ni Alt** (consigné le
  14 septembre 2026, en neutralisant les raccourcis du navigateur). La
  branche des flèches du clavier de `main.ts` ne teste que Maj : Alt+← et
  Alt+→, que l'interface neutralise comme raccourcis du navigateur,
  déplacent la sélection comme ← et → (constaté par script, DevTools), et
  Ctrl+← et Ctrl+→ aussi (lu dans le code), alors que « Rotation »
  (`app/README.md`) les dit libres pour déplacer le focus sans changer la
  sélection.
- [ ] **Ctrl+O et Ctrl+S sont sans effet quand le focus est dans un champ**
  (consigné le 14 septembre 2026, en neutralisant les raccourcis du
  navigateur). Dans le champ du mot de passe d'un fichier chiffré ou dans le
  numéro de page de la vue, le clavier de `main.ts` s'arrête avant eux dès
  que la cible est un champ : ni Ouvrir… ni Enregistrer sous… (constaté par
  script, DevTools). D'après la documentation de WebView2, ces deux
  raccourcis y sont toujours coupés.
- [ ] **F6 et Maj+F6 restent hors des raccourcis neutralisés** (consigné le
  14 septembre 2026, en neutralisant les raccourcis du navigateur). La
  documentation de WebView2 les dit actifs en hébergement fenêtré, celui de
  wry (« Focus Next Pane », « Focus Previous Pane » : passer d'un volet à
  l'autre). Envoyé comme message de fenêtre, F6 arrive à la page sans effet
  visible par script, ni rechargement ni nouvelle fenêtre, avant comme après
  le filtre ; Maj+F6 et un déplacement du focus n'ont pas été vérifiés. À
  essayer au clavier avant de décider s'ils rejoignent la liste
  (`app/README.md`, « Raccourcis du navigateur neutralisés »).
- [ ] **Aucun écran ne donne la version ni le commit du build** (consigné
  le 14 septembre 2026 comme « Rien ne distingue à l'écran un build de
  développement d'un build empaqueté », réduit le 17). Depuis le
  17 septembre 2026, un build de développement porte « — DEV » à la fin du
  titre de sa fenêtre, posé côté Rust (`window_title`, `app/src/main.rs`),
  et un build empaqueté non (`app/README.md`, « Construire et lancer »).
  Reste la version : aucun écran ne la donne, et deux builds de la même
  version, empaquetés à des jours différents, ne se distingueraient pas
  plus par elle, qui est la même dans leurs propriétés de fichier ; il y
  faudrait aussi le commit ou la date de compilation, dans la barre d'état
  ou un écran « À propos ».
- [ ] **L'icône laisse des coutures translucides, et sa plaque de fond n'est
  pas transparente** (consigné le 16 septembre 2026, en mettant à jour la
  documentation de l'icône posée la veille). Dans `app/icons/icon.svg`, le
  papier ambre et les coups de griffe sont deux tracés qui se touchent :
  lissé chacun de son côté, leur bord commun ne couvre pas tout à fait, et
  ce qu'il y a derrière l'icône s'y voit. Mesuré dans les PNG livrés, sans
  être regardé à l'écran : 12,8 % des pixels de `32x32.png`, la taille de la
  barre des tâches, ont une opacité inférieure à 250 sur 255, jusqu'à 191 ;
  3,6 % dans `128x128.png` et 1,2 % dans `icon.png` (512 × 512). Leur
  couleur, entre l'ambre et le brun, et leur place le long des griffes
  désignent bien ce bord commun. Par ailleurs, hors des angles arrondis,
  l'icône est peinte en `#f7f8f8` plutôt que laissée transparente : elle
  porte une plaque blanc cassé, qui se verra sur un fond sombre.
- [ ] **Contrastes sous les seuils de WCAG 2.1** (consigné le 15 septembre
  2026, en posant la palette ambre). Calculés, pas estimés, pour 4,5 : 1 sur
  du texte et 3 : 1 sur un contour ; tous étaient déjà sous le seuil avec
  l'ancienne palette, dont la valeur suit entre parenthèses. Deux paires ont
  été corrigées le jour même, le bord des champs de texte (`--field-border`,
  3,23 : 1 au plus bas) et l'accent sur le fond de la vue (3,01 : 1,
  `docs/couleurs.md`). Restent, sans correction décidée :
  - `--border`, bord des boutons et des vignettes : 1,37 : 1 sur
    `--surface-raised`, 1,31 : 1 sur `--surface` (1,36 et 1,24). Dans la
    barre d'outils, un bouton a le fond de la barre, et seul ce bord le
    délimite ; son libellé le nomme ;
  - le fond du champ de mot de passe contre son bandeau, 1,19 : 1 (1,09) :
    le champ est délimité par son bord, à 3,23 : 1 ;
  - l'anneau de la vignette sélectionnée, `--accent-soft` sur `--surface` :
    1,06 : 1 (1,15). Il double la bordure `--accent`, à 4,92 : 1, qui porte
    l'état seule : pas de correction proposée.

  Choix, et non défaut : le texte d'attente des vignettes (« … », « aperçu
  indisponible ») reste en `--border`, à 1,37 : 1 (1,36). C'est un signe
  transitoire sur une vignette qui va se remplir, discret exprès ;
  `--text-muted` le porterait à 5,93 : 1.
- [ ] **L'anneau de focus reste celui du moteur web** (consigné le
  15 septembre 2026, en posant la palette ambre). La feuille ne dessine
  aucun style de focus pour les boutons ni pour le champ de mot de passe :
  Chromium y met son anneau par défaut, dont la couleur calculée, `#101010`
  sur le champ de mot de passe, ne suit ni les rôles ni `color-scheme: dark`
  (mesuré par DevTools, `docs/couleurs.md`, « Thème sombre »). La sélection
  de texte dans les champs et les barres de défilement de la grille restent
  aussi au moteur. Un anneau aux couleurs de l'application, que demandera un
  thème sombre, passe par une règle de focus qui changerait la forme de
  l'anneau, pas seulement sa couleur : hors de la session des couleurs.
   
- [ ] **`Enregistrer sous…` ne fait pas du fichier écrit le document
  ouvert** (consigné le 15 septembre 2026, en posant la question avant de
  perdre des modifications). Après un enregistrement sous un autre nom, la
  barre d'outils nomme toujours le fichier d'origine, désormais sans marque
  « — modifié » : ce que l'enregistrement écrirait est sur le disque, dans
  l'autre fichier, et rien ne serait perdu en fermant. Mais le fichier
  d'origine, lui, ne contient pas ce qui est affiché, et le prochain
  `Enregistrer sous…` propose encore `origine-modifié.pdf` plutôt que le
  fichier écrit. Les éditeurs font du fichier écrit le document courant ;
  ici, cela demande de le rouvrir, avec ses bandeaux, sans perdre
  l'historique (`app/README.md`, « Modifications non enregistrées »).
- [ ] **La question avant de perdre des modifications ne liste qu'un
  document** (consigné le 15 septembre 2026, en la posant). L'ADR 0004 veut
  qu'avec plusieurs documents modifiés la fermeture les liste ; l'application
  n'en ouvre qu'un aujourd'hui, et `notices.ts` ne garde qu'une question à la
  fois, qui nomme ce document. À reprendre avec les documents multiples.
- [ ] **Demander en place le mot de passe d'un fichier à fusionner**
  (consigné le 17 septembre 2026, en ajoutant la fusion depuis la fenêtre).
  `Fusionner…` ouvre chaque fichier sans mot de passe : un fichier protégé
  est ignoré avec un bandeau qui renvoie à l'ouvrir seul, l'enregistrer en
  clair, puis fusionner ce fichier. Le bandeau de mot de passe de
  `notices.ts` ne sert que l'ouverture (`requestOpen`) ; il faudrait un
  bandeau du même genre qui relance la fusion du seul fichier protégé avec
  le mot de passe tapé, `merge_documents` prenant alors un mot de passe par
  fichier.
- [ ] **Choisir les pages de chaque fichier fusionné** (consigné le
  17 septembre 2026, en ajoutant la fusion). Toutes les pages de chaque
  fichier viennent, à la fin de la grille ou devant une page (« Fusionner
  ici… ») ; on supprime ensuite à la main celles qu'on ne voulait pas.
  `ops::merge` prend des documents entiers, mais son `Builder` travaille
  déjà sur des paires (document, page) : une fonction publique qui prend une
  sélection par document suffirait au noyau, `fyp merge` pourrait l'exposer
  aussi.
- [ ] **Plusieurs fichiers déposés d'un coup : seul le premier s'ouvre**
  (consigné le 17 septembre 2026, en ajoutant la fusion). `main.ts` ouvre
  le premier `.pdf` déposé et ignore les autres sans un mot. Avec la fusion
  dans la fenêtre, deux lectures se défendent : ouvrir le premier et
  fusionner les suivants, ou, sur un document déjà ouvert, fusionner tous
  les fichiers déposés au lieu de remplacer le document. L'ADR 0004 (« les
  opérations multi-documents portent sur les documents déjà ouverts »)
  tranchera avec les documents multiples ; d'ici là, dire au moins que les
  autres fichiers ont été ignorés.
- [ ] **Les raisons du noyau sont en anglais dans les bandeaux** (consigné
  le 17 septembre 2026, en ajoutant la fusion). « Impossible d'ouvrir … :
  missing or malformed %PDF header », « table des objets reconstruite …
  (cross-reference error at byte 64: object 1 0 announced here, found
  2 0) » : le texte de `fyp_core::Error` et la raison d'une reconstruction
  sont écrits pour le développeur, en anglais, et l'interface les cite tels
  quels, à l'ouverture comme à la fusion. Il faudrait ou bien des messages
  du noyau traduisibles par un code (`Error` porte déjà des variantes,
  `reconstructed()` une chaîne libre), ou bien une table côté interface
  pour les cas fréquents, avec le texte anglais en détail.
