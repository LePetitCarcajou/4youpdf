---
description: Corriger ce que la relecture et les tests manuels ont trouvé
argument-hint: <id de session, ex. v0.5.0-C>
allowed-tools: Bash(git status:*), Bash(git diff:*)
---

Correctifs de la session **$ARGUMENTS**.

1. Lis `docs/sessions/$ARGUMENTS.md`, puis
   `docs/sessions/$ARGUMENTS-relecture.md` s'il existe, puis les lignes
   « Résultat : » remplies par Martin dans la checklist de
   `docs/sessions/$ARGUMENTS-rapport.md`.
2. Liste ce que tu vas corriger : tout le « Bloquant », le « À corriger »
   que Martin n'a pas barré, et chaque case de la checklist en échec.
   Le « Pour le backlog » va dans les backlogs, pas dans le code.
   Attends le go de Martin.
3. Corrige, avec un test qui aurait échoué avant quand c'est possible.
4. Mets à jour le rapport : ajoute une section « Correctifs » (problème,
   cause, correction, test), ajuste le découpage en commits et la
   checklist (seulement les cases à refaire).
