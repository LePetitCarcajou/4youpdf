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
simples, sans fonte, sur `#1f4e79`, le bleu qui était l'accent de l'interface
avant sa palette ambre ([docs/couleurs.md](../docs/couleurs.md)), en
attendant une icône à la nouvelle identité. Pour les régénérer, depuis
`app/` :
`.tools/tauri-cli/bin/cargo-tauri icon icons/icon.svg -o <dossier temporaire>`,
puis copier dans `icons/` les fichiers de `bundle > icon` et `icon.png` (la
commande produit aussi des icônes Android, iOS et Microsoft Store, inutiles
ici).

## Structure

| Fichier | Rôle |
|---|---|
| `src/main.rs` | commandes exposées à l'interface : ouvrir, fermer, état du rendu, fichier passé en ligne de commande, rendre une page, faire pivoter des pages, enregistrer, dialogues de fichiers ; ouverture de la fenêtre (profil WebView2 d'une copie portable) ; message et arrêt si WebView2 manque |
| `src/session.rs` | le document ouvert vu par `fyp-core` : pages, réparation, chiffrement ; rotation par `ops::rotate`, qui réécrit le document gardé en mémoire ; enregistrement par `ops::extract_pages` |
| `src/render.rs` | images des pages (vignettes, vue d'une page) : thread dédié qui charge PDFium et sert les demandes une à une ; où chercher la bibliothèque ; seul endroit qui connaît `pdfium-render`. Le banc de fidélité du rendu (`tools/render_bench`) compile ce fichier tel quel et appelle ses trois étapes une à une : ouvrir, dessiner, encoder |
| `ui/src/main.ts` | la fenêtre : grille, glisser-déposer, sélection, menu contextuel, panneau de vignettes à côté de la vue d'une page, clavier, raccourcis du navigateur neutralisés, avis en place |
| `ui/src/history.ts` | ordre et rotation des pages, avec annuler et refaire ; une rotation est faite par le côté Rust, une à la fois |
| `ui/src/notices.ts` | bandeaux au-dessus de la grille, sans DOM : seule une ouverture réussie les remplace |
| `ui/src/pagenumber.ts` | numéros de la vue d'une page, sans DOM : légende, lecture du numéro tapé pour aller à une page (une position dans l'ordre actuel), aide et refus |
| `ui/src/shortcuts.ts` | raccourcis du navigateur neutralisés, sans DOM : la liste, chacun reconnu par son code de touche virtuelle et ses modificateurs exacts |
| `ui/src/thumbnails.ts` | chargement progressif : une page visible est demandée avant les autres ; pendant la vue d'une page, aucune demande tant qu'elle dessine, puis une à la fois pour le panneau |
| `ui/src/viewer.ts` | vue d'une page à côté du panneau de vignettes ou par-dessus la grille : navigation, numéro de page à taper, largeur de rendu adaptée à la fenêtre, page affichée puis ses voisines |
| `ui/src/api.ts` | façade typée des commandes ; `tauri.d.ts` décrit le sous-ensemble de l'API globale de Tauri utilisé |
| `ui/styles.css` | la feuille de style ; en tête, les couleurs en deux niveaux, couleurs brutes puis rôles, décrites par [docs/couleurs.md](../docs/couleurs.md) |
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
- Les raccourcis du navigateur qui rechargeraient la page de l'interface,
  l'imprimeraient ou y chercheraient (F5, Ctrl+P, Ctrl+F…) sont neutralisés,
  et leurs touches restent libres pour l'application (voir « Raccourcis du
  navigateur neutralisés »).

### Vue d'une page

Double-clic sur une vignette, ou Entrée sur la vignette qui a le focus (à
défaut, sur la première page sélectionnée) : la page s'affiche à côté du
panneau de vignettes (voir « Panneau de vignettes ») ou, panneau masqué,
dans toute la zone de la grille. Ce n'est ni une fenêtre ni une boîte
modale (ADR 0004) mais un état de la fenêtre : la grille reste là, réduite
au panneau ou dessous, et la barre d'outils, les bandeaux et la barre d'état
restent visibles, si bien qu'un fichier réparé ou chiffré reste annoncé de la
même façon.

| Action | Clavier | Souris |
|---|---|---|
| Page suivante | → ou Pg suiv. | molette vers le bas ou vers la droite, bouton `›` |
| Page précédente | ← ou Pg préc. | molette vers le haut ou vers la gauche, bouton `‹` |
| Première, dernière page | Début, Fin | |
| Aller à une page | son numéro, puis Entrée (voir « Aller à une page ») | clic sur le numéro de la légende |
| Aller à la page d'une vignette du panneau | Entrée sur la vignette | clic sur la vignette |
| Afficher, masquer les vignettes | F4 | bouton `Vignettes` |
| Faire pivoter la page à droite, à gauche | R, Maj+R | boutons ↷, ↶ |
| Retour à la grille | Échap | bouton `Grille` |

- Un cran de molette, ou un geste du pavé tactile avec son inertie, tourne
  une seule page.
- Un clic à côté de la page, sur le fond sombre de la vue, ne fait rien,
  panneau affiché ou non, pas plus qu'un double-clic sur la page : seuls
  Échap et le bouton `Grille` ramènent à la grille, et ouvrir un autre
  document ferme aussi la vue. Ce clic la fermait quand la vue recouvrait
  toujours la grille ; à côté du panneau, le fond fait partie de la vue, et
  un clic égaré y faisait perdre sa place. Le double-clic sur la page est
  gardé pour sélectionner un mot, quand la page aura son texte (« Barre
  d'annotation », `docs/backlog-ui.md`).
- Les pages suivent l'ordre de la grille, modifications comprises ; la
  légende donne la page d'origine d'une page déplacée.
- De retour dans la grille, la dernière page vue a le focus ; si ce n'est
  pas la page ouverte au départ, elle devient la sélection, sinon la
  sélection reste celle d'avant.
- Annuler, Refaire, Supprimer et les boutons de rotation de la barre
  d'outils sont inactifs tant que la vue est ouverte : on réordonne et on
  supprime dans la grille, pas dans le panneau. Seule la page affichée peut
  pivoter, par les boutons et les touches de la vue ; la rotation entre dans
  l'historique et s'annule dans la grille (Ctrl+Z), ou par la rotation
  inverse. Ouvrir… (Ctrl+O) et Enregistrer sous… (Ctrl+S) restent
  disponibles.
- La page est rendue à la largeur qu'elle occupe à l'écran, densité de
  pixels comprise, par paliers de 200 pixels (4096 au plus). La page
  affichée passe d'abord, sa vignette agrandie en attendant, puis ses deux
  voisines, pour que la navigation soit immédiate. Le moteur de rendu sert
  les demandes une par une : la vue n'en envoie qu'une à la fois, et les
  vignettes attendent qu'elle n'ait plus rien à dessiner (voir « Panneau de
  vignettes »). Une page sautée en naviguant vite n'est pas dessinée ;
  agrandir la fenêtre redessine la page à la nouvelle taille.

### Panneau de vignettes

Le panneau est la grille elle-même, réduite à une colonne à gauche de la
page affichée : les mêmes vignettes, avec les mêmes numéros (« 3 (était
5) »). F4, ou le bouton `Vignettes` de la barre de la vue, l'affiche ou le
masque. Le bouton est à droite de la barre, à côté de `Grille`, parce que la
gauche de la barre se décale avec la vue quand le panneau s'affiche ou se
masque : un second clic au même endroit tomberait à côté du bouton.

- Il est affiché à la première ouverture de la vue. Son état est un état de
  la fenêtre (ADR 0004) : il reste le même d'une page à l'autre, d'une
  ouverture de la vue à la suivante et pour un autre document, jusqu'à la
  fermeture de l'application, qui ne l'enregistre pas.
- La vignette de la page affichée y est encadrée et gardée visible : le
  panneau défile jusqu'à elle à chaque changement de page, et quand il
  s'affiche.
- Un clic sur une vignette, ou Entrée sur celle qui a le focus, affiche sa
  page. Rien ne s'y modifie : ni sélection, ni glisser-déposer, ni bouton
  `×`, ni menu du clic droit. La sélection de la grille est gardée, mais le
  panneau ne la montre pas.
- Ses vignettes passent après la page affichée : aucune n'est demandée tant
  que la vue dessine, puis une seule à la fois. Passées celles que la grille
  avait déjà demandées à l'ouverture de la vue (trois au plus), une page
  demandée en naviguant n'attend donc au plus qu'une demande, une page
  voisine en cours ou une vignette. Panneau masqué, aucune n'est demandée
  avant le retour à la grille.

Pourquoi F4 : c'est la touche du panneau latéral de pdf.js, le lecteur PDF de
Firefox, qui l'a reprise d'Adobe Reader
([mozilla/pdf.js#10358](https://github.com/mozilla/pdf.js/pull/10358)). Elle
ne sert à rien d'autre dans l'application et n'est pas un raccourci de
WebView2 : elle ne figure ni dans la liste, non exhaustive, que donne
`AreBrowserAcceleratorKeysEnabled`, ni dans le tableau des raccourcis de sa
documentation (voir « Raccourcis du navigateur neutralisés »).

### Aller à une page

La légende de la vue commence par le numéro de la page affichée, dans un
champ : « Page [3] sur 12 ». Taper un chiffre n'importe où dans la vue, ou
cliquer sur ce numéro, permet d'en taper un autre ; Entrée y va, Échap
annule.

Le numéro tapé est la **position de la page dans l'ordre actuel**, pas son
numéro dans le fichier d'origine. C'est la numérotation du fichier que
produira `Enregistrer sous…`, celle que la légende et chaque vignette donnent
en premier et que borne le « sur 12 » ; le numéro d'origine, donné en second
(« (page 5 du fichier) », « (était 5) »), peut ne plus désigner aucune page
après une suppression. Pendant la saisie, la barre de la vue le dit à la
place de ses raccourcis : « Numéro dans l'ordre actuel (1 à 12) ».

- Un numéro hors de portée (0, ou plus que le nombre de pages) et ce qui
  n'est pas un numéro (lettres, signe, virgule, champ vide) ne changent
  rien : la page reste affichée, le champ est encadré de rouge, son contenu
  sélectionné, et la barre dit pourquoi (« Aucune page ne porte ce numéro
  dans l'ordre actuel (1 à 12) »). Seuls comptent les chiffres de 0 à 9 ;
  les espaces autour et les zéros en tête sont ignorés.
- Le champ reprend le numéro de la page affichée dès qu'il perd le focus :
  après Entrée, après Échap, ou sur un clic ailleurs.
- Sur un clavier AZERTY, les chiffres de la rangée du haut demandent Maj ou
  le verrouillage des majuscules ; ceux du pavé numérique, non.

Pourquoi les chiffres plutôt que Ctrl+G : aucun chiffre n'est un raccourci
de l'application, alors que Ctrl+G est « rechercher le suivant » dans les
navigateurs et dans pdf.js, un sens que lui garde la recherche de texte
prévue au backlog (voir « Raccourcis du navigateur neutralisés »).

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
évitent les raccourcis de WebView2, comme Ctrl+R pour recharger la page ou
Alt+flèches pour naviguer dans l'historique, que l'interface neutralise
(voir « Raccourcis du navigateur neutralisés »), ainsi que Ctrl+Alt+flèches,
qui fait pivoter l'écran avec certains pilotes graphiques. Ctrl+[ et Ctrl+],
la convention des lecteurs PDF de Chrome et d'Edge, demandent AltGr sur un
clavier AZERTY ; Ctrl+flèches reste libre pour déplacer le focus sans
changer la sélection, comme dans une liste.

### Raccourcis du navigateur neutralisés

wry laisse actifs les raccourcis de navigateur de WebView2 : F5 rechargeait
la page de l'interface, et les modifications non enregistrées disparaissaient
sans un mot avec l'historique d'annulation (un fichier passé en argument
était rouvert tel qu'il est sur le disque) ; Ctrl+P imprimait l'interface.
wry sait les couper tous (`with_browser_accelerator_keys`), mais Tauri 2.11
ne transmet pas ce réglage, et atteindre `AreBrowserAcceleratorKeysEnabled`
autrement passerait par un appel COM `unsafe`, interdit dans le dépôt.
L'interface les neutralise donc (`ui/src/shortcuts.ts`) : un écouteur de
`keydown`, posé sur la fenêtre en phase de capture, empêche l'action par
défaut de chaque raccourci de la liste avant tout autre écouteur.

| Ce que ferait WebView2 | Touches neutralisées |
|---|---|
| Recharger la page, avec ou sans le cache | F5, Maj+F5, Ctrl+F5, Ctrl+R, Ctrl+Maj+R ; touche Actualiser du clavier, seule, avec Maj ou avec Ctrl |
| Imprimer | Ctrl+P ; Ctrl+Maj+P, l'impression par la boîte de dialogue du système dans Chromium |
| Chercher dans la page | Ctrl+F, Ctrl+G, Ctrl+Maj+G, F3, Maj+F3 |
| Zoomer, déjà coupé ici par la configuration (plus bas) | Ctrl+plus, Ctrl+Maj+plus, Ctrl+moins, Ctrl+Maj+moins, Ctrl+0 ; Ctrl avec plus, moins ou 0 du pavé numérique |
| Revenir en arrière, avancer | Alt+←, Alt+→ ; touches Précédent et Suivant du clavier |
| Navigation au curseur | F7 |
| Téléchargements | Ctrl+J |
| Outils de développement | F12, Ctrl+Maj+I, Ctrl+Maj+J, Ctrl+Maj+C |

La liste reprend le tableau des raccourcis que coupe
`AreBrowserAcceleratorKeysEnabled` dans la documentation de WebView2
(« Differences between Microsoft Edge and WebView2 »), avec ses Ctrl++,
Ctrl+- et Ctrl+0 sous les formes que leur donne Chromium (avec Maj, au pavé
numérique). S'y ajoutent F12, que nomme la référence de
`AreBrowserAcceleratorKeysEnabled` hors de ce tableau, et Ctrl+Maj+P ; Échap
en est retiré : la vue d'une page s'en sert, et le navigateur ne fait
qu'arrêter avec lui une page en cours de chargement. Les autres raccourcis
de ce document sont coupés dans WebView2, comme Ctrl+S ou Ctrl+O, ou sans
objet ici, sauf F6 et Maj+F6 (« Focus Next Pane », « Focus Previous
Pane »), que ce document dit actifs en hébergement fenêtré, celui de wry :
ils restent hors de la liste (`docs/backlog-ui.md`). Un raccourci qui
agirait encore malgré tout s'ajoute à `BROWSER_SHORTCUTS`.

- **Neutraliser n'est pas abandonner.** Seule l'action du navigateur est
  empêchée : l'événement poursuit son chemin jusqu'aux écouteurs de
  l'application, et chaque touche de la liste reste libre pour une commande
  à venir, sauf celles des outils de développement (plus bas). Deux besoins
  en attendent déjà (`docs/backlog-ui.md`) : Ctrl+F, F3 et Ctrl+G, la
  recherche de texte ; Ctrl+P, l'impression du document.
- **Aucune touche de l'application n'est dans la liste** : ni Ctrl+O,
  Ctrl+S, Ctrl+Z, Ctrl+Y ou Ctrl+A, ni F4, les chiffres de la vue, R, Maj+R,
  Échap, Suppr, Retour arrière ou les flèches. Un raccourci n'est reconnu
  qu'avec exactement ses modificateurs : R et Maj+R font toujours pivoter
  quand Ctrl+R est neutralisé, les chiffres restent à la vue quand Ctrl+0
  l'est, et AltGr, que Windows signale comme Ctrl+Alt, tape toujours « @ »
  ou « € », dans un mot de passe comme ailleurs. Alt avec une touche de
  fonction reste à Windows (Alt+F4 ferme la fenêtre). `ui/tests/shortcuts.test.ts`
  le vérifie touche par touche. Dans la grille, Alt+← et Alt+→ déplacent
  pourtant la sélection comme ← et →, le traitement des flèches ne regardant
  pas Alt : une commande sur ces touches demandera d'abord de corriger ce
  point (`docs/backlog-ui.md`).
- **Reconnus par leur code de touche virtuelle de Windows** (`keyCode`),
  comme les reconnaît Chromium, sur lequel WebView2 est construit : `key`
  les manquerait sur une disposition non latine, et `code`, une position sur
  un clavier américain, prendrait d'autres touches pour eux en Bépo ou en
  Dvorak.
- **Outils de développement neutralisés dans tous les builds.** wry les
  coupe déjà en release, mais le build qu'on essaie doit se comporter comme
  celui qu'on livre. Leurs raccourcis sont les seuls que le filtre arrête en
  plus d'en empêcher l'action par défaut. Il arrête aussi ce que Tauri prend
  pour Ctrl+Maj+I : dans un build de développement, Tauri ajoute à la page un
  écouteur qui ouvre les outils sur Ctrl et Maj tenus avec la touche placée
  comme I sur un clavier américain, quels que soient les autres
  modificateurs et la disposition (AltGr+Maj+I, Win+Ctrl+Maj+I, Ctrl+Maj+D en
  Bépo), sans regarder l'action par défaut. Arrêter ces touches n'empêche
  pas ce qu'elles tapent. Constaté le 14 septembre 2026 par DevTools : sans
  cet arrêt, chacune de ces touches ouvrait les outils. Le clic droit
  (« Inspecter ») et le port de débogage de WebView2 les ouvrent toujours
  dans un build de développement. Que la release les garde coupés ne dépend
  pas de l'interface (`docs/backlog-technique.md`).
- **Le zoom reste d'abord à la configuration.** Tauri garde le zoom de
  WebView2 coupé (`zoomHotkeysEnabled`, faux par défaut) : Ctrl+molette,
  pincement et, d'après la documentation de WebView2, Ctrl+plus et
  Ctrl+moins. Un test de `src/main.rs` vérifie qu'aucune fenêtre ne
  l'active, ni dans `tauri.conf.json`, ni dans `tauri.bundle.json`, ni dans
  un fichier de configuration propre à Windows. La liste garde les touches
  de zoom pour le jour où ce réglage changerait ; la molette, non :
  l'intercepter dans l'interface demanderait un écouteur non passif sur
  tout le document, que chaque défilement de la grille attendrait.
- **Mesuré.** La documentation de WebView2 annonce que ses raccourcis
  passent avant la page. Le 14 septembre 2026, avec le runtime WebView2
  152.0.4191.66, F5, F12 et F3 envoyés comme messages de fenêtre à WebView2
  arrivaient d'abord à la page : ils rechargeaient l'interface, ouvraient
  les outils de développement ou la barre de recherche, sauf quand la page
  empêchait leur action par défaut. Ce n'était pas une vraie touche. Les
  combinaisons avec Ctrl ou Alt, dont Ctrl+R et Ctrl+P, n'ont pas pu être
  envoyées ainsi : elles restent à essayer au clavier, et le même essai
  (F5, Ctrl+R, Ctrl+P) dira après chaque mise à jour de WebView2 si c'est
  toujours vrai.
- **Reste actif** le menu contextuel par défaut de WebView2, hors des
  vignettes, avec « Actualiser » et « Imprimer » (`docs/backlog-ui.md`).

## Tests

`cargo test -p fyp-app` : description des fixtures (pages, réparation,
chiffrement, mot de passe faux), ouverture ratée qui laisse le document en
cours ouvert, enregistrement d'un réordonnancement, rotation (relative à la
rotation de chaque page, héritée ou non, ramenée dans 0..360, enregistrée ;
document chiffré tourné en clair ; rotation refusée sans effet ; rotation
appliquée seulement au document qu'elle vise, jamais à un fichier ouvert
entre-temps), emplacements de PDFium (un paquet ne cherche jamais dans le
dépôt dont il vient), dossier `data` qui rend une copie portable,
configuration (fenêtre ouverte par l'application, pas de version propre,
zoom de WebView2 laissé coupé),
et, quand `app/pdfium/` est présent, rendu réel d'une page en
PNG, à la taille d'une vignette et à celle de la vue d'une page, et d'une
page tournée, dessinée couchée.

`python tools/build_ui.py` vérifie les types (`tsgo`), puis exécute les
tests de `ui/tests/` : chaque `*.test.ts` est assemblé par esbuild et lancé
par QuickJS-ng, sans DOM ni Node ; `--check` s'arrête là. Ils portent sur
la logique qui se passe du DOM : les bandeaux à l'ouverture d'un fichier
(échec, mot de passe, succès), l'historique des pages (déplacements et
suppressions annulés sur place, rotations faites et annulées par un côté
Rust simulé, une à la fois, rotation refusée sans effet), le numéro tapé
pour aller à une page (position dans l'ordre actuel, y compris après
déplacements et suppressions ; numéro hors de portée ou qui n'en est pas un ;
légende, aide et refus), le nombre de vignettes demandées à la fois selon
ce que fait la vue d'une page, et les raccourcis du navigateur neutralisés
(chacun de la liste, aucune touche de l'application, aucun raccourci tenu
avec d'autres modificateurs ; arrêtés au filtre, seuls ceux des outils de
développement et ce que Tauri prend pour Ctrl+Maj+I). Le panneau lui-même (disposition, défilement,
clics), le champ du numéro et l'écouteur qui neutralise les raccourcis
passent par le DOM : ils n'y sont pas testés.
