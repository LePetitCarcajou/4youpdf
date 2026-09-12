# ADR 0005 — PDFium comme moteur de rendu temporaire

**Statut** : accepté — 2026-09

## Contexte
L'interface (jalon 0.3) montre les pages en vignettes : il faut dessiner
une page, ce que le noyau ne sait pas faire. Un moteur de rendu maison
(interprète de contenu, fontes, images, ombrages) est un chantier de
plusieurs mois, hors du chemin critique de la première fenêtre. Or l'ADR
0004 fait du rendu un prérequis : pas d'aperçu sans page affichée.

Les bibliothèques disponibles depuis Rust :

- **PDFium** (Google, Chromium), C++ sous BSD-3-Clause, exposé par la crate
  `pdfium-render` (MIT ou Apache-2.0) qui charge la bibliothèque partagée à
  l'exécution ; binaires prêts à l'emploi publiés par
  `bblanchon/pdfium-binaries` (scripts Apache-2.0). Rendu fidèle, rapide,
  éprouvé sur des milliards de documents.
- **MuPDF** (Artifex), AGPL-3.0 ou licence commerciale : compatible avec
  notre licence, mais son bindings Rust est peu maintenu et la
  bibliothèque doit être compilée avec l'application.
- **Poppler**, GPL-2.0 : incompatible avec la distribution sous AGPL-3.0
  sans exception, et pas de bindings Rust sérieux.
- **pdf-render** et autres crates Rust pures : trop incomplètes (pas de
  fontes CFF/Type 1 correctes, pas d'ombrages).

## Décision
1. **PDFium via `pdfium-render`, chargé dynamiquement**, pour dessiner les
   vignettes et, plus tard, la page affichée. Licences BSD-3 (PDFium), MIT
   ou Apache-2.0 (`pdfium-render`, ses dépendances) : compatibles avec
   l'AGPL-3.0, vérifiées par `cargo deny`. Le binaire n'est pas dans le
   dépôt : `tools/fetch_pdfium.py` récupère une version épinglée
   (`chromium/8044`) dans `app/pdfium/`, ignoré par Git, avec sa licence.
2. **Dépendance temporaire, confinée.** Seul le module `app/src/render.rs`
   connaît `pdfium-render`. Le reste de l'application, et l'interface,
   ne voient qu'un service : « page N du document ouvert, W pixels de
   large → PNG », et un état « aperçus disponibles ou non, et pourquoi ».
   Remplacer PDFium par notre moteur reviendra à réécrire ce module.
3. **L'application fonctionne sans PDFium.** Bibliothèque absente ou
   inchargeable : les vignettes montrent une page vide au bon format, la
   barre d'état dit pourquoi, et ouvrir, réorganiser, supprimer, enregistrer
   marchent. Le noyau (`fyp-core`) reste la seule source de vérité sur la
   structure du document ; PDFium ne fait que dessiner.
4. **Un seul thread pour PDFium.** La bibliothèque n'est pas réentrante :
   un thread dédié la charge et sert les demandes une par une, à travers un
   canal. Les commandes de l'interface attendent hors du thread de
   l'interface.
5. **Pas dans le noyau, pas dans les modules.** `fyp-core` ne dépend pas de
   PDFium ; aucun module ne l'appelle. Le rendu est un service de
   l'application, réservé à l'affichage.

## Conséquences
- **Deux parseurs regardent chaque fichier.** Ce que PDFium affiche peut
  différer de ce que le noyau lit : un fichier réparé par le scan du noyau
  peut être rendu autrement par PDFium, ou pas du tout. L'interface montre
  alors une vignette vide en signalant l'échec, sans rien conclure sur le
  document. À terme, notre moteur lira le modèle objet du noyau, et cette
  divergence disparaîtra.
- **Le document est chargé deux fois en mémoire** : une fois par le noyau,
  une fois par PDFium (copie des octets). Acceptable pour cette étape ; à
  reprendre avec la lecture par blocs prévue par l'ADR 0004.
- **Mot de passe transmis à PDFium.** Un fichier chiffré est déchiffré par
  le noyau pour la structure et par PDFium pour l'image : le mot de passe
  saisi dans l'interface est donné aux deux. Il reste en mémoire le temps
  de la session, jamais sur disque.
- **Installation en deux temps** tant que le rendu est externe : le binaire
  PDFium doit accompagner l'exécutable (à côté de lui, ou dans le dossier
  `FYP_PDFIUM_DIR`). Le futur paquet d'installation l'embarquera avec sa
  licence ; en développement, `tools/fetch_pdfium.py` suffit.
- **Une version épinglée.** `pdfium-render` cible une version précise de
  l'API PDFium (`pdfium_latest` de la crate) ; la version du binaire est
  fixée dans le script et changée délibérément, avec un passage sur les
  fixtures et le corpus.
- **Critère de sortie.** PDFium est retiré quand notre moteur rend les
  fixtures et le corpus public avec une fidélité comparable, mesurée par
  comparaison d'images sur un jeu de pages de référence. D'ici là, cet ADR
  est la seule raison pour laquelle du code non Rust s'exécute dans
  l'application.
