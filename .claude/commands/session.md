---
description: Mener une session entière à partir de son brief (état des lieux, code, testeur, relecteur, corrections, rapport, script de commits)
argument-hint: <id de session, ex. v0.5.1-A>
allowed-tools: Bash(git status:*), Bash(git log:*), Bash(git diff:*)
---

Session **$ARGUMENTS**, en mode autonome (`CLAUDE.md`, « Rôles et déroulé
d'une session »). Tu ne t'arrêtes que sur l'un des arrêts que liste
`CLAUDE.md`. À chaque arrêt : ce que tu as constaté, les options, ta
recommandation, puis « En attente de Martin. » et rien d'autre.

État du dépôt au démarrage :
!`git status --short`
!`git log --oneline -5`

## 0. Préalables

- Lis `docs/sessions/$ARGUMENTS.md` en entier. S'il n'existe pas,
  arrête-toi et dis-le.
- Si le dépôt n'est pas propre ci-dessus, arrête-toi : liste les fichiers,
  ils ne sont pas à toi.

## 1. État des lieux

Relis chaque fichier que le brief nomme, puis ceux dont ils dépendent
directement. Écris dans la conversation, en vingt lignes au plus : ce qui
existe, ce que tu vas faire, et ta réponse à chaque question que le brief
te laisse. Si le brief dit « Arrêt en phase 1 : oui », ou si l'un des
arrêts de `CLAUDE.md` s'applique, arrête-toi ici. Sinon, continue sans
attendre.

## 2. Réalisation

Code, tests, documentation, backlogs, comme le brief les demande. Puis, sur
chaque crate touché : `cargo fmt -p <crate>`, `cargo clippy -p <crate>
--all-targets -- -D warnings`, `cargo test -p <crate>` ; `python
tools/build_ui.py` si `app/ui/` a changé ; `cargo deny check` si
`Cargo.lock` a changé. Tout est vert avant l'étape 3.

## 3. Testeur

Lance le sous-agent `testeur` avec ce message : « Session $ARGUMENTS.
Brief : docs/sessions/$ARGUMENTS.md. Compte rendu attendu dans
docs/sessions/$ARGUMENTS-tests.md. » Attends sa fin. Vérifie ensuite avec
`git status --short` qu'il n'a rien modifié hors de
`docs/sessions/$ARGUMENTS-tests.md` (`target/` est ignoré par Git) : sinon,
arrête-toi.

## 4. Relecteur

Note la sortie de `git status --porcelain -uall` et l'empreinte de
`git diff | git hash-object --stdin`. Lance le sous-agent `relecteur` avec
ce message : « Session $ARGUMENTS. Brief : docs/sessions/$ARGUMENTS.md.
Compte rendu du testeur : docs/sessions/$ARGUMENTS-tests.md. » À son
retour, refais les deux relevés : s'ils diffèrent, arrête-toi. Sinon,
enregistre sa réponse telle quelle dans
`docs/sessions/$ARGUMENTS-relecture.md`.

## 5. Corrections, deux tours au plus

Corrige tout le « Bloquant » et tout le « À corriger » de la relecture, et
chaque défaut « bloquant » ou « à corriger » du testeur, chacun avec un test
qui aurait échoué avant quand c'est possible. Le « Pour le backlog » va dans
les backlogs, pas dans le code. Si un point te paraît faux, ne le corrige
pas : dis pourquoi dans le rapport.

Puis relance les vérifications de l'étape 2, et :
- le testeur, seulement sur les défauts qu'il avait trouvés (cite-les-lui) ;
  il complète son compte rendu d'une section « Tour N » ;
- le relecteur, seulement sur ce que le tour a changé ; ajoute sa réponse à
  la relecture sous « Tour N », avec les mêmes relevés avant et après.

Après deux tours, ce qui reste ouvert et qui est bloquant est l'arrêt 6 ;
le reste est cité dans le rapport, § 6.

## 6. Rapport

Écris `docs/sessions/$ARGUMENTS-rapport.md` au format de `CLAUDE.md`.

## 7. Script de commits

Dans `../4YouPDF-patches/$ARGUMENTS/`, hors du dépôt :
- un patch par commit du § 4 du rapport (`1.patch`, `2.patch`…), qui
  ensemble couvrent exactement l'arbre de travail, fichiers non suivis
  compris ; un fichier partagé entre deux commits est découpé par blocs ;
- un fichier de message par commit (`1.msg`…), en anglais, UTF-8 sans BOM :
  sujet Conventional Commits, ligne vide, corps qui explique le pourquoi ;
- `commits.ps1`, pour Windows PowerShell 5.1 : `$ErrorActionPreference =
  'Stop'` ; vérifie d'abord que `git status --porcelain` liste exactement
  les fichiers attendus ; puis pour chaque commit `git apply --cached
  <patch>` et `git commit -s -F <msg>`, en testant `$LASTEXITCODE` après
  chaque commande git et en s'arrêtant au premier échec ; à la fin, vérifie
  que `git status --porcelain` est vide et affiche `git log --oneline` des
  commits créés. Jamais de `git push` ni de `git tag`.

Puis prouve-le, hors du dépôt :
- **chaque commit seul** : dans une copie tirée par `git archive HEAD`, applique
  les patches un à un ; à chaque état, `cargo fmt --check`, `cargo clippy`
  et `cargo test` sur les crates touchés, et `python tools/build_ui.py` si
  `app/ui/` est touché ; après le dernier, la copie est égale à l'arbre de
  travail ;
- **le script** : dans un clone `git clone --shared --no-checkout` suivi de
  `git read-tree HEAD`, les `git apply --cached` du script donnent, pour
  chaque fichier, le même blob que `git hash-object --path` sur l'arbre de
  travail.

Écris le résultat de ces deux preuves dans
`../4YouPDF-patches/$ARGUMENTS/preuves.md`, pas dans le rapport : il est
déjà dans un patch.

## 8. Fin

Dis à Martin, en dix lignes au plus : le verdict du relecteur, ce qui
reste ouvert, le nombre de cases de la checklist manuelle, et les
commandes à lancer :

```
powershell -ExecutionPolicy Bypass -File ..\4YouPDF-patches\$ARGUMENTS\commits.ps1
git push
```
