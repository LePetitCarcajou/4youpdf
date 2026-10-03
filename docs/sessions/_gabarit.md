# Session <id> — <titre en une ligne>

**Type** : <rampe ou palier vX.Y.Z, session N sur M ; ou clôture de rampe,
de palier> · **Lancement** : `/session <id>` (ou `/brief <id>`) ·
**Arrêt en phase 1** : <non, ou oui et pourquoi> · **Crates autorisés** :
<lesquels ; ou **Code de production** : aucun changement>. <Dépendances
nouvelles admises, ou aucune.>

## À lire d'abord

- `docs/feuille-de-route.md`, § <n>
- <fichiers de code et documents concernés, avec ce qu'il faut y regarder>
- <entrées de `docs/backlog-ui.md` et `docs/backlog-technique.md` visées,
  citées par leur titre>

## Objectif

Ce que l'utilisateur pourra faire à la fin, ou ce que le dépôt sera, en
deux ou trois phrases.

## Décisions déjà prises (Martin, <date>)

1. **<Décision>.** <Le choix d'architecture tranché, et pourquoi, assez
   précis pour qu'aucune question ne reste.>

## Questions laissées à la session

- <Ce que la session tranche elle-même et justifie dans son rapport.>

## Hors périmètre

- <Ce qui est explicitement exclu, et la session ou le palier qui s'en
  chargera ; toute trouvaille hors sujet va au backlog.>

## Critère de fin

- <Comportement vérifiable, et le test qui le prouve.>
- `cargo test`, `cargo clippy` sur les crates touchés et
  `python tools/build_ui.py` au vert ; <scripts de `tools/ui_smoke/`>.
- `app/README.md`, `docs/architecture.md` et backlogs à jour : <entrées
  fermées ou réduites>.

## Pour le testeur

- <Ce qu'il doit essayer de mettre en défaut : cas limites, fichiers
  hostiles, pannes, et ce qu'il doit montrer sur une copie sous
  `target/agents/<id>/`.>

## Pour le relecteur

- <Ce qu'il doit confronter au code, au brief et à `git log`.>

## Après la session (Martin)

1. Checklist manuelle du rapport, puis `commits.ps1`, puis `git push`.
2. <Tag, ou session suivante : `/clear` puis `/session <id suivant>`.>
