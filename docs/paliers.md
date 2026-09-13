# Rampes et paliers

La méthode de développement de 4YouPDF : deux sortes de versions en
alternance, et une règle de périmètre pour chaque session de travail.

## Rampe : on ajoute

Une rampe ajoute des fonctionnalités. La dette qu'elle laisse derrière elle
est inscrite, pas dissimulée : au backlog (`backlog-technique.md`,
`backlog-ui.md`) ou dans les « Limites connues » d'un ADR, au moment où on
la laisse, avec ce qui la montre.

## Palier : on solde

Un palier n'ajoute rien : il solde la dette des rampes. Il se termine par un
tag, et par un dépôt où chaque phrase de la documentation est vraie.

## Nommage

Une rampe prend la version mineure suivante, `v0.X.0` ; le palier qui la
suit prend le patch suivant. Les versions jusqu'à v0.3.3 ont précédé cette
méthode et ne suivent pas ce nommage ; le premier palier est v0.3.4.

## Règle de périmètre

Chaque session a un objectif écrit. Une trouvaille faite pendant la session
va au backlog, sauf si elle empêche d'atteindre cet objectif : alors
seulement elle devient une tâche de la session, même si la correction tient
en deux lignes, même si elle est évidente. La règle vaut pour les rampes
comme pour les paliers.

Exemple réel, le 13 septembre 2026 : une session de re-base des versions a
produit deux jobs CI (`msrv-product`, `msrv-plugin-api`), un épinglage
d'action par SHA et une analyse du contrat des modules. Tous utiles, aucun
demandé : selon la règle, chacun serait allé au backlog.

## Grille de sortie de palier

À recopier et cocher dans la PR qui clôt le palier. Une case ne se coche que
sur une vérification faite pendant le palier.

- [ ] Documentation vraie : aucune affirmation fausse ou périmée, aucune
      vérification ancienne présentée comme actuelle.
- [ ] Aucun code mort.
- [ ] Dette de sécurité traitée, ou reportée avec sa condition de
      déclenchement écrite.
- [ ] `cargo deny check` vert, et chaque dépendance nouvelle justifiée.
- [ ] Versions cohérentes : tag, workspace et exécutable
      (`python tools/check_version.py --tag vX.Y.Z`, propriétés de
      `4YouPDF.exe`).
- [ ] Corpus, fixtures et fuzz passés (`cargo test --workspace` ;
      `cargo test -p fyp-core --test corpus --release` après
      `tools/fetch_corpus.py` ; fuzz de la CI).
- [ ] Backlog à jour.
