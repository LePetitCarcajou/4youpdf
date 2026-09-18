# Changelog

Toutes les modifications notables de 4YouPDF, une section par version à
partir de la 0.4.0, d'après les Conventional Commits du dépôt depuis le tag
précédent. Les versions antérieures, jusqu'à v0.3.4, n'ont que les notes de
leur Release GitHub, que `release.yml` produit par git-cliff (`cliff.toml`)
à partir des sujets des commits ; les groupes ci-dessous sont les siens.

## [0.4.0] — en préparation

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
