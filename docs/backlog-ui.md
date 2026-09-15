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

- [ ] **Ctrl+molette pour zoomer dans la vue plein cadre.** La vue ajuste
  toujours la page à la fenêtre et `viewer.ts` ignore aujourd'hui la molette
  quand Ctrl est enfoncé ; zoomer demandera de rendre la page à la taille
  agrandie (le moteur de rendu plafonne à 4096 pixels de large) et de
  pouvoir se déplacer dans la page.
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
  « Raccourcis du navigateur neutralisés ») ; le menu contextuel de WebView2
  propose encore d'imprimer l'interface (« Neutraliser le menu contextuel par
  défaut de WebView2 », plus bas).
- [ ] **Attribuer les touches neutralisées selon les conventions communes**
  (consigné le 14 septembre 2026). Quand elles recevront des commandes de
  l'application, se caler sur les conventions partagées par Acrobat, pdf.js
  et les navigateurs : Ctrl+F, puis F3 ou Ctrl+G, pour la recherche ;
  Ctrl+plus, Ctrl+moins et Ctrl+0 pour le zoom ; Ctrl+P pour l'impression.
  Pas sur les raccourcis propres à Acrobat, qui se chevauchent d'un outil à
  l'autre et dépendent d'une préférence
  ([raccourcis clavier d'Acrobat](https://helpx.adobe.com/fr/acrobat/desktop/get-started/preferences-and-settings/keyboard-shortcuts.html)).
  Rien n'est attribué aujourd'hui (`app/README.md`, « Raccourcis du
  navigateur neutralisés »).
- [ ] **Neutraliser le menu contextuel par défaut de WebView2** (consigné le
  14 septembre 2026, en neutralisant les raccourcis du navigateur). Hors des
  vignettes, où l'interface montre son propre menu, un clic droit ouvre celui
  de WebView2 : `AreDefaultContextMenusEnabled` garde sa valeur par défaut,
  que wry ne change pas et que Tauri 2.11 n'expose pas. Relevé par UI
  Automation dans un build de développement, après un clic droit envoyé comme
  message de fenêtre sur la barre d'état : « Retour », « Actualiser »
  (Ctrl+R), « Enregistrer sous », « Imprimer » (Ctrl+P), « Outils
  supplémentaires », « Inspecter ». « Actualiser » rechargerait l'interface
  et perdrait le travail non enregistré avec son historique, comme F5 le
  faisait, et « Imprimer » imprimerait l'interface. Empêcher `contextmenu` dans
  l'interface retirerait aussi « Inspecter », par lequel un build de
  développement ouvre les outils de développement (`app/README.md`,
  « Raccourcis du navigateur neutralisés »). La touche Menu et Maj+F10
  n'ont pas été essayées.
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
   
