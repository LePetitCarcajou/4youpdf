# app — application desktop (jalon 0.3)

Tauri 2 (Rust) + interface en TypeScript et CSS, sans framework. Première
étape : une fenêtre qui ouvre un PDF, montre ses pages en vignettes, permet
de les réordonner et de les supprimer, et enregistre le résultat par
`fyp_core::ops`. Pas de palette, pas de conformité, pas de modules pour
l'instant.

## Construire et lancer

Aucun Node.js n'est requis. Deux binaires autonomes compilent l'interface
et un troisième dessine les vignettes ; trois scripts les récupèrent dans
des dossiers ignorés par Git :

```
python tools/fetch_ui_tools.py   # esbuild (MIT) et tsgo (Apache-2.0) -> app/.tools/
python tools/fetch_pdfium.py     # PDFium (BSD-3), ADR 0005            -> app/pdfium/
python tools/build_ui.py         # vérification des types + bundle      -> app/dist/
cargo run -p fyp-app
```

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
| `src/render.rs` | vignettes : thread dédié qui charge PDFium et sert les demandes une à une ; seul endroit qui connaît `pdfium-render` |
| `ui/src/main.ts` | la fenêtre : grille, glisser-déposer, sélection, menu contextuel, clavier, avis en place |
| `ui/src/history.ts` | ordre des pages avec annuler et refaire |
| `ui/src/thumbnails.ts` | chargement progressif : une page visible est demandée avant les autres |
| `ui/src/api.ts` | façade typée des commandes ; `tauri.d.ts` décrit le sous-ensemble de l'API globale de Tauri utilisé |
| `tauri.conf.json`, `capabilities/` | fenêtre unique, `withGlobalTauri`, permissions minimales |

## Comportement

- Glisser un PDF sur la fenêtre l'ouvre ; `Ouvrir…` et Ctrl+O aussi.
- Les vignettes arrivent au fil du défilement, trois à la fois, les pages
  visibles d'abord ; une page qui sort de la vue avant son tour n'est pas
  dessinée.
- Glisser une vignette (ou une sélection : Ctrl+clic, Maj+clic, Ctrl+A)
  la déplace ; le bouton `×`, la touche Suppr ou le clic droit la supprime.
  Ctrl+Z et Ctrl+Y annulent et refont. Un document garde au moins une page.
- `Enregistrer sous…` (Ctrl+S) écrit un fichier neuf, relu avant d'être
  annoncé. Le fichier d'origine n'est jamais modifié.
- Un fichier réparé (table reconstruite, `startxref` corrigé) ou chiffré
  est annoncé au-dessus de la grille, avec les mêmes mots que `fyp info` ;
  un fichier chiffré demande son mot de passe dans le même bandeau, et
  l'enregistrement est annoncé comme produisant un fichier en clair.
- Aucune fenêtre secondaire ni boîte modale (ADR 0004), hormis les
  sélecteurs de fichiers du système.

## Tests

`cargo test -p fyp-app` : description des fixtures (pages, réparation,
chiffrement, mot de passe faux), enregistrement d'un réordonnancement, et,
quand `app/pdfium/` est présent, rendu réel d'une page en PNG. Le
TypeScript est vérifié par `tsgo` à chaque `build_ui.py`.
