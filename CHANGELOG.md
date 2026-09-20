# Changelog

Toutes les modifications notables de 4YouPDF, une section par version à
partir de la 0.4.0, d'après les Conventional Commits du dépôt depuis le tag
précédent. Les versions antérieures, jusqu'à v0.3.4, n'ont que les notes de
leur Release GitHub, que `release.yml` produit par git-cliff (`cliff.toml`)
à partir des sujets des commits ; les groupes ci-dessous sont les siens.

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
