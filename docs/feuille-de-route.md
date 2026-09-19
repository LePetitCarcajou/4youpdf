# 4YouPDF — Feuille de route vers la 1.0

> **À lire en premier** par toute nouvelle discussion avec une IA et par toute
> session Claude Code. Ce document dit où en est le projet, ce qu'on vise, et
> quel est le prochain petit pas. Il est mis à jour à la fin de chaque jalon.
>
> Dernière mise à jour : 17 septembre 2026, à la clôture de la rampe v0.4.0.

---

## 1. Le projet en dix lignes

4YouPDF, le « VLC du PDF » : un logiciel pour lire et modifier des PDF,
gratuit, libre (AGPL-3.0-or-later, contributions sous DCO, pas de CLA),
100 % local (le cœur n'accède jamais au réseau : ni télémétrie, ni mise à
jour automatique), hors du contrôle de tout organisme lucratif, et
extensible par des modules tiers sandboxés (WebAssembly, permissions
explicites).

- Dépôt : `github.com/LePetitCarcajou/4youpdf`, local `E:\CODE\4YouPDF`.
- Crates : `fyp-core` (noyau, zéro réseau, `forbid(unsafe)`), `fyp-crypto`,
  `fyp-conformance`, `fyp-plugin-api` (contrat des modules, **versionné à
  part**, 0.2.0), `fyp-host` (sandbox Wasmtime), `fyp-cli` (`fyp`), `app/`
  (Tauri 2, interface TypeScript sans framework).
- Rendu : **PDFium** en production. Décision mesurée (`docs/mesure-hayro.md`),
  à ne rouvrir qu'avec une nouvelle mesure du banc `tools/render_bench/`.

## 2. Méthode de travail (ne pas déroger)

**Rampes et paliers.** Une rampe ajoute des fonctionnalités et se tague en
version mineure (`v0.N.0`). Un palier n'ajoute rien : il solde la dette, la
sécurité et la documentation fausse, et se tague en correctif (`v0.N.1`).
Détail dans `docs/paliers.md`.

**Taille d'un jalon.** Un jalon = **une seule capacité** visible ou un seul
chantier de dette, **1 à 3 sessions** Claude Code, un **critère de fin
vérifiable**. Un jalon estimé à plus de 3 sessions est découpé avant de
commencer.

**Une session Claude Code = un objectif.** Chaque brief :
1. fait relire les fichiers pertinents en tête (Martin fait des `/clear`) ;
2. borne strictement l'objectif ;
3. dit explicitement : « toute trouvaille hors sujet va au backlog
   (`docs/backlog-ui.md` ou `docs/backlog-technique.md`), sans la corriger » ;
4. exige `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo test --workspace`, `cargo deny check` au vert ;
5. demande un rapport de fin : fichiers modifiés, découpage en commits
   proposé avec le « pourquoi », lignes ajoutées au backlog, **checklist de
   vérification manuelle** pour ce qu'aucun script ne prouve ;
6. interdit à Claude Code de committer ou de taguer.

**Commits.** Faits à la main par Martin. Conventional Commits, signés
(`git commit -s`), en anglais (règle de `CLAUDE.md`), message qui explique
le pourquoi. Code en anglais ; documentation et interface en français
(README et CONTRIBUTING aussi en anglais).

**Clôture d'un jalon.** Checklist manuelle faite → commits → CI verte sur
`main` → tag → `release.yml` publie (binaires nommés à la bonne version,
`SHA256SUMS.txt`, attestation de provenance) → mise à jour de ce document.

## 3. Où on en est

| Version | Type | Contenu |
|---|---|---|
| v0.1.0 | rampe | Noyau : xref classique et flux, `/Prev`, filtres, object streams, reconstruction par scan, écriture, chiffrement RC4/AES en lecture, corpus public |
| v0.2.0 | rampe | Opérations de pages dans le noyau et la CLI (fusion, extraction, découpage, rotation, suppression) |
| v0.3.0 – v0.3.3 | rampe | Chargeur de modules WASM audité, première fenêtre Tauri : vignettes PDFium, réordonner, supprimer, vue d'une page, rotation avec annuler/refaire |
| v0.3.4 | palier | Release publiant de vrais binaires, CI épinglée, actionlint, fuzz étendu (0 crash / ~1,3 M exécutions), signalement privé de vulnérabilités — plus palette ambre, icône du carcajou, panneau F4, zoom, raccourcis navigateur neutralisés, avertissement avant perte |
| **v0.4.0** | rampe | Fusion multi-documents depuis l'interface, vignettes alignées quelle que soit l'orientation, marqueur « — DEV » dans le titre, README/CONTRIBUTING en anglais — **commits faits, tag à poser** |

Note : les binaires de la Release v0.3.4 portent le nom 0.3.3 (version du
workspace non montée avant le tag). `release.yml` appelle désormais
`tools/check_version.py --tag`, ce qui empêche la récidive.

### Dettes connues, par ordre de gravité

1. **WebView non durcie** : pas de CSP, capabilities Tauri larges, DevTools
   accessibles en release, menu contextuel natif. → palier v0.4.1.
2. **PDFium dans le processus principal** : du C++ qui lit des fichiers
   hostiles ; un crash tue l'application. → palier v0.5.1.
3. **Documentation fausse** : des fichiers nomment encore 0.3.3, disent
   qu'aucune release n'a de fichier, documentent `cargo tauri dev` (CLI
   Tauri non requis) ; `README.md` et `CLAUDE.md` peuvent encore parler du
   jalon 0.1 ; les numéros de jalons cités dans les ADR (PAdES « jalon 0.5 »)
   ne correspondent plus à ce document. → palier v0.4.1.
4. **Dette WASM** : compilation de module non bornée, modules non signés.
   Acceptable tant qu'aucun module tiers n'est installable depuis un
   fichier. → palier obligatoire avant le bloc C.

## 4. Ce que promet la 1.0 (critères de sortie)

La 1.0 est une promesse de stabilité. Chaque critère doit être vérifiable.

**Aux utilisateurs — « vous pouvez désinstaller votre lecteur actuel »**
- [ ] Le banc de fidélité ne signale aucune perte de contenu silencieuse
      sur le corpus de référence.
- [ ] Le noyau réécrit 100 % du corpus ouvrable sans perte (jalon
      « round-trip », `FYP_CORPUS_STRICT=1` bloquant en CI).
- [ ] Rechercher, sélectionner et copier du texte.
- [ ] Imprimer.
- [ ] Naviguer : signets, liens internes, ajustement à la largeur, plein écran.
- [ ] Remplir un formulaire et l'enregistrer.
- [ ] Opérations de pages complètes depuis l'interface : fusion par plages,
      extraction, découpage, rotation, réordonnancement, suppression.
- [ ] Surligner, souligner, barrer, ajouter une note.
- [ ] Protéger un document par mot de passe (chiffrement à l'écriture).
- [ ] Un enregistrement ne peut jamais laisser un fichier à moitié écrit.

**Sécurité — aucune dette connue à la sortie**
- [ ] WebView durcie (CSP, capabilities minimales, DevTools absents en release).
- [ ] Rendu isolé dans un processus séparé.
- [ ] Compilation des modules bornée, modules signés et vérifiés hors ligne.
- [ ] Fuzzing continu en CI ; politique de sécurité publiée (versions suivies).

**Aux auteurs de modules**
- [ ] `fyp-plugin-api` en 1.0 : contrat figé, changement cassant = 2.0.
- [ ] Installation d'un module depuis un fichier, vérification de signature
      sans réseau, écran de consentement aux permissions (ADR 0003, 0006).
- [ ] Catalogue signé consultable hors de l'application (ADR 0003, point 5).
- [ ] Documentation pour auteurs, gabarit de module, un exemple complet.

**Normes**
- [ ] Rapport de conformité PDF/A et PDF/UA (validation, sans correction
      assistée pour la 1.0).
- [ ] Validation des signatures PAdES présentes dans un document.

**Distribution — ressembler à VLC**
- [ ] Installeur Windows signé (piste : SignPath Foundation, gratuit pour le
      libre) ; paquet Linux (Flatpak ou AppImage).
- [ ] Distribution par un gestionnaire de paquets (winget, Flathub) qui
      porte les mises à jour, puisque le cœur n'a pas de réseau.
- [ ] Interface en français et en anglais ; entièrement utilisable au
      clavier ; testée avec un lecteur d'écran.
- [ ] Peut devenir le lecteur PDF par défaut ; écran « À propos » avec
      version et commit.
- [ ] Documentation utilisateur.

**Explicitement repoussé en 1.x** : conformité PDF/X, PDF/E, PDF/VT et
correction assistée, signature PAdES (signer, pas seulement valider), OCR,
rédaction irréversible, filigrane et numérotation des pages, macOS (sauf
décision contraire, voir § 7), bascule éventuelle vers hayro.

## 5. Les prochains jalons (horizon détaillé)

Seuls les jalons proches sont numérotés et détaillés. Les autres (§ 6) sont
dans un ordre indicatif, revu à chaque palier.

### v0.4.0 — Rampe « fusion » : clôture ⏳
- Reste : poser le tag, vérifier la Release, installer l'installeur publié
  (titre « 4YouPDF », version 0.4.0), annoter la Release v0.3.4 (« les
  fichiers portent 0.3.3 par erreur »).

### v0.4.1 — Palier « WebView et documentation vraie » (2 sessions)
**Session A — durcissement de la WebView**
- CSP stricte dans la configuration Tauri ; capabilities Tauri 2 réduites
  aux seules commandes utilisées ; DevTools et port de débogage absents en
  release ; menu contextuel natif neutralisé en release.
- Fin : tests verts ; checklist manuelle sur le build **release** : F12,
  Ctrl+Maj+I, clic droit n'ouvrent rien ; toutes les fonctions de l'interface
  marchent encore (ouvrir, fusionner, tourner, zoomer, enregistrer).

**Session B — documentation vraie**
- Corriger les documents qui nomment 0.3.3, disent qu'aucune release n'a de
  fichier, documentent `cargo tauri dev`, ou annoncent le jalon 0.1.
- Faire pointer la section « Feuille de route » de `docs/architecture.md`
  et `CLAUDE.md` vers ce document ; aligner les numéros de jalons cités dans
  les ADR (ou les remplacer par des noms de blocs).
- Intégrer `mixed12.pdf` (et son script générateur, sauvegardés dans
  `E:\CODE\4YouPDF-fixtures`) aux fixtures ou au corpus, avec un test.
- Combler la lacune `.gitattributes` pour `app/ui` (avertissements CRLF).
- Fin : une recherche de « 0.3.3 » ne trouve plus rien hors du CHANGELOG et
  de l'historique ; CI verte.

### v0.5.0 — Rampe « outils de pages complets » (2 sessions)
- Session A (noyau) : `ops::merge` accepte une plage de pages par fichier ;
  la CLI l'expose ; tests et fixtures.
- Session B (interface) : choisir les pages de chaque fichier lors d'une
  fusion ; « Extraire la sélection » vers un nouveau fichier ; « Découper »
  (toutes les N pages ou par plages) depuis l'interface.
- Fin : tests de round-trip sur les nouvelles opérations ; checklist manuelle
  des trois gestes.

### v0.5.1 — Palier « rendu isolé » (2 à 3 sessions)
- PDFium tourne dans un processus enfant ; un plantage du rendu laisse
  l'application vivante (vignettes vides + bandeau) et le processus est
  relancé.
- Fin : tuer le processus de rendu depuis le Gestionnaire des tâches ne
  ferme pas la fenêtre et le rendu reprend ; le banc de rendu ne montre pas
  de ralentissement notable ; ADR écrit pour le protocole entre processus.

### v0.6.0 — Rampe « navigation » (2 sessions)
- Noyau : lire les signets (`/Outlines`) et les annotations de lien
  (`/Link` : destination interne, action `/GoTo`, `/URI`).
- Interface : panneau des signets ; liens internes cliquables dans la vue ;
  lien web : l'adresse est montrée et ouverte par le navigateur du système
  sur clic explicite seulement (ADR 0006).
- Fin : tests noyau sur des fixtures à signets et liens ; checklist manuelle.

### v0.7.0 — Rampe « confort de lecture » (1 à 2 sessions)
- Ajustement à la largeur et à la hauteur ; mode plein écran (où vont les
  bandeaux, comment Échap se partage) ; écran « À propos » (version, commit,
  licence).
- Fin : checklist manuelle ; rien de modifié dans `render.rs` hors besoin
  démontré.

## 6. Au-delà : les blocs jusqu'à la 1.0 (ordre indicatif)

Chaque ligne deviendra un jalon numéroté quand elle entrera dans l'horizon
détaillé. Un palier s'intercale au moins après chaque bloc.

**Bloc A — Le texte** *(clé de voûte : recherche, sélection, annotations,
PDF/UA et, plus tard, OCR et rédaction en dépendent)*
1. Extraction du texte par le noyau, polices simples et `/ToUnicode` ;
   commande `fyp text` ; comparaison avec `pdftotext` sur un sous-ensemble
   du corpus ; nouvelle cible de fuzz.
2. Palier : robustesse et performance de l'interpréteur de contenu.
3. Polices composites (Type0/CID) et position de chaque caractère.
4. Recherche dans l'interface (Ctrl+F rendu à l'application).
5. Sélection et copie du texte.

**Bloc B — Interagir avec le document**
1. Impression (approche à décider par un ADR).
2. Annotations de balisage : surligner, souligner, barrer, note
   (ISO 32000-2, 12.5.6).
3. Remplir un formulaire AcroForm et l'enregistrer.
4. Protéger par mot de passe (chiffrement AES-256 à l'écriture).
5. Enregistrement atomique garanti (si pas déjà acquis).

**Bloc C — Ouvrir aux modules tiers**
1. Palier obligatoire : borne de compilation et signature des modules.
2. Installer un module depuis un fichier, écran des permissions.
3. `fyp-plugin-api` 1.0 candidat, documentation pour auteurs, gabarit,
   exemple complet.
4. Catalogue signé hors application.

**Bloc D — Normes**
1. Rapport de conformité PDF/A.
2. Rapport de conformité PDF/UA.
3. Validation des signatures PAdES (hors ligne, et dit comme tel).

**Bloc E — Distribution**
1. Installeur Windows signé.
2. Paquet Linux.
3. winget / Flathub.
4. Traduction anglaise de l'interface ; accessibilité clavier et lecteur
   d'écran ; association comme lecteur par défaut.
5. Documentation utilisateur.

**1.0.0-rc — Palier de sortie**
- Gel des fonctionnalités ; tous les critères du § 4 cochés ; revue de
  sécurité (externe si possible) ; round-trip du corpus bloquant ; banc de
  fidélité propre ; puis 1.0.0.

## 7. Décisions ouvertes

| Question | Options | Quand trancher |
|---|---|---|
| macOS pour la 1.0 ? | Oui (signature Apple payante, 99 $/an) ou 1.x | Avant le bloc E |
| OCR en 1.0 ou 1.x ? | Proposé : 1.x (dépend du bloc A et des modules natifs) | Après le bloc A |
| PAdES : signer en 1.0 ? | Proposé : valider seulement en 1.0, signer en 1.x | Avant le bloc D |
| Impression | Rendu PDFium vers l'impression du système, ou autre | ADR au début du bloc B |
| Numérotation des jalons dans les ADR | Aligner sur ce document, ou citer des noms de blocs | Palier v0.4.1 |

## 8. Reprendre dans une nouvelle discussion

Donner à l'IA, dans cet ordre :
1. **ce document** ;
2. la dernière **note de reprise** (état du jour, ce qui reste ouvert) ;
3. le dernier **rapport de fin de session** Claude Code, s'il y en a un.

Puis demander : « rédige le brief Claude Code de la prochaine session du
jalon en cours, au format du § 2 ».

À la fin de chaque jalon, mettre à jour : le tableau du § 3, les dettes, les
cases du § 4, et faire glisser l'horizon du § 5 (le prochain bloc du § 6
devient des jalons numérotés).
