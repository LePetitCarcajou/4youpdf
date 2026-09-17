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

Le job `version-check` de `release.yml` lance
`python tools/check_version.py --tag <tag>` avant toute compilation. Il lit
le **patch du tag** pour savoir laquelle des deux règles s'applique :

| Tag | Ce qu'il est | Ce qui est exigé du workspace |
|---|---|---|
| patch nul, `v0.4.0` | une rampe | `[workspace.package] version` vaut exactement `0.4.0` |
| patch non nul, `v0.3.4` | un palier | même majeure et même mineure, et un patch qui ne redescend pas sous le sien |

Une rampe ajoute des fonctionnalités : la version du workspace avance avec
elle, et son tag la nomme. Un palier n'ajoute rien : la version du workspace
peut rester celle que la rampe a laissée, et le tag prend le patch suivant
sans qu'elle bouge. Sur un workspace à 0.3.3, `v0.3.4` passe donc ; `v0.3.1`
est refusé, un tag ne redescendant pas, et `v0.5.1` aussi, sa mineure
n'étant pas celle du workspace. Tout tag qui n'est pas
`vMAJEUR.MINEUR.PATCH` en trois nombres simples est refusé plutôt que
deviné : `v0.3`, `v0.3.4-rc1`, `v0.3.04`.

**Ce qu'un palier laisse alors incohérent.** Les fichiers d'une release
portent la version du workspace, la seule inscrite dans le build :
`tools/package_app.py` les nomme avec elle, et tauri-build l'écrit dans les
propriétés de l'exécutable. Tant que la version du workspace reste sous le
patch du tag, une Release v0.3.4 attache donc `4YouPDF_0.3.3_x64-setup.exe`.
Faire avancer `[workspace.package] version` au patch du palier lève l'écart
et passe la même vérification, le patch du tag valant alors celui du
workspace. Décision à prendre avant la première release publique
(`backlog-technique.md`).

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
- [ ] Versions cohérentes : le tag passe
      `python tools/check_version.py --tag vX.Y.Z` (voir « Nommage ») et les
      propriétés de `4YouPDF.exe` portent la version du workspace. Si
      celle-ci est restée sous le patch du tag, les fichiers de la release
      ne portent pas son numéro : le dire dans la PR.
- [ ] Corpus, fixtures et fuzz passés (`cargo test --workspace` ;
      `cargo test -p fyp-core --test corpus --release` après
      `tools/fetch_corpus.py` ; fuzz de la CI).
- [ ] Backlog à jour.
