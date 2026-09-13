# app — application desktop

Tauri 2 (Rust) + interface en TypeScript et CSS, sans framework. Une fenêtre
qui ouvre un PDF, montre ses pages en vignettes ou une par une en grand,
permet de les réordonner, de les faire pivoter et de les supprimer, avec
annuler et refaire, et enregistre le résultat par `fyp_core::ops`. Pas de
palette, pas de conformité, pas de modules pour l'instant. Pour Windows, un
installeur et une archive portable (voir « Empaqueter pour Windows »).

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

Sous Linux et macOS, `python` s'appelle souvent `python3`.

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

## Empaqueter pour Windows

```
python tools/fetch_ui_tools.py   # une fois
python tools/fetch_pdfium.py     # une fois par version de PDFium
python tools/package_app.py      # interface, installeur, archive portable
```

`package_app.py` compile et teste l'interface (`build_ui.py`), construit au
premier passage la CLI de Tauri, épinglée dans le script, depuis crates.io
avec son `Cargo.lock` (dans `app/.tools/tauri-cli/`, quelques minutes), puis
lance `cargo tauri build` en fusionnant `tauri.bundle.json` à
`tauri.conf.json`. Il écrit dans `target/release/bundle/` et affiche
l'empreinte SHA-256 de chaque fichier :

| Fichier | Contenu |
|---|---|
| `nsis/4YouPDF_<version>_x64-setup.exe` | l'installeur NSIS (6,5 Mio en 0.3.3) |
| `portable/4YouPDF_<version>_x64_portable.zip` | l'archive portable (6,5 Mio) : un dossier `4YouPDF/` |

Les deux livrent le même exécutable, `4YouPDF.exe`, et les mêmes fichiers à
côté de lui : `pdfium.dll`, `LICENSE.txt` (AGPL-3.0) et `third-party/pdfium/`,
la notice de la compilation de PDFium et les licences de PDFium et des
bibliothèques compilées dedans (FreeType, libjpeg-turbo, OpenJPEG, ICU…).
L'exécutable n'importe que des bibliothèques de Windows : la CLI de Tauri lie
statiquement le runtime Visual C++, et `pdfium.dll` n'en demande pas
d'autre. L'archive ajoute un dossier `data/` vide (voir plus bas).

`tauri.bundle.json` n'est pas dans `tauri.conf.json` : il active
l'empaquetage et nomme des fichiers que seul `fetch_pdfium.py` fait exister,
or tauri-build refuse de compiler, même pour un `cargo build`, dès qu'une
ressource manque. Sans lui, `cargo tauri build` ne produit que l'exécutable,
jamais un installeur privé de PDFium.

`--tag vX.Y.Z` arrête le script avant de compiler si le tag ne nomme pas la
version du workspace ; `--out dossier` y copie les deux fichiers. C'est ainsi
que le workflow de release (`.github/workflows/release.yml`) est écrit pour
les construire sur un tag `v*`, puis les attacher à la Release GitHub avec
les notes produites par git-cliff, leurs empreintes (`SHA256SUMS.txt`) et une
attestation de provenance de GitHub. Sous cette forme, il n'a encore tourné
sur aucun tag : les releases publiées jusqu'à v0.3.3 n'ont aucun fichier.
Les paquets pour macOS et Linux viendront ensuite.

### Installeur et archive portable

| | Installeur | Archive portable |
|---|---|---|
| Droits d'administrateur | aucun : installation pour le compte courant | aucun |
| Emplacement | `%LOCALAPPDATA%\4YouPDF\`, modifiable pendant l'installation | le dossier où l'on extrait l'archive |
| Registre | `HKCU` seulement : l'entrée des « Applications installées » (`Software\Microsoft\Windows\CurrentVersion\Uninstall\4YouPDF`) et, pour une réinstallation, le dossier d'installation et la langue de l'installeur (`Software\4YouPDF contributors\4YouPDF`) | rien |
| Raccourcis | menu Démarrer ; bureau, case cochée par défaut à la fin de l'installation | aucun |
| Profil de WebView2 (cache du moteur d'affichage) | `%LOCALAPPDATA%\org.fouryoupdf.desktop\` | `data\`, dans le dossier |
| WebView2 absent | installé par le bootstrapper de Microsoft, avec une connexion à Internet | un message dit quoi installer, puis l'application s'arrête |
| Associations de fichiers | aucune : 4YouPDF ne se déclare pas lecteur PDF | aucune |
| Mise à jour | lancer l'installeur de la nouvelle version | remplacer le dossier |
| Retrait | « Applications installées », ou `uninstall.exe` | supprimer le dossier |

La désinstallation retire les fichiers, les raccourcis, l'entrée et la clé du
registre, et le profil de WebView2, même quand la case « Supprimer les
données de l'application » reste décochée (`windows/installer-hooks.nsh`) :
ce ne sont pas des données de l'utilisateur. La case retire en plus
`%APPDATA%\org.fouryoupdf.desktop` et le reste de
`%LOCALAPPDATA%\org.fouryoupdf.desktop`, où 4YouPDF n'écrit rien aujourd'hui.

Ces dossiers portent l'identifiant de l'application, `org.fouryoupdf.desktop`
(`identifier` de `tauri.conf.json`). Il ne doit plus changer : les données
des utilisateurs changeraient de dossier. Deux contraintes l'ont fixé avant
la première release publique. Il ne finit pas par `.app`, l'extension des
paquets d'application de macOS, ce que Tauri signale. Aucun de ses éléments
ne commence par un chiffre, d'où `fouryoupdf` : un élément de nom D-Bus ne le
peut pas, et l'identifiant d'application GLib, sous Linux, serait invalide.

L'archive fait une copie portable par son dossier `data/` : quand il existe à
côté de l'exécutable et qu'on peut y écrire, WebView2 y garde son profil au
lieu de `%LOCALAPPDATA%\org.fouryoupdf.desktop` (`portable_data_dir`, dans
`src/main.rs`), si bien que supprimer le dossier supprime tout ce que 4YouPDF
a écrit. Sur un support en lecture seule, `data/` est ignoré et le profil va
dans `%LOCALAPPDATA%`, comme pour la version installée. Il faut extraire
l'archive avant de lancer `4YouPDF.exe` : lancé depuis l'intérieur du zip,
l'exécutable part seul dans un dossier temporaire, sans `pdfium.dll` ni
`data/`.

### À vérifier sur une machine

L'installeur et l'archive ont été essayés sur Windows 11 avant la re-base
des versions et de l'identité, avec l'identifiant précédent. Ces essais ne
valent plus, et aucun n'a été refait depuis. Restent à vérifier, sur des
paquets construits avec `org.fouryoupdf.desktop` et marqués comme
téléchargés d'Internet :

- **l'installeur** : avertissement SmartScreen, puis « Exécuter quand
  même » ; installation sans droits d'administrateur dans
  `%LOCALAPPDATA%\4YouPDF\` ; raccourcis du menu Démarrer et du bureau ;
  lancement par le menu Démarrer, ouverture d'un PDF par le sélecteur de
  fichiers, pages dessinées ; profil de WebView2 dans
  `%LOCALAPPDATA%\org.fouryoupdf.desktop\EBWebView` ; désinstallation depuis
  « Applications installées », case « Supprimer les données de
  l'application » décochée, après laquelle ne restent ni le dossier
  d'installation, ni le profil, ni les clés `HKCU` du tableau ci-dessus ;
- **l'archive portable** : extraction par l'Explorateur dans un dossier au
  nom quelconque (espaces, parenthèses, accent) et avertissement SmartScreen
  au premier lancement ; `pdfium.dll` chargée depuis ce dossier ; profil de
  WebView2 dans `data\EBWebView`, rien dans `%LOCALAPPDATA%` ni
  `%APPDATA%` ; après suppression du dossier, aucun fichier ni aucune clé
  écrits par 4YouPDF ; sur un support en lecture seule, profil dans
  `%LOCALAPPDATA%` ;
- **l'absence de WebView2**, dont aucun essai n'est consigné : message et
  code de sortie 1 pour l'archive, arrêt de l'installeur privé de connexion.

Ne sont pas des restes de 4YouPDF les traces que Windows et NSIS laissent
pour tout programme : la copie du désinstalleur dans `%TEMP%`, les
documents récents et l'historique du sélecteur de fichiers (`Recent`,
`RecentDocs`, `ComDlg32`, liste « Ouvrir avec » de `.pdf`), le nom de
l'exécutable et ses lancements (`MuiCache`, `UserAssist`), les métadonnées
du menu Démarrer.

### WebView2 absent

`bundle > windows > webviewInstallMode` vaut `embedBootstrapper`. L'installeur
cherche le runtime WebView2 dans le registre ; s'il manque, il lance le
bootstrapper de Microsoft qu'il embarque (1,8 Mio, signé par Microsoft), qui
télécharge et installe le runtime « Evergreen », mis à jour ensuite par
Windows. Sans connexion, l'installation s'arrête et le dit.

Le compromis entre taille et fiabilité :

- **WebView2 manque rarement.** Il fait partie de Windows 11 ; sur Windows 10,
  Microsoft l'a largement distribué et de nombreuses applications
  l'installent. Il manque surtout sur des éditions allégées ou LTSC, ou
  quand il a été désinstallé.
- **`downloadBootstrapper`**, le défaut de Tauri, économise 1,8 Mio mais fait
  télécharger le bootstrapper par l'installeur lui-même : une connexion de
  plus, ouverte par notre installeur, et un point d'échec de plus.
- **`offlineInstaller`** fonctionne sans Internet mais embarque le runtime
  complet, plus de 100 Mio : un installeur près de vingt fois plus lourd pour
  tout le monde, au profit de ce cas rare. L'ADR 0002 a écarté Electron pour
  son navigateur embarqué ; qui en a besoin trouve l'installeur hors ligne
  chez Microsoft.
- **`fixedRuntime`** figerait une version du moteur dans l'application, sans
  les mises à jour de sécurité de Microsoft : écarté.

Avec `embedBootstrapper`, la seule connexion est celle de l'outil de
Microsoft, pour un composant du système : l'installeur de 4YouPDF n'en ouvre
aucune, ce qui s'accorde avec l'ADR 0006, qui laisse le moteur web du système
hors de son champ.

L'archive portable n'installe rien : sans WebView2, `4YouPDF.exe` affiche une
boîte de message qui dit quoi installer et où, sans ouvrir de lien, puis
s'arrête avec le code 1. C'est la seule boîte de dialogue hors des
sélecteurs de fichiers (ADR 0004), la fenêtre ne pouvant pas exister ; sans
elle, un programme sans console quitterait sans un mot. Une copie installée
dont WebView2 a été retiré depuis donne le même message ; si l'étape
WebView2 de l'installeur échoue, lui s'arrête sans rien installer.

### PDFium dans les paquets

L'application cherche `pdfium.dll`, dans l'ordre : dans le dossier désigné
par `FYP_PDFIUM_DIR` ; à côté de son exécutable, où l'installeur et
l'archive la mettent ; puis, dans un build de développement seulement
(`cargo run`, pas `cargo tauri build`), dans `app/pdfium/`. Un paquet ne
regarde jamais le dépôt dont il vient : il fonctionne sur une machine où
`fetch_pdfium.py` n'a jamais tourné. Sous Windows, pas de recherche système :
elle passe par le dossier courant et le `PATH`, où un `pdfium.dll` déposé
serait chargé à la place du nôtre. Au survol, « Aperçus : PDFium » dans la
barre d'état donne le fichier chargé ; sinon la barre dit quels chemins ont
été essayés.

### Version, signature, icônes

`tauri.conf.json` ne porte pas de version : l'application, l'installeur et
l'entrée des « Applications installées » prennent celle du workspace
(`Cargo.toml`), et `build.rs` la transmet à tauri-build pour les propriétés
de l'exécutable. Changer de version, c'est changer `Cargo.toml`.

Ni l'installeur ni l'exécutable ne sont signés : SmartScreen avertit au
lancement d'un fichier téléchargé (voir « Installer » dans le `README.md` à
la racine).

`icons/icon.svg` est la source de toutes les icônes : un « 4Y » en formes
simples, sans fonte, sur la couleur d'accent de l'interface, en attendant la
vraie identité visuelle. Pour les régénérer, depuis `app/` :
`.tools/tauri-cli/bin/cargo-tauri icon icons/icon.svg -o <dossier temporaire>`,
puis copier dans `icons/` les fichiers de `bundle > icon` et `icon.png` (la
commande produit aussi des icônes Android, iOS et Microsoft Store, inutiles
ici).

## Structure

| Fichier | Rôle |
|---|---|
| `src/main.rs` | commandes exposées à l'interface : ouvrir, fermer, état du rendu, fichier passé en ligne de commande, rendre une page, faire pivoter des pages, enregistrer, dialogues de fichiers ; ouverture de la fenêtre (profil WebView2 d'une copie portable) ; message et arrêt si WebView2 manque |
| `src/session.rs` | le document ouvert vu par `fyp-core` : pages, réparation, chiffrement ; rotation par `ops::rotate`, qui réécrit le document gardé en mémoire ; enregistrement par `ops::extract_pages` |
| `src/render.rs` | images des pages (vignettes, vue d'une page) : thread dédié qui charge PDFium et sert les demandes une à une ; où chercher la bibliothèque ; seul endroit qui connaît `pdfium-render` |
| `ui/src/main.ts` | la fenêtre : grille, glisser-déposer, sélection, menu contextuel, clavier, avis en place |
| `ui/src/history.ts` | ordre et rotation des pages, avec annuler et refaire ; une rotation est faite par le côté Rust, une à la fois |
| `ui/src/notices.ts` | bandeaux au-dessus de la grille, sans DOM : seule une ouverture réussie les remplace |
| `ui/src/thumbnails.ts` | chargement progressif : une page visible est demandée avant les autres ; en pause pendant la vue d'une page |
| `ui/src/viewer.ts` | vue d'une page par-dessus la grille : navigation, largeur de rendu adaptée à la fenêtre, page affichée puis ses voisines |
| `ui/src/api.ts` | façade typée des commandes ; `tauri.d.ts` décrit le sous-ensemble de l'API globale de Tauri utilisé |
| `ui/tests/` | tests de la logique sans DOM, exécutés par QuickJS-ng ; `check.ts` est leur harnais |
| `tauri.conf.json`, `capabilities/` | fenêtre unique, ouverte par `main.rs`, `withGlobalTauri`, permissions `core:default` et `dialog:default`, plus larges que ce que l'interface utilise (`docs/backlog-technique.md`) ; identité de l'application, icônes, réglages de l'installeur et de WebView2 |
| `tauri.bundle.json` | fusionné à `tauri.conf.json` par `tools/package_app.py` : active l'empaquetage et liste les fichiers livrés à côté de l'exécutable |
| `windows/installer-hooks.nsh` | ce que l'installeur et le désinstalleur font de plus que ceux de Tauri |
| `icons/` | `icon.svg`, source de toutes les icônes |
| `build.rs` | page d'attente quand l'interface n'est pas compilée ; version du workspace dans les propriétés de l'exécutable Windows |

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
- `Enregistrer sous…` (Ctrl+S) écrit le fichier choisi, relu avant d'être
  annoncé ; le nom proposé est celui du fichier d'origine suivi de
  `-modifié`. Le fichier d'origine n'est remplacé que si on le choisit comme
  destination.
- Un fichier réparé (table reconstruite, `startxref` corrigé) ou chiffré
  est annoncé au-dessus de la grille, avec la cause et la description du
  chiffrement que donne aussi `fyp info` ; un fichier protégé par un mot de
  passe le demande dans le même bandeau, et l'enregistrement d'un fichier
  chiffré est annoncé comme produisant un fichier en clair.
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
entre-temps), emplacements de PDFium (un paquet ne cherche jamais dans le
dépôt dont il vient), dossier `data` qui rend une copie portable,
configuration (fenêtre ouverte par l'application, pas de version propre),
et, quand `app/pdfium/` est présent, rendu réel d'une page en
PNG, à la taille d'une vignette et à celle de la vue d'une page, et d'une
page tournée, dessinée couchée.

`python tools/build_ui.py` vérifie les types (`tsgo`), puis exécute les
tests de `ui/tests/` : chaque `*.test.ts` est assemblé par esbuild et lancé
par QuickJS-ng, sans DOM ni Node ; `--check` s'arrête là. Ils portent sur
la logique qui se passe du DOM : les bandeaux à l'ouverture d'un fichier
(échec, mot de passe, succès) et l'historique des pages (déplacements et
suppressions annulés sur place, rotations faites et annulées par un côté
Rust simulé, une à la fois, rotation refusée sans effet).
