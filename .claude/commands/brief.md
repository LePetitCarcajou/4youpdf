---
description: Lancer une session de travail à partir d'un brief de docs/sessions/
argument-hint: <id de session, ex. v0.5.0-C>
allowed-tools: Bash(git status:*), Bash(git log:*), Bash(git diff:*)
---

Session **$ARGUMENTS**.

État du dépôt au démarrage :
!`git status --short`
!`git log --oneline -5`

1. Lis `docs/sessions/$ARGUMENTS.md` en entier. S'il n'existe pas,
   arrête-toi et dis-le.
2. Si le dépôt n'est pas propre ci-dessus, liste les fichiers modifiés et
   demande à Martin s'il faut continuer : ils ne sont pas à toi, tu ne les
   touches pas.
3. Relis chaque fichier que le brief nomme, puis ceux dont ils dépendent
   directement pour la tâche.
4. **Phase 1** (règles de `CLAUDE.md`) : présente l'état des lieux, ta
   proposition et les questions ouvertes du brief avec ta recommandation
   pour chacune. N'écris aucun code. Termine par « En attente de ton go. »
5. Après le go de Martin (avec ses éventuelles corrections), **phase 2** :
   réalise, teste, documente, puis écris
   `docs/sessions/$ARGUMENTS-rapport.md` au format de `CLAUDE.md`.
