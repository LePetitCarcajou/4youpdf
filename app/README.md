# app — application desktop (jalon 0.3)

Tauri 2 (Rust) + interface en TypeScript et CSS, sans framework. Première
étape : une fenêtre qui ouvre un PDF, montre ses pages en vignettes ou une
par une en grand, permet de les réordonner et de les supprimer, et enregistre
le résultat par `fyp_core::ops`. Pas de palette, pas de conformité, pas de
modules pour l'instant.

## Construire et lancer

Aucun Node.js n'est requis. Trois binaires autonomes compilent et testent
l'interface et un quatrième dessine les pages ; trois scripts les
récupèrent dans des dossiers ignorés par Git et construisent l'interface :

```
python tools/fetch_ui_tools.py   # esbuild (MIT), tsgo (Apache-2.0), QuickJS-ng (MIT) -> app/.tools/
python tools/fetch_pdfium.py     # PDFium (BSD-3), ADR 0005                            -> app/pdfium/
python tools/build_ui.py         # vérification des types, tests, bundle              -> app/dist/
cargo run -p fyp-app
```

Un `app/.tools/` récupéré avant l'arrivée de QuickJS-ng ne suffit plus :
relancer `fetch_ui_tools.py`, sans quoi `build_ui.py` s'arrête faute de
`qjs`.

Tauri embarque `app/dist/` dans le binaire à la compilation : après une
modification de l'interface, relancer `build_ui.py` puis `cargo run`.
Sans `build_ui.py`, la crate compile quand même (le script de build écrit
une page d'attente dans `dist/`), donc `cargo test --workspace` marche sur
un dépôt fraîchement cloné, pourvu que les prérequis système ci-dessous
soient installés. Sans PDFium, l'application fonctionne avec des vignettes
vides et dit pourquoi dans sa barre d'état.

### Prérequis système

- **Windows** : WebView2, présent sur Windows 11.
- **Linux** : WebKitGTK 4.1 et les bibliothèques de développement qui
  l'accompagnent. Sous Debian ou Ubuntu (la CI tourne sur Ubuntu 24.04) :

  ```
  sudo apt-get update
  sudo apt-get install libwebkit2gtk-4.1-dev libgtk-3-dev \
    libayatana-appindicator3-dev librsvg2-dev libsoup-3.0-dev pkg-config
  ```

  Sans eux, la compilation de la crate s'arrête sur une erreur de
  `pkg-config` (`glib-2.0`, `gobject-2.0`, `webkit2gtk-4.1`…). Pour les
  autres distributions, voir les
  [prérequis de Tauri 2](https://v2.tauri.app/start/prerequisites/).

## Structure

| Fichier | Rôle |
|---|---|
| `src/main.rs` | commandes exposées à l'interface : ouvrir, lister, rendre une page, enregistrer, dialogues de fichiers |
| `src/session.rs` | le document ouvert vu par `fyp-core` : pages, réparation, chiffrement ; enregistrement par `ops::extract_pages` |
| `src/render.rs` | images des pages (vignettes, vue d'une page) : thread dédié qui charge PDFium et sert les demandes une à une ; seul endroit qui connaît `pdfium-render` |
| `ui/src/main.ts` | la fenêtre : grille, glisser-déposer, sélection, menu contextuel, clavier, avis en place |
| `ui/src/history.ts` | ordre des pages avec annuler et refaire |
| `ui/src/notices.ts` | bandeaux au-dessus de la grille, sans DOM : seule une ouverture réussie les remplace |
| `ui/src/thumbnails.ts` | chargement progressif : une page visible est demandée avant les autres ; en pause pendant la vue d'une page |
| `ui/src/viewer.ts` | vue d'une page par-dessus la grille : navigation, largeur de rendu adaptée à la fenêtre, page affichée puis ses voisines |
| `ui/src/api.ts` | façade typée des commandes ; `tauri.d.ts` décrit le sous-ensemble de l'API globale de Tauri utilisé |
| `ui/tests/` | tests de la logique sans DOM, exécutés par QuickJS-ng ; `check.ts` est leur harnais |
| `tauri.conf.json`, `capabilities/` | fenêtre unique, `withGlobalTauri`, permissions minimales |

## Comportement

- Glisser un PDF sur la fenêtre l'ouvre ; `Ouvrir…` et Ctrl+O aussi.
- Les vignettes arrivent au fil du défilement, trois à la fois, les pages
  visibles d'abord ; une page qui sort de la vue avant son tour n'est pas
  dessinée.
- Glisser une vignette (ou une sélection : Ctrl+clic, Maj+clic, Ctrl+A)
  la déplace ; le bouton `×`, la touche Suppr ou le clic droit la supprime.
  Ctrl+Z et Ctrl+Y annulent et refont. Un document garde au moins une page.
- Double-cliquer sur une vignette, ou appuyer sur Entrée, montre la page en
  grand (voir « Vue d'une page »).
- `Enregistrer sous…` (Ctrl+S) écrit un fichier neuf, relu avant d'être
  annoncé. Le fichier d'origine n'est jamais modifié.
- Un fichier réparé (table reconstruite, `startxref` corrigé) ou chiffré
  est annoncé au-dessus de la grille, avec les mêmes mots que `fyp info` ;
  un fichier chiffré demande son mot de passe dans le même bandeau, et
  l'enregistrement est annoncé comme produisant un fichier en clair.
- Les bandeaux ne sont remplacés qu'une fois un autre fichier ouvert. Une
  ouverture qui échoue laisse le document affiché, ses bandeaux et la barre
  d'état tels quels, et ajoute un seul message d'erreur (celui d'un échec
  précédent est remplacé). Un fichier qui demande un mot de passe laisse de
  même le document affiché jusqu'à ce que le bon mot de passe l'ouvre ; la
  demande nomme le fichier.
- Aucune fenêtre secondaire ni boîte modale (ADR 0004), hormis les
  sélecteurs de fichiers du système.

### Vue d'une page

Double-clic sur une vignette, ou Entrée sur la vignette qui a le focus (à
défaut, sur la première page sélectionnée) : la page occupe toute la zone de
la grille. Ce n'est ni une fenêtre ni une boîte modale (ADR 0004) mais un
état de la fenêtre : la grille reste dessous, à sa place, et la barre
d'outils, les bandeaux et la barre d'état restent visibles, si bien qu'un
fichier réparé ou chiffré reste annoncé de la même façon.

| Action | Clavier | Souris |
|---|---|---|
| Page suivante | → ou Pg suiv. | molette vers le bas ou vers la droite, bouton `›` |
| Page précédente | ← ou Pg préc. | molette vers le haut ou vers la gauche, bouton `‹` |
| Première, dernière page | Début, Fin | |
| Retour à la grille | Échap | clic en dehors de la page, bouton `Grille` |

- Un cran de molette, ou un geste du pavé tactile avec son inertie, tourne
  une seule page.
- Les pages suivent l'ordre de la grille, modifications comprises ; la
  légende donne la page d'origine d'une page déplacée.
- De retour dans la grille, la dernière page vue a le focus ; si ce n'est
  pas la page ouverte au départ, elle devient la sélection, sinon la
  sélection reste celle d'avant.
- Annuler, Refaire et Supprimer sont inactifs tant que la vue est ouverte :
  on modifie les pages dans la grille. Ouvrir… (Ctrl+O) et Enregistrer sous…
  (Ctrl+S) restent disponibles.
- La page est rendue à la largeur qu'elle occupe à l'écran, densité de
  pixels comprise, par paliers de 200 pixels (4096 au plus). La page
  affichée passe d'abord, sa vignette agrandie en attendant, puis ses deux
  voisines, pour que la navigation soit immédiate. Le moteur de rendu sert
  les demandes une par une : la vue n'en envoie qu'une à la fois et les
  vignettes attendent qu'elle soit fermée. Une page sautée en naviguant vite
  n'est pas dessinée ; agrandir la fenêtre redessine la page à la nouvelle
  taille.

## Tests

`cargo test -p fyp-app` : description des fixtures (pages, réparation,
chiffrement, mot de passe faux), ouverture ratée qui laisse le document en
cours ouvert, enregistrement d'un réordonnancement, et, quand
`app/pdfium/` est présent, rendu réel d'une page en PNG, à la taille d'une
vignette et à celle de la vue d'une page.

`python tools/build_ui.py` vérifie les types (`tsgo`), puis exécute les
tests de `ui/tests/` : chaque `*.test.ts` est assemblé par esbuild et lancé
par QuickJS-ng, sans DOM ni Node ; `--check` s'arrête là. Ils portent sur
la logique qui se passe du DOM, aujourd'hui les bandeaux à l'ouverture d'un
fichier (échec, mot de passe, succès).
