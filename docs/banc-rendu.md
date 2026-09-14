# Banc de fidélité du rendu

L'ADR 0005 fixe le critère de sortie de PDFium : notre moteur devra rendre les
fixtures et le corpus public « avec une fidélité comparable, mesurée par
comparaison d'images sur un jeu de pages de référence ». Le banc est
l'instrument de cette mesure. Deux moteurs dessinent les mêmes pages à la même
largeur ; le banc donne, pour chaque page, l'écart entre les deux images et le
temps de chaque moteur, dessin et encodage PNG séparés. Il connaît deux
moteurs : PDFium, celui de l'application, et hayro, un moteur écrit en Rust
ajouté pour être mesuré contre lui (`docs/mesure-hayro.md`). Un autre s'ajoute
sans toucher au banc (« Ajouter un moteur »).

Le banc est en Rust (`tools/render_bench`). Il doit exécuter le code même de
`app/src/render.rs`, compilé avec les versions exactes du `Cargo.lock` de
l'application, et comparer des millions de pixels par page. Les scripts
Python de `tools/`, qui s'en tiennent à la bibliothèque standard, ne feraient
ni l'un ni l'autre en un temps raisonnable.

## Lancer

Prérequis : PDFium (`python tools/fetch_pdfium.py`) et le corpus public
(`python tools/fetch_corpus.py`). Sans le corpus, ses pages sont « non
mesurées » et le rapport le dit.

```
cargo run --release -p fyp-render-bench
```

Cette commande équivaut à `run --a pdfium --b pdfium`. Elle construit le moteur
PDFium, rend le jeu de pages avec les deux moteurs et écrit :

- `target/render-bench/<date>-<A>-<B>/index.html`, le rapport ;
- à côté, `results.json` (tout le détail) et les images `a/`, `b/` et `diff/`,
  environ 80 Mio par exécution, jamais effacés par le banc ;
- `target/render-bench/latest.html`, qui ouvre le dernier rapport.

Sur la machine de référence (Intel Core i7-11700KF, 16 fils, Windows 11), une
exécution prend 34 s pour les 162 pages, avec deux moteurs et trois
répétitions par page. Le banc refuse un build debug, dont les temps ne valent
rien, sauf avec `--allow-debug`.

| Option | Rôle | Défaut |
|---|---|---|
| `--a`, `--b` | moteurs A (référence) et B (mesuré contre A) | `pdfium`, `pdfium` |
| `--pages` | jeu de pages | `tools/render_bench/pages.toml` |
| `--engines` | fichier des moteurs | `tools/render_bench/engines.toml` |
| `--metric` | `ssim` ou `pixels` | `ssim` |
| `--repeat` | rendus et encodages de chaque page, par moteur | 3 |
| `--limit N` | les N premières pages seulement : un aperçu, pas une mesure | toutes |
| `--timeout` | secondes sans réponse avant d'arrêter un moteur | 120 |
| `--no-build` | ne pas construire les moteurs | |
| `--jobs` | fils de comparaison des images | tous |
| `--record-timings FICHIER` | écrire les temps de référence du moteur A | |
| `--fail-above DISTANCE` | garde de non-régression (« Garde en CI ») | |

Codes de sortie : 0 ; 1 pour une erreur (moteur inconnu, jeu de pages
illisible, construction ratée) ; 3 quand la garde échoue. Les autres commandes
sont `select`, qui choisit un jeu de pages (« Le jeu de pages »), et `engines`,
qui liste les moteurs.

## Ce que fait une exécution

1. Elle vérifie l'empreinte SHA-256 de chaque fichier du jeu. Une page dont le
   fichier manque ou a changé n'est pas rendue : elle est « non mesurée ».
2. Elle construit les moteurs (commande `build` de `engines.toml`).
3. Elle traite les documents un par un. Pour chaque document, elle lance un
   processus par moteur, l'un après l'autre, jamais en même temps. Le moteur
   qui passe en premier alterne d'un document à l'autre. Un moteur qui plante,
   se tait plus de `--timeout` secondes ou répond hors du protocole ne coûte
   que les pages restantes de ce document ; son erreur standard va dans le
   rapport.
4. Elle compare les images sur tous les fils, une fois tous les rendus faits,
   pour ne pas fausser les temps.
5. Elle écrit le rapport. En tête, le verdict, puis les pages, les distances
   et les temps ; ensuite chaque page, de la pire à la meilleure, avec les
   deux images et leur différence. Sont classés d'abord les échecs de B, puis
   ceux de A, puis ceux des deux, puis les pages comparées par distance
   décroissante, et à la fin les pages non mesurées. L'image de différence
   montre A en gris, et en rouge chaque pixel qui diffère, même d'un niveau ;
   le rouge est d'autant plus vif que l'écart est grand.

Deux images de tailles différentes d'un pixel, par l'arrondi de la hauteur,
sont comparées sur leur surface commune. Au-delà d'un pixel, les moteurs ne
voient pas la même page, et la distance vaut 1.

## Les moteurs

Un moteur est un programme décrit dans `tools/render_bench/engines.toml` : sa
commande de construction et sa commande de lancement. Le banc ne connaît que
son protocole (`tools/render_bench/src/protocol.rs`, version 1) :

- le banc écrit sur l'entrée standard du moteur une demande en JSON : le
  document, son mot de passe, la largeur, le nombre de répétitions, et pour
  chaque page son numéro et le fichier PNG à écrire ; puis il ferme l'entrée ;
- le moteur répond sur sa sortie standard, une ligne JSON par réponse : il se
  présente (nom, version, détail), dit s'il a ouvert le document et en combien
  de temps, puis rend compte de chaque page (taille, temps de chaque
  répétition, répétitions identiques ou non) ou de son échec ; une réponse
  `fatal` arrête tout ;
- les temps sont pris par le moteur autour de ses propres appels : ni le
  lancement du processus ni la lecture du fichier ne comptent. Le rendu va de
  la page demandée à ses pixels en mémoire, 8 bits par canal ; l'encodage est
  celui de ces pixels en PNG, comme le fait le service de pages de
  l'application (compression rapide, filtre `Up`).

Le moteur PDFium (`tools/render_bench/engines/pdfium`) compile
`app/src/render.rs` tel quel et en appelle les trois étapes une à une :
`Renderer::open`, `Loaded::draw` et `encode_png`. Le fil de travail de
l'application enchaîne ces mêmes étapes pour chaque demande. Le moteur cherche
la bibliothèque dans `FYP_PDFIUM_DIR`, puis dans `app/pdfium/` : la version
épinglée, jamais une copie restée à côté d'un exécutable. Il donne pour
version le contenu du fichier `RELEASE` que `tools/fetch_pdfium.py` écrit à
côté de la bibliothèque, et le début de l'empreinte de celle-ci. Un test
(`tests/engine.rs`) vérifie qu'il compile `render.rs` avec les dépendances et
les fonctionnalités de `app/Cargo.toml`.

Le moteur hayro (`tools/render_bench/engines/hayro`) dessine avec hayro 0.7.1,
épinglé, et ses crates `hayro-interpret` et `hayro-syntax`. Son module
`src/render.rs` a la forme du service de pages de l'application : ouvrir,
dessiner, encoder. Le document est ouvert par `fyp-core` avec le mot de passe ;
hayro lit ensuite les octets du fichier ou, si le fichier est chiffré, la
réécriture en clair qu'en fait le noyau : le mot de passe ne lui parvient
jamais. hayro ne rend pas d'erreur pendant le dessin et peut paniquer : chaque
appel est fait sous `catch_unwind`, si bien qu'une panique fait échouer une
page, pas le document. La hauteur de l'image est arrondie comme le fait
PDFium. Ses tests (`tests/engine.rs`) vérifient qu'il encode le PNG avec les
réglages du service de pages et la même crate `image`, qu'il refuse un mauvais
mot de passe au noyau, et que la version qu'il annonce est celle de
`Cargo.lock`.

Le moteur factice (`fyp-render-engine-fake`) sert aux tests du banc. Il suit le
même protocole sans lire le document, et peut planter, se taire, répondre du
charabia ou refuser une page.

### Ajouter un moteur

1. Écrire un programme qui suit le protocole. En Rust, dépendre de
   `fyp-render-bench` pour ses types (`protocol::Request`, `protocol::Reply`) ;
   le moteur PDFium sert de modèle. Il doit encoder le PNG comme le service de
   pages, sans quoi ses temps d'encodage ne se comparent pas à ceux de PDFium.
2. Le décrire dans `engines.toml`, sous un nom en minuscules.
3. `cargo run --release -p fyp-render-bench -- run --a pdfium --b <nom>`.

Le banc n'est pas modifié.

## Le jeu de pages

`tools/render_bench/pages.toml` est versionné : deux exécutions à des semaines
d'écart mesurent les mêmes pages, et un fichier du corpus qui aurait changé
est signalé au lieu d'être mesuré. Chaque page porte son fichier, l'empreinte
de celui-ci, son numéro, le mot de passe s'il en faut un, et les raisons de sa
présence (`why`). Son en-tête dit comment il a été choisi, et à partir de quels
commits du corpus.

Le jeu du 13 septembre 2026 compte 162 pages de 157 fichiers, à 1400 pixels de
large, la largeur à laquelle l'application montre une page en grand :

- **les 20 fixtures**, une page chacune. Elles sont presque vides, mais ce
  sont les structures de fichier que le noyau gère : tables reconstruites,
  `startxref` décalé, flux de références croisées, fichiers hybrides,
  chiffrement RC4 et AES-256 ;
- **102 pages du corpus pour couvrir 141 étiquettes**, deux pages par
  étiquette quand le corpus en a deux. Les étiquettes décrivent ce qu'un
  moteur peut mal rendre : filtres des images et des contenus (Flate, LZW,
  DCT, JPX, JBIG2, CCITT, RunLength, ASCII), polices (Type 1, Multiple Master,
  Type 1C, TrueType, OpenType, Type 3, CID, polices non incorporées, dont les
  14 standard), encodages et CMap, espaces de couleur (Device, Cal, Lab, ICC à
  1, 3 et 4 composantes, Indexed, Separation, DeviceN, NChannel, Pattern),
  ombrages de types 1 à 7, fonctions de types 0, 2, 3 et 4, motifs,
  transparence (alpha constant, masques souples, groupes de transparence, à
  élimination compris), modes de fusion, surimpression, fonctions de
  transfert, trames, images masquées ou à 1, 4 et 16 bits par composante,
  images en ligne, modes de rendu du texte, contenus optionnels, annotations
  avec ou sans apparence, pages tournées, `CropBox`, `UserUnit`, fichiers
  réparés, fichiers chiffrés, dont deux protégés par un mot de passe
  utilisateur. Un nom que la norme ne définit pas compte pour une seule
  étiquette, `unknown` ;
- **16 pages parmi les plus lourdes**, une par fichier, estimées au volume de
  leur contenu et au nombre de pixels de leurs images. Les pages qui couvrent
  les étiquettes sont prises au moins coûteux et ne ressemblent guère à des
  documents réels ; celles-ci donnent des temps de pages chargées. La plus
  lente est un plan A1 de `pdfjs/22060_A1_01_Plans.pdf` (1,4 s par rendu) ;
- **24 pages tirées au hasard**, 8 par lot (pdf.js, qpdf, veraPDF), avec une
  graine fixe.

Une page du corpus n'entre que si PDFium la rend en 3 s au plus, et si son
image garde une hauteur entre 32 pixels et quatre fois sa largeur ; seules
les 8 premières pages de chaque fichier sont candidates, et les 62 fichiers
que le noyau refuse d'ouvrir ne le sont pas du tout. Les fixtures entrent
toutes.

La taille du jeu suit le temps. Les 34 s d'une exécution se répartissent
ainsi : 24 s de rendu et d'encodage (trois répétitions, deux moteurs), 7 s
pour lancer 314 processus (157 documents, deux moteurs, 24 ms chacun en
moyenne), et 2 s de comparaison. `select` prend 8 s. Le jeu peut encore grossir de plusieurs
fois avant de gêner. Pour le choisir de nouveau :

```
cargo run --release -p fyp-render-bench -- select
```

Options : `--per-feature`, `--max-per-file`, `--sample-per-lot`, `--heavy`,
`--max-pages-scanned`, `--max-render-ms`, `--width`. À corpus et réponses de
PDFium identiques, le choix est le même. Mais un nouveau jeu rompt toute
comparaison avec les rapports et les temps de référence précédents : ne le
faire que pour une raison écrite dans le commit, et réenregistrer les temps
dans la foulée.

## La métrique

La distance par défaut, `ssim`, vaut `1 − SSIM moyen`. C'est la similarité
structurelle de Wang, Bovik, Sheikh et Simoncelli (2004), calculée comme leur
code de référence : l'image est réduite par moyenne de blocs, pour que son
petit côté fasse environ 256 pixels (facteur 5 pour une page de 1400 pixels
de large), puis comparée par une fenêtre gaussienne 11 × 11 d'écart type 1,5
à chaque position. La similarité est calculée sur les canaux rouge, vert et
bleu, puis moyennée ; une similarité négative compte pour 0. La distance vaut
0 pour deux images identiques, et 1 pour deux images sans structure commune.

Une différence de pixels, même pondérée, et SSIM en pleine résolution ne
conviennent pas. Deux moteurs ne tracent jamais le bord d'un glyphe ou d'un
trait fin avec le même anticrénelage, ni au même sous-pixel. En pleine
résolution, ces écarts touchent tous les bords de la page et pèsent autant
qu'un contenu manquant, ou davantage. Mesure du 13 septembre 2026, faite avec
un programme jetable hors du dépôt : PDFium rend une même page à 1400 pixels
avec des réglages différents, et chaque variante est comparée au rendu
normal.

| Page | Variante | SSIM réduit (retenu) | SSIM pleine résolution, fenêtres 8 × 8 | Différence pondérée |
|---|---|---:|---:|---:|
| `pdfjs/TAMReview.pdf`, texte dense | texte sans anticrénelage | 0,0108 | 0,0380 | 0,0166 |
| | tout décalé d'un demi-pixel | 0,0122 | 0,0435 | 0,0172 |
| | bloc de 4 % de la page effacé | **0,0201** | 0,0097 | 0,0021 |
| `pdfjs/160F-2019.pdf`, formulaire | texte sans anticrénelage | 0,0102 | 0,0381 | 0,0166 |
| | tout décalé d'un demi-pixel | 0,0425 | 0,0822 | 0,0281 |
| | sans annotations ni données de formulaire | **0,0656** | 0,0584 | 0,0229 |
| | bloc de 4 % effacé | 0,0264 | 0,0113 | 0,0028 |
| `pdfjs/S2.pdf`, couleur | tout décalé d'un demi-pixel | 0,0053 | 0,0174 | 0,0104 |
| | niveaux de gris | **0,1898** | 0,1630 | 0,0938 |
| | bloc de 4 % effacé | 0,0306 | 0,0187 | 0,0181 |
| `pdfjs/22060_A1_01_Plans.pdf`, plan au trait | traits et images sans anticrénelage | 0,0949 | 0,1064 | 0,0249 |
| | tout décalé d'un demi-pixel | 0,0407 | 0,1740 | 0,0290 |
| | bloc de 4 % effacé | 0,0262 | 0,0223 | 0,0072 |

La différence pondérée est la moyenne, sur les pixels, du plus grand écart
entre canaux. Sur le texte dense, elle et SSIM en pleine résolution font peser
l'anticrénelage plusieurs fois plus lourd qu'un paragraphe effacé ; SSIM
réduit rétablit l'ordre. Chaque canal compté à part, un changement de couleur
de même luminance reste visible.

Ce que la mesure ne corrige pas : sur un formulaire ou un plan fait de traits
fins, un placement au sous-pixel ou un anticrénelage différents pèsent autant
qu'un bloc effacé de 4 %, même réduits. SSIM réduit voit mal, aussi, un détail
isolé, un glyphe par exemple, que la réduction mêle au blanc qui l'entoure. La
métrique `pixels`, la part des pixels qui diffèrent sans aucune tolérance, et
l'image de différence du rapport montrent ces écarts.

**Ce qu'un seuil voudra dire.** La distance est une surface dissemblable
pondérée : 0,02 correspond à peu près à 4 % d'une page de texte effacée,
0,19 à une page en couleur passée en niveaux de gris. Un seuil par page ne
s'interprète qu'une fois connu le bruit de fond d'un second moteur. Entre
PDFium et un autre moteur, l'anticrénelage et le placement des traits fins
laisseront une distance non nulle sur presque toutes les pages, jusqu'à 0,04
ou 0,09 sur les pages de traits d'après le tableau. Un seuil fixé en dessous
de ce bruit échouerait partout.

**Changer de métrique** : implémenter le trait `Metric`
(`tools/render_bench/src/metric.rs`), l'ajouter à `NAMES` et à `by_name`, puis
passer `--metric`. Ni les moteurs, ni les rapports, ni le jeu de pages ne
changent.

## Temps de référence

Par page et par moteur, le banc mesure :

- **l'ouverture** du document, une fois par processus ;
- **le rendu** : la page demandée, dessinée, ses pixels en mémoire ;
- **l'encodage** de ces pixels en PNG.

Chaque page est rendue et encodée `--repeat` fois, 3 par défaut ; le rapport
prend la médiane. `tools/render_bench/timings/pdfium.toml` garde les temps de
référence de PDFium : par page, la médiane de rendu et d'encodage et leurs
extrêmes, sur les 6 échantillons des deux processus. La table `[run]` dit sur
quelle machine, avec quel compilateur, quelle révision et quel jeu de pages
(empreinte comprise) ils ont été pris. Ils ne valent que là : sur une autre
machine, comparer les deux moteurs d'une même exécution, pas les temps d'un
rapport à ceux de ce fichier. Pour les réenregistrer :

```
cargo run --release -p fyp-render-bench -- run --record-timings tools/render_bench/timings/pdfium.toml
```

Bruit de la mesure, PDFium contre lui-même d'un processus à l'autre : l'écart
relatif du temps de rendu d'une même page est de 2,8 % en médiane, 8,9 % au
90e centile et jusqu'à 30 % sur une page de quelques millisecondes.

**Le rendu domine, pas l'encodage.** En release, sur les 156 pages comparées,
l'encodage PNG prend 5,3 % du temps de rendu et d'encodage : 213 ms contre
3 779 ms. Par page, la médiane est de 1,1 ms pour l'encodage et de 3,1 ms pour
le rendu. Au pire, l'encodage prend 11,6 ms, et le rendu 1 388 ms, le plan A1.
La mesure citée par `app/src/render.rs`, 190 ms pour encoder une page de
1400 pixels, venait d'un build debug, où `image` et `png` ne sont pas
optimisés ; `docs/architecture.md` la reprend sans le préciser
(`docs/backlog-technique.md`).

## Ligne de base : PDFium contre PDFium

Exécution du 13 septembre 2026, avec la commande par défaut. Les 156 pages
dessinées par PDFium sont identiques au pixel près d'un processus à l'autre, et
d'une répétition à l'autre dans chaque processus : écart nul, rendu
déterministe sur cette machine.

Les 6 autres pages sont des fixtures que PDFium ne rend pas, des deux côtés :
il refuse d'ouvrir `garbage-xref.pdf`, `no-header-junk.pdf`, `no-header.pdf`,
`root-dangling.pdf` et `root-direct.pdf` (`FormatError`), et n'ouvre pas la
page de `hybrid.pdf`. Le noyau ouvre les six. C'est la divergence des deux
parseurs que décrit l'ADR 0005 ; pour ces pages, PDFium ne donne aucune image
de référence.

## Garde en CI

Le job `render-fidelity` de `.github/workflows/ci.yml` est écrit mais inactif
(`if: false`). Il récupère PDFium et le corpus, lance le banc avec
`--fail-above`, et publie le rapport. Le banc sort avec le code 3 si une page
dépasse la distance, si B échoue là où A réussit, si une page n'est pas
mesurée, ou si le jeu est partiel (`--limit`). B peut être PDFium lui-même,
contre qui le seul seuil connu est zéro, ou hayro, contre qui aucun seuil n'est
fixé (`docs/mesure-hayro.md`).

Pour l'activer, il manque :

- **le moteur à garder** : sans lui, la garde vérifie seulement le
  déterminisme de PDFium ;
- **un seuil mesuré contre ce moteur** au-dessus de son bruit de fond
  (« La métrique »). Sans doute un seuil par page plutôt qu'un seul : une
  distance enregistrée par page, avec une tolérance, empêche une page de
  régresser sans exiger que toutes soient déjà bonnes ;
- **la stabilité de la métrique sur le runner** : sous Linux, PDFium est une
  autre compilation, et les polices non incorporées y sont remplacées par
  d'autres polices système que sous Windows. Le déterminisme et le bruit ne
  sont vérifiés que sur la machine de référence ; les seuils sont à mesurer
  sur le runner lui-même ;
- **le corpus dans le job** : trois dépôts téléchargés, 316 Mio de PDF gardés,
  à mettre en cache, avec un jeton contre la limite de l'API de GitHub ;
- **le temps** : 34 s pour le banc sur la machine de référence, plus la
  compilation et les téléchargements ; à mesurer sur le runner. Le rapport
  publié pèse environ 80 Mio.

## Limites connues

- Les temps ne se comparent que sur une même machine.
- Le jeu de pages dépend de ce que le noyau lit : un fichier qu'il refuse
  n'est jamais candidat, et les étiquettes viennent de sa lecture des pages.
- Une page que PDFium ne rend pas n'a pas d'image de référence : ni la
  distance ni la garde ne disent rien d'elle.
- Le banc n'efface pas ses rapports de `target/render-bench/`.
- Les tests du banc tournent sans PDFium, avec le moteur factice ; ceux du
  moteur PDFium vérifient seulement son refus quand la bibliothèque manque,
  ce qui est le cas dans la CI.
