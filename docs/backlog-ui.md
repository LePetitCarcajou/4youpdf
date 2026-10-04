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
- [ ] **Les parties d'un découpage ne se nomment pas comme celles de la
  ligne de commande** (consigné le 22 septembre 2026, en ajoutant le
  découpage à la fenêtre). `fyp split` écrit `<nom>-001-003.pdf`, d'après la
  première et la dernière page de la partie dans le fichier ; la fenêtre
  écrit `<nom>_partie-01.pdf`, numéroté à partir de 1, parce que ses parties
  sont des tranches de l'ordre affiché, où une page a pu être déplacée ou
  supprimée : les numéros du fichier n'y décriraient plus rien. Les deux
  schémas se défendent chacun de son côté, mais un même produit devrait
  nommer pareil ; à trancher, avec le choix du nom par l'utilisateur.
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
  apparence (`/AP /N`), que PDFium ne dessine pas tel que
  `app/src/render/pdfium.rs` l'appelle : pas d'environnement de formulaire
  sans `/AcroForm`, donc pas de dessin des widgets. Vu sur
  `pdfjs/issue12963.pdf`, page 1 (le nom rempli « СУВОРОВ » manque) et
  `qpdf/annotations-no-acroform-with-p.pdf`, page 1 (textes des deux
  champs) ; hayro les dessine. La norme demande de dessiner l'apparence
  d'une annotation visible (ISO 32000-2, 12.5.5), qu'il y ait un formulaire
  ou non.
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
- [ ] **Une extraction peut remplacer le fichier tout juste enregistré**
  (consigné le 23 septembre 2026, en relisant la rampe v0.5.0, session C).
  Après `Enregistrer sous…` vers `B.pdf`, le document compte comme
  enregistré (`history.saved`, `app/ui/src/main.ts:1267`) ; une extraction
  vers `B.pdf`, acceptée dans la question de Windows, le remplace par les
  seules pages extraites, et la fenêtre tient toujours le travail pour
  enregistré : la fermer ne demande rien, et les modifications ne sont
  plus nulle part sur le disque. Même défaut que celui corrigé en
  session C pour le fichier ouvert, que seul `Session::path` couvre
  (`app/src/session.rs:257-266`). Lu dans le code, pas reproduit. Piste :
  que le côté Rust retienne aussi le dernier fichier enregistré et le
  refuse de même, ou que l'interface marque le document modifié quand une
  extraction écrit sur ce fichier.
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
- [ ] **Fusionner dans un nouveau fichier** (consigné le 23 septembre 2026,
  en choisissant les pages de chaque fichier fusionné). `Fusionner…` ajoute
  les pages choisies à la grille, comme une modification que Ctrl+Z annule,
  et c'est `Enregistrer sous…` qui écrit le résultat. Le brief de la rampe
  v0.5.0, session C, décrivait l'autre geste : un fichier neuf écrit sans
  toucher au document ouvert, à son historique ni à son point
  d'enregistrement, les pages du document ouvert désignées par leur numéro
  dans l'ordre affiché ; l'ajout à la grille a été retenu
  (`docs/sessions/v0.5.0-C-rapport.md`). S'il est voulu, ce geste écrirait
  par `ops::merge_selected` un fichier neuf, jamais sur le fichier du
  document ouvert (comme l'extraction), et demanderait une commande de plus.
  À cadrer avec les documents multiples de l'ADR 0004 (point 8), où la
  fusion porte sur des documents ouverts.
- [ ] **Dupliquer une page ; une page tapée deux fois à la fusion**
  (consigné le 23 septembre 2026, en choisissant les pages de chaque fichier
  fusionné). `fyp merge --pages 3,3` prend deux fois la page 3 ; le bandeau
  de fusion de la fenêtre refuse une page tapée deux fois (« la page 3 est
  demandée deux fois »), et le côté Rust aussi. Vérifié le 23 septembre
  2026 : `ops::merge_selected` écrit alors deux objets page qui partagent
  leur contenu, leurs ressources et leurs annotations
  (`docs/backlog-technique.md`, « Une page qu'une fusion prend deux fois
  partage ses annotations ») ; tourner l'un laisse l'autre, et extraire les
  deux fonctionne. La grille les montrerait comme deux pages ajoutées,
  indépendantes. Ce qu'elle ne sait pas faire, c'est tenir une même page
  du document deux fois dans l'ordre : `history.ts` et le cache des
  vignettes sont indexés par page du fichier, une rotation tournerait les
  deux, et l'enregistrement la refuse (`ops::extract_pages`, « selected
  more than once »). Dupliquer une page passerait donc par le côté Rust,
  une copie de la page ajoutée au document en mémoire comme une fusion ;
  lever alors le refus du bandeau alignerait la fenêtre sur la ligne de
  commande.
- [ ] **Deux refus de la lecture des pages sont maladroits** (consigné le
  23 septembre 2026, en alignant la fenêtre sur la ligne de commande). Un
  numéro vide dans une plage (`-3`, `3-`, ou `1-` en cours de frappe dans le
  bandeau de fusion) est cité vide, deux espaces entre les guillemets :
  « numéro de page invalide : «  » » ; un document d'une page est « le
  document a 1 page, numérotées de 1 à 1 ». Les deux messages viennent de
  `parse_pages` (`crates/fyp-cli/src/main.rs`), que la fenêtre reprend mot
  pour mot, et `tests/fixtures/page-ranges.tsv` les fixe pour les deux
  lecteurs : les reprendre ensemble, la table de cas d'abord.
- [ ] **Dire ce qu'une fusion ne reprend pas d'un fichier** (consigné le
  23 septembre 2026, en choisissant les pages de chaque fichier fusionné).
  `ops::merge_selected` ne garde que le catalogue du document ouvert
  (`docs/architecture.md`, « Pertes connues ») : d'un fichier fusionné, la
  structure balisée (`/StructTreeRoot`, l'accessibilité de ses pages) et
  les calques (`/OCProperties`, dont la configuration se perd, un calque
  masqué risquant de s'afficher, à vérifier dans ISO 32000-2, 8.11, avant
  d'écrire le message) ne sont pas repris, sans que la fenêtre le dise.
  Recommandation de la session C : un bandeau ciblé, seulement quand un
  fichier fusionné porte l'une de ces deux structures, jamais un
  avertissement à chaque fusion ; `/PageLabels`, `/Metadata` et les
  préférences d'affichage n'en valent pas un. Le côté Rust peut le savoir
  sans toucher au noyau (`Document::catalog`) au moment où le sélecteur
  compte les pages.
- [ ] **L'aperçu d'une fusion ne voit pas un fichier changé depuis son
  choix** (consigné le 23 septembre 2026, en relisant la rampe v0.5.0,
  session C). Le bandeau compte les pages de chaque fichier quand il est
  choisi (`session::candidates`) ; la fusion relit le fichier et vérifie
  les listes tapées (`session::selection`), mais pas un champ vide ni un
  fichier dit ignoré, envoyés tous deux `null` (`app/ui/src/merge.ts:76`).
  Un fichier remplacé entre-temps par un autre de plus de pages est pris
  en entier, au-delà du nombre annoncé ; un fichier protégé ou illisible
  au choix, lisible à la fusion, est fusionné en entier alors que le
  bandeau le disait ignoré. Piste : envoyer pour chaque fichier le nombre
  de pages vu au choix, ou « ignoré », et refuser la fusion quand le côté
  Rust trouve autre chose, comme pour une liste hors limites.
- [ ] **Échap ferme les bandeaux de fusion et de découpage quand il visait
  autre chose** (consigné le 23 septembre 2026, en relisant la rampe
  v0.5.0, session C). `app/ui/src/main.ts:1722-1733` : Échap ferme le menu
  contextuel, arrête un glisser, et ferme toujours les deux bandeaux de
  réglage. Qui presse Échap pour fermer le menu d'une vignette, ou dans le
  champ d'un mot de passe demandé par un autre bandeau, perd les fichiers
  choisis et les listes tapées de la fusion, ou le réglage du découpage.
  Piste : qu'Échap ne ferme qu'une chose à la fois, la plus récente (menu,
  glisser, puis bandeau), ou seulement le bandeau qui a le clavier.
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
- [ ] **Les messages du noyau numérotent les pages à partir de 0**
  (consigné le 23 septembre 2026, en relisant la rampe v0.5.0, session C ;
  réduit le 3 octobre 2026 de sa part sur le rendu).
  `fyp pages extract mixed12.pdf 3,3` répond « invalid page operation:
  page 2 selected more than once » (`crates/fyp-core/src/ops.rs:307`) :
  l'utilisateur a tapé 3, le noyau cite l'indice 2 ; `fyp pages delete` et
  `fyp pages rotate` avec `3,3` disent de même (vérifié le 23 septembre
  2026). La fenêtre passe le texte tel quel (`AppError::from`,
  `app/src/main.rs:60-66`) : un enregistrement, une extraction, une
  rotation ou une partie d'un découpage qui recevrait une page deux fois
  afficherait « Enregistrement impossible : invalid page operation: page 2
  selected more than once » (lu dans le code ; l'interface n'envoie pas de
  doublon aujourd'hui). Les autres erreurs de `ops` qui citent une page :
  `Error::NoSuchPage`, « no page at index {index}: the document has
  {count} page(s) » (`crates/fyp-core/src/lib.rs:172`), levée par
  `check_bounds` (`ops.rs:291`, donc `extract_pages`, `delete_pages`,
  `rotate`, `merge_selected` et `split`) et à la construction
  (`ops.rs:462`), que la ligne de commande n'atteint pas (`parse_pages`
  refuse d'abord, en français et à partir de 1) et la fenêtre seulement
  sur une erreur de l'interface ; « empty page range {start}..{end} » de
  `ops::split` (`ops.rs:263`), à partir de 0 et fin exclue, qu'aucune des
  deux n'atteint. Hors de `ops`, le même défaut dans les messages de
  rendu (« page {page} hors de portée », « rendu de la page {page} : … »),
  affichés par la vue d'une page, est corrigé depuis le 3 octobre 2026
  (session v0.5.1-A, `app/src/render/pdfium.rs`) : ils citent les pages à
  partir de 1. Les autres refus de `ops` (« no page selected »,
  « the selections leave no page to merge », « deleting every page would
  leave no page ») ne citent pas de page. Piste : que l'erreur porte
  l'indice en donnée (`NoSuchPage` le fait déjà, un doublon n'a que du
  texte) et que chaque interface écrive le message à partir de 1, avec
  l'entrée « Les raisons du noyau sont en anglais dans les bandeaux » ;
  sans toucher au noyau, la ligne de commande peut refuser le doublon
  avant l'appel, comme `session::selection` le fait pour la fusion.
- [ ] **Une page dont le rendu a échoué n'est pas redemandée** (consigné le
  3 octobre 2026, session v0.5.1-A). Quand le moteur de rendu s'arrête
  pendant le dessin d'une page, sa vignette reste vide avec, en infobulle,
  « le moteur de rendu s'est arrêté ; il sera relancé », et la vue affiche
  « Rendu impossible : … » : `ThumbnailLoader` et `PageViewer` gardent
  l'échec (`failed`) jusqu'à un autre document ou une rotation de la page.
  Le moteur est bien relancé à la demande suivante, mais rien ne redemande
  la page qui a échoué, alors qu'une seule chute peut venir d'ailleurs que
  de la page (processus tué, mémoire du système). À décider : une nouvelle
  tentative, automatique ou par un geste, pour les échecs que le côté Rust
  dit passagers, ce qui demande qu'il les distingue des refus définitifs
  (page hors de portée, page refusée après deux chutes).
- [ ] **Une page très haute cesse d'être dessinée en zoomant** (consigné le
  3 octobre 2026, session v0.5.1-A). Depuis l'ADR 0008, une image de plus
  de 16 384 pixels de haut ou de 4096 × 8192 pixels est refusée : une page
  plus de deux fois plus haute que large (ticket de caisse, infographie)
  affiche « Rendu impossible : la page 1 est trop haute pour être dessinée
  à cette largeur » passé un certain grossissement, alors que l'image
  précédente était nette. Le plafond du zoom (`zoom.ts`) ne connaît que la
  largeur maximale, 4096 pixels. Piste : qu'il tienne compte aussi de la
  hauteur de la page, avec l'entrée « Le plafond du zoom vient vite sur un
  écran dense », ou un rendu par tuiles.
- [ ] **Les erreurs de PDFium arrivent à l'utilisateur sous leur forme
  `Debug` anglaise** (consigné le 3 octobre 2026, session v0.5.1-A, testeur
  R3). Exemple : « rendu de la page 1 :
  PdfiumLibraryInternalError(Unknown) », formé par `{e:?}` dans
  `app/src/render/pdfium.rs`. Déjà le cas avant la session ; la page est
  bien citée à partir de 1. À traiter avec « Les raisons du noyau sont en
  anglais dans les bandeaux ».
- [ ] **L'infobulle de l'état du rendu reste sur l'échec du premier
  démarrage** (consigné le 3 octobre 2026, session v0.5.1-A, testeur R4 et
  relecture). Après un premier démarrage raté du moteur, `renderer_status`
  répond « disponible » avec le détail « le moteur de rendu n'a pas
  démarré : … » : la barre d'état affiche « Aperçus : PDFium » avant que
  PDFium ait été trouvé, et son infobulle garde cette raison après une
  relance réussie, l'interface ne relisant l'état qu'après une page qui
  échoue (`showRendererStatus`, `app/ui/src/main.ts`). Piste : relire
  l'état après la première page rendue, ou un libellé propre à « pas encore
  démarré ».
