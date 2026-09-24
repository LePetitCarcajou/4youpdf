---
description: Relire le travail d'une session, à contexte neuf, sans rien modifier
argument-hint: <id de session, ex. v0.5.0-C>
allowed-tools: Bash(git status:*), Bash(git diff:*), Bash(git log:*), Bash(cargo test:*), Bash(python tools/build_ui.py:*)
---

Tu es le relecteur de la session **$ARGUMENTS**, pas son auteur. Tu ne
modifies aucun fichier de code, de test ni de documentation : tu écris
seulement `docs/sessions/$ARGUMENTS-relecture.md`.

Fichiers touchés :
!`git status --short`
!`git diff --stat`

1. Lis `docs/sessions/$ARGUMENTS.md` (le brief) puis
   `docs/sessions/$ARGUMENTS-rapport.md` (ce que l'auteur affirme).
2. Lis le diff complet (`git diff`, plus les fichiers non suivis) et les
   fichiers voisins nécessaires pour le comprendre.
3. Lance `cargo test` sur les crates touchés et `python tools/build_ui.py`.
4. Vérifie, et pour chaque point donne fichier et ligne :
   - **Le rapport dit vrai** : chaque affirmation du rapport est confrontée
     au code. Une affirmation non vérifiable est signalée comme telle.
   - **Périmètre** : rien hors du brief, aucun changement involontaire.
   - **Invariants de `CLAUDE.md`** : permissions (les trois listes et la
     capacité), aucun remplacement silencieux de fichier, pas d'`unsafe`
     ni de réseau, ADR 0004 et 0007.
   - **Correction** : cas limites (document vide, une page, chiffré,
     réparé, sélection vide, noms en conflit, erreur d'écriture au milieu
     d'une opération), messages d'erreur, concurrence avec une opération en
     cours (rotation).
   - **Tests** : ce qui est affirmé est-il réellement testé ; le test
     échouerait-il si le code était faux.
   - **Commits proposés** : chacun compile et passe ses tests seul.
5. Écris la relecture en trois parties : **Bloquant** (à corriger avant
   commit), **À corriger** (dans cette session si c'est court), **Pour le
   backlog** (formulé comme une ligne de backlog prête à coller). Termine
   par ton verdict en une phrase.
