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

- [ ] **Basculer l'affichage du panneau de vignettes.** La grille de
  vignettes occupe aujourd'hui toute la zone de travail et la vue plein
  cadre la recouvre entièrement ; il s'agit de pouvoir afficher ou masquer
  les vignettes, par exemple en panneau à côté de la page affichée, la forme
  restant à choisir.
- [ ] **Aller à une page en tapant son numéro.** On ne se déplace
  aujourd'hui que de proche en proche (flèches, Pg préc./suiv., Début/Fin,
  molette) ou en faisant défiler la grille ; il faudra préciser si le numéro
  tapé désigne la position dans l'ordre courant ou la page du fichier
  d'origine, que la légende de la vue distingue déjà.
- [ ] **Ctrl+molette pour zoomer dans la vue plein cadre.** La vue ajuste
  toujours la page à la fenêtre et `viewer.ts` ignore aujourd'hui la molette
  quand Ctrl est enfoncé ; zoomer demandera de rendre la page à la taille
  agrandie (le moteur de rendu plafonne à 4096 pixels de large) et de
  pouvoir se déplacer dans la page.
- [ ] **Recherche de texte dans le document.** Le noyau ne lit pas encore le
  contenu des pages : extraire le texte suppose d'interpréter les flux de
  contenu et de retrouver l'Unicode des glyphes par les polices et
  `/ToUnicode` (ISO 32000-2, 9.10), un chantier du noyau à cadrer et à
  tester comme un morceau à part, avant l'interface de recherche, qui devra
  aussi reprendre Ctrl+F à WebView2.
- [ ] **Bug : Ctrl+P imprime une capture de l'interface au lieu du PDF.**
  wry laisse actifs les raccourcis de navigateur de WebView2 et `main.ts` ne
  traite pas Ctrl+P, si bien que WebView2 imprime la page HTML de
  l'interface ; la touche est à intercepter côté interface, puisque couper
  `AreBrowserAcceleratorKeysEnabled` depuis Rust passerait par un appel COM
  `unsafe`, et les autres raccourcis de navigateur restés actifs (voir
  « Rotation » dans `app/README.md`) relèvent du même correctif.
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
  n'ajuste aujourd'hui que la page entière à la fenêtre et sa molette tourne
  les pages ; une page ajustée à la largeur peut dépasser de la fenêtre, et
  la molette devra alors d'abord la faire défiler, à concevoir avec le point
  « Ctrl+molette », dont celui-ci partage le rendu agrandi.
- [ ] **Mode plein écran pour la lecture.** La vue d'une page garde
  volontairement visibles la barre d'outils, les bandeaux (fichier réparé ou
  chiffré) et la barre d'état, et Échap y ramène à la grille : le plein
  écran devra dire où passent ces avertissements et comment Échap se partage
  entre sortir du plein écran et quitter la vue.
- [ ] **OCR : reconnaissance du texte des pages scannées.** Priorité
  confirmée par plusieurs comparatifs d'éditeurs PDF ; la feuille de route
  le prévoit, sans version attribuée, comme module natif Tesseract côté hôte
  (`docs/architecture.md`), et non dans `fyp-core`, qui n'embarque ni code
  natif ni `unsafe` et compile pour wasm32-wasip1 ; rien n'est commencé, ni
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
  `capabilities/default.json` n'accorde que `core:default`, dont
  `core:window:default` permet de lire le titre sans le changer (il y
  faudrait `core:window:allow-set-title`) : la demande est refusée, et
  `main.ts` ignore le refus. Seule la barre d'outils nomme le document.
  Constaté par script : lancée avec `minimal.pdf` en argument, la fenêtre
  s'appelle encore « 4YouPDF » quinze secondes plus tard.
