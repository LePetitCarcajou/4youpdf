# Changelog

Toutes les modifications notables de 4YouPDF, une section par version à
partir de la 0.4.0, d'après les Conventional Commits du dépôt depuis le tag
précédent. Les versions antérieures, jusqu'à v0.3.4, n'ont que les notes de
leur Release GitHub, que `release.yml` produit par git-cliff (`cliff.toml`)
à partir des sujets des commits ; les groupes ci-dessous sont les siens.

## [0.5.0] — 3 octobre 2026

Une rampe : les outils de pages complets depuis la fenêtre, à savoir choisir
les pages de chaque fichier fusionné, extraire la sélection et découper le
document (`docs/paliers.md`).

### Ajouts

- **core :** `ops::merge_selected` fusionne une sélection de pages de chaque
  document : toutes, ou une liste d'indices dans l'ordre voulu, où une page
  peut revenir plusieurs fois. Une page laissée de côté est traitée comme
  une page supprimée : un signet qui la visait garde son titre et perd sa
  destination, un lien qui n'a plus de cible disparaît. `ops::merge` garde
  sa signature et confie son travail à `ops::merge_selected`.
- **cli :** `fyp merge` accepte un `--pages` par fichier, dans le même
  ordre, avec la syntaxe de plages des autres commandes (`1,3,5-8`, `8-5`
  à l'envers) et le mot `all` pour un fichier entier. Sans `--pages`, chaque
  fichier donne toutes ses pages, comme avant.
- **app :** `Fusionner…` (Ctrl+M) et `Fusionner ici…` ouvrent un bandeau
  qui demande quelles pages prendre de chaque fichier choisi, toutes si
  l'on ne tape rien, avec la syntaxe de la ligne de commande. Le bandeau
  montre ce que la fusion ajouterait (« 7 pages ajoutées à la fin : 3 de
  « B.pdf », 4 de « C.pdf » ; le document en aura 10. ») et refuse en place,
  sans rien fusionner, un numéro qui n'en est pas un, une page que le
  fichier n'a pas ou une page tapée deux fois. Les pages choisies s'ajoutent
  à la grille, comme avant, et Ctrl+Z les retire.
- **app :** `Extraire la sélection…`, dans le menu contextuel d'une vignette
  ou par Ctrl+E, écrit les pages sélectionnées dans un nouveau fichier, dans
  l'ordre de la grille et avec leurs rotations, sans toucher au document
  affiché ni à son historique. L'extraction refuse d'écrire sur le fichier
  du document ouvert, reconnu même sous d'autres majuscules ou par un détour
  de chemin, et le dit dans un bandeau.
- **app :** `Découper…` (Ctrl+D) partage le document en plusieurs fichiers,
  dans un dossier choisi, toutes les N pages ou avant chaque page
  sélectionnée, d'après l'ordre de la grille. Un bandeau montre ce qui
  serait écrit (« 3 fichiers : 5 + 5 + 2 pages. ») ; aucun fichier existant
  n'est remplacé, et si un seul nom est pris, rien n'est écrit.

### Sécurité

- **deps :** Wasmtime passe de 48.0.2 à 48.0.5, qui corrige cinq avis
  (RUSTSEC-2026-0315, -0316, -0325, -0326 et -0327) ; aucun n'atteignait le
  bac à sable des modules, qui borne leur temps par epoch et non par fuel,
  et compile Wasmtime sans ramasse-miettes, sans exceptions ni modèle de
  composants.

### Documentation

- `docs/architecture.md` : ce qu'une sélection de fusion prend et laisse
  (« Sélection à la fusion »), et l'application telle qu'elle est à 0.5.0.
- `app/README.md` : la fusion par pages, l'extraction et le découpage.
- `docs/feuille-de-route.md` : la rampe v0.5.0 faite, le palier v0.5.1
  détaillé en trois sessions, et un palier pour la lecture par blocs et le
  budget mémoire.

### Tests

- `tests/fixtures/page-ranges.tsv` : 43 listes de pages telles qu'on les
  tape, avec ce que leur lecture doit donner, refus compris mot pour mot.
  La ligne de commande et la fenêtre parcourent toutes deux la table, et la
  lisent de la même façon.
- Chaque opération de la rampe a un test qui écrit son résultat puis le
  relit sans réparation, ni table reconstruite ni `startxref` à
  rechercher : la sélection à la fusion du noyau, `fyp merge --pages`,
  l'extraction, le découpage et la fusion par pages de la fenêtre.

## [0.4.1] — 20 septembre 2026

Un palier : rien de nouveau pour l'utilisateur, la dette soldée
(`docs/paliers.md`).

### Corrections

- **app :** la Content-Security-Policy de la fenêtre part de
  `default-src 'none'`, chaque directive ne nommant que ce dont l'interface
  se sert ; `base-uri` et `form-action`, qui ne retombent pas sur
  `default-src`, valent `'none'`, et `freezePrototype` est activé.
  L'ancienne politique admettait `'unsafe-inline'` pour les styles sans en
  avoir besoin et n'avait pas de `connect-src` : l'IPC de Tauri était refusé
  à chaque lancement et retombait silencieusement sur `postMessage`.
- **app :** la fenêtre ne reçoit que les commandes et les événements que
  l'interface appelle, chacun avec sa raison dans
  `app/permissions/commands.toml`. `core:default` et `dialog:default`
  accordaient des ensembles entiers de permissions dont elle ne se sert pas.
- **app :** en release, le port de débogage que
  `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` ouvrait est fermé, et le menu
  contextuel natif de WebView2 — qui proposait « Actualiser », lequel
  perdait le travail non enregistré, et « Imprimer », qui imprimait
  l'interface — est bloqué par un script d'initialisation. Un build de
  développement garde les deux, pour inspecter et pour piloter les essais.

### Documentation

- `docs/feuille-de-route.md` : la route jusqu'à la 1.0 en petits jalons,
  les critères de sortie, les dettes connues et les décisions ouvertes.
  `docs/architecture.md` et `CLAUDE.md` y renvoient désormais.
- ADR 0007 : le durcissement de la fenêtre WebView, ses décisions, les
  solutions écartées et les limites connues.
- L'état du projet est vrai partout : la version courante, les releases qui
  attachent des fichiers depuis v0.3.4, et les instructions de build — la
  CLI de Tauri n'est requise que pour empaqueter, `python tools/build_ui.py`
  puis `cargo run -p fyp-app` suffisent.
- Les numéros de jalons cités par les ADR 0004 et 0006 sont datés et
  renvoient aux blocs de la feuille de route.

### Tests

- Une fixture de douze pages, `tests/fixtures/mixed12.pdf`, et son
  générateur : A4 portrait et paysage, `/Rotate 90`, Letter et une page de
  300 × 800 points, de quoi voir si les vignettes restent alignées quelle
  que soit la forme de la page. Un test du noyau tient son nombre de pages,
  ses dimensions et ses rotations.

### Build

- `.gitattributes` garde tout fichier de texte en LF, dans le dépôt comme
  dans la copie de travail : Git n'avertit plus « LF will be replaced by
  CRLF » sur les fichiers de `app/`.

## [0.4.0] — 18 septembre 2026

### Ajouts

- **app :** fusion d'autres PDF depuis la fenêtre. `Fusionner…` (Ctrl+M)
  ajoute toutes les pages des fichiers choisis à la suite du document, et
  `Fusionner ici…`, dans le menu contextuel d'une vignette, les insère
  devant la page sélectionnée. Une seule étape d'historique : Ctrl+Z les
  retire, Ctrl+Y les rend. Un fichier qui ne s'ouvre pas, protégé par un
  mot de passe ou illisible, est nommé dans un bandeau et laissé de côté ;
  les autres sont fusionnés sans lui.
- **app :** un build de développement porte « — DEV » à la fin du titre de
  sa fenêtre ; un build empaqueté s'appelle « 4YouPDF », comme avant.

### Corrections

- **ui :** dans la grille des vignettes, le numéro d'une page en paysage,
  tournée ou non, se retrouvait plus haut que ceux de ses voisines. Chaque
  vignette réserve désormais la hauteur d'une page en portrait, la page
  centrée et son numéro en bas, dans la grille comme dans le panneau (F4)
  de la vue d'une page.

### Documentation

- `README.md` et `CONTRIBUTING.md` sont en anglais ; le reste de la
  documentation reste en français.
