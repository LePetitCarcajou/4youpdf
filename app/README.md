# app — application desktop (jalon 0.3)

Tauri 2 (Rust) + interface en TypeScript et CSS, sans framework. Première
étape : une fenêtre qui ouvre un PDF, montre ses pages en vignettes ou une
par une en grand, permet de les réordonner, de les faire pivoter et de les
supprimer, et enregistre le résultat par `fyp_core::ops`. Pas de palette, pas de conformité, pas de
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
| `src/main.rs` | commandes exposées à l'interface : ouvrir, lister, rendre une page, faire pivoter des pages, enregistrer, dialogues de fichiers |
| `src/session.rs` | le document ouvert vu par `fyp-core` : pages, réparation, chiffrement ; rotation par `ops::rotate`, qui réécrit le document gardé en mémoire ; enregistrement par `ops::extract_pages` |
| `src/render.rs` | images des pages (vignettes, vue d'une page) : thread dédié qui charge PDFium et sert les demandes une à une ; seul endroit qui connaît `pdfium-render` |
| `ui/src/main.ts` | la fenêtre : grille, glisser-déposer, sélection, menu contextuel, clavier, avis en place |
| `ui/src/history.ts` | ordre et rotation des pages, avec annuler et refaire ; une rotation est faite par le côté Rust, une à la fois |
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
- R et Maj+R, ou les boutons ↷ et ↶ de la barre d'outils, font pivoter les
  pages sélectionnées d'un quart de tour (voir « Rotation »).
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
| Faire pivoter la page à droite, à gauche | R, Maj+R | boutons ↷, ↶ |
| Retour à la grille | Échap | clic en dehors de la page, bouton `Grille` |

- Un cran de molette, ou un geste du pavé tactile avec son inertie, tourne
  une seule page.
- Les pages suivent l'ordre de la grille, modifications comprises ; la
  légende donne la page d'origine d'une page déplacée.
- De retour dans la grille, la dernière page vue a le focus ; si ce n'est
  pas la page ouverte au départ, elle devient la sélection, sinon la
  sélection reste celle d'avant.
- Annuler, Refaire, Supprimer et les boutons de rotation de la barre
  d'outils sont inactifs tant que la vue est ouverte : on réordonne et on
  supprime dans la grille. Seule la page affichée peut pivoter, par les
  boutons et les touches de la vue ; la rotation entre dans l'historique et
  s'annule dans la grille (Ctrl+Z), ou par la rotation inverse. Ouvrir…
  (Ctrl+O) et Enregistrer sous… (Ctrl+S) restent disponibles.
- La page est rendue à la largeur qu'elle occupe à l'écran, densité de
  pixels comprise, par paliers de 200 pixels (4096 au plus). La page
  affichée passe d'abord, sa vignette agrandie en attendant, puis ses deux
  voisines, pour que la navigation soit immédiate. Le moteur de rendu sert
  les demandes une par une : la vue n'en envoie qu'une à la fois et les
  vignettes attendent qu'elle soit fermée. Une page sautée en naviguant vite
  n'est pas dessinée ; agrandir la fenêtre redessine la page à la nouvelle
  taille.

### Rotation

| Action | Clavier | Souris |
|---|---|---|
| Faire pivoter à droite (sens horaire) | R | bouton ↷ |
| Faire pivoter à gauche | Maj+R | bouton ↶ |

Dans la grille, la rotation porte sur les pages sélectionnées ; dans la vue
d'une page, sur la page affichée. Elle s'applique tout de suite, sans
confirmation (ADR 0004), et entre dans l'historique comme un déplacement ou
une suppression.

- Chaque page part de sa propre rotation, héritée de l'arbre des pages ou
  non, et le résultat est ramené dans 0..360 (ISO 32000-2, 7.7.3.3) : c'est
  `ops::rotate`, appelé côté Rust. L'interface ne calcule aucun angle.
- Le côté Rust réécrit le document en mémoire (en clair s'il était chiffré)
  et le garde : les pages sont ensuite dessinées et enregistrées à partir de
  ce document. Dès sa réponse, la vignette prend la forme de la page
  tournée, puis son image arrive ; la vue d'une page redessine la page de
  même. Ce qui s'affiche est donc le document qui sera enregistré, pas une
  image tournée par l'interface.
- Annuler une rotation demande la rotation inverse. Annuler ou refaire une
  rotation garde la sélection et le focus : aucune page ne change de place.
- Les rotations passent une par une : une rotation demandée pendant qu'une
  autre est en cours attend son tour. Jusqu'à la fin, déplacer, supprimer,
  annuler et refaire sont refusés (la barre d'état le dit), Enregistrer
  sous… attend, et une page en cours de rotation est estompée si l'attente
  se voit. Une touche maintenue ne fait pivoter qu'une fois.
- Réécrire le document prend un temps proportionnel à son contenu : moins
  de 0,4 s en build debug pour les fichiers de 1 à 11 Mio essayés du corpus,
  mais 3,6 s (0,6 s en release) pour `pdfjs/bug1978317.pdf` et ses 65 000
  objets.

Pourquoi R et Maj+R : ces touches ne servent à rien d'autre, ni dans la
grille ni dans la vue, et valent dans les deux. Sans Ctrl ni Alt, elles
évitent les raccourcis de navigateur de WebView2, que wry laisse actifs
(`AreBrowserAcceleratorKeysEnabled` à sa valeur par défaut ; d'après la
documentation de WebView2, Ctrl+R et F5 rechargent la page, Ctrl+plus et
Ctrl+moins zooment, Alt+flèches navigue), ainsi que Ctrl+Alt+flèches, qui
fait pivoter l'écran avec certains pilotes graphiques. Ctrl+[ et Ctrl+], la
convention des lecteurs PDF de Chrome et d'Edge, demandent AltGr sur un
clavier AZERTY ; Ctrl+flèches reste libre pour déplacer le focus sans
changer la sélection, comme dans une liste.

## Tests

`cargo test -p fyp-app` : description des fixtures (pages, réparation,
chiffrement, mot de passe faux), ouverture ratée qui laisse le document en
cours ouvert, enregistrement d'un réordonnancement, rotation (relative à la
rotation de chaque page, héritée ou non, ramenée dans 0..360, enregistrée ;
document chiffré tourné en clair ; rotation refusée sans effet ; rotation
appliquée seulement au document qu'elle vise, jamais à un fichier ouvert
entre-temps), et, quand `app/pdfium/` est présent, rendu réel d'une page en
PNG, à la taille d'une vignette et à celle de la vue d'une page, et d'une
page tournée, dessinée couchée.

`python tools/build_ui.py` vérifie les types (`tsgo`), puis exécute les
tests de `ui/tests/` : chaque `*.test.ts` est assemblé par esbuild et lancé
par QuickJS-ng, sans DOM ni Node ; `--check` s'arrête là. Ils portent sur
la logique qui se passe du DOM : les bandeaux à l'ouverture d'un fichier
(échec, mot de passe, succès) et l'historique des pages (déplacements et
suppressions annulés sur place, rotations faites et annulées par un côté
Rust simulé, une à la fois, rotation refusée sans effet).
