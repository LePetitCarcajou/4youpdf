# Mesure de hayro contre PDFium

L'ADR 0005 retire PDFium quand un autre moteur rend les fixtures et le corpus
public « avec une fidélité comparable ». Cette mesure applique ce critère à
hayro (`github.com/LaurenzV/hayro`), un rastériseur PDF écrit en Rust, sans
basculer le moteur de l'application : elle rend les pages du banc de fidélité
(`docs/banc-rendu.md`) avec PDFium en A et hayro en B, puis examine les écarts,
les temps, les fichiers cassés, l'accès au texte et la compilation pour
WebAssembly. L'application dessine toujours avec PDFium ; `app/` n'a pas
changé.

Exécutions des 13 et 14 septembre 2026 (UTC), sur la machine de référence du
banc (Intel Core i7-11700KF, 16 fils, Windows 11, rustc 1.98.1) : hayro 0.7.1,
hayro-interpret 0.7.0, hayro-syntax 0.7.2 ; PDFium chromium/8044. Le rapport
du banc, avec les images de chaque page et leurs différences, est dans
`target/render-bench/20260913-232855-pdfium-hayro/index.html` : environ
80 Mio, non versionné, jamais effacé par le banc.

## Recommandation

**Basculer avec réserves.** hayro tient les trois principes que PDFium
contredit, et sa fidélité est comparable sur les pages qui ressemblent à des
documents réels. Mais il perd du contenu sur des cas précis et fréquents :
annotations sans apparence, états des cases à cocher, modes de fusion dans des
groupes non isolés. Il fait aussi avorter son processus sur un fichier réparé,
et met plus d'une minute à dessiner deux pages du corpus. Ces points doivent
être levés avant la bascule ; ils se mesurent avec le banc.

Ce qui plaide pour la bascule :

- **Les principes.** Aucun code C ou C++, rien à télécharger hors de
  crates.io, `unsafe` interdit dans sept des huit crates de hayro (licences
  Apache-2.0 ou MIT, « Sûreté et licences »). PDFium, lui, est du C++, lié par
  17 181 occurrences de `unsafe` dans `pdfium-render`.
- **La fidélité sur les pages réalistes.** Sur les 24 pages tirées au hasard,
  distance médiane 0,0001 et maximale 0,036, dues à l'anticrénelage du texte.
  Une seule y perd du contenu : un formulaire dont les cases cochées
  paraissent vides. Sur les 16 pages les plus lourdes, médiane 0,0017.
- **Les fichiers cassés.** Sur la première page des 4 493 fichiers du corpus
  que le noyau ouvre, hayro en rend 15 que PDFium ne rend pas. PDFium en rend 4
  que hayro ne rend pas en 60 s, détaillés aux conditions ci-dessous. Quand le
  moteur lit la réécriture du noyau, l'écart se referme dans les deux sens
  (« Les six fixtures que PDFium refuse »).
- **La page la plus lourde.** Le plan A1 est rendu en 328 ms au lieu de
  1 346 ms.
- **Le déterminisme.** Les 161 pages que hayro rend sont identiques au pixel
  d'un processus à l'autre : une garde en CI par page est possible.
- **Le mot de passe.** Il ne quitte plus le noyau : hayro dessine ce que
  `fyp-core` a ouvert et déchiffré.
- **Le texte.** Les positions des glyphes viennent de la même interprétation
  que l'image : recherche et sélection tomberaient au pixel près.
- **WebAssembly.** hayro compile pour wasm32-wasip1, avec les six mêmes
  importations WASI que le module de fusion.

Conditions à lever avant de basculer :

1. **Aucun contenu perdu sur les annotations et les formulaires.** 18 pages
   du jeu perdent ou faussent du contenu (« Écarts par cause »). En tête, les
   annotations sans apparence : 11 pages, où hayro ne dessine rien. Viennent
   ensuite les états `/AS`, avec lesquels les cases cochées paraissent vides,
   et les groupes non isolés, où un mot disparaît sous un surlignage.

   Deux voies, cumulables :
   - corriger hayro en amont, puisque ses crates notent elles-mêmes « blending
     and isolation » parmi leurs limites ;
   - faire écrire par le noyau les apparences manquantes quand il réécrit un
     fichier, travail que la conformité PDF/A exigera de toute façon : les
     fichiers veraPDF du corpus « 6.3.3 Annotation appearances » l'éprouvent.

   Critère : ces pages reviennent au niveau du bruit dans le banc.
2. **Un rendu qui ne fait ni tomber ni figer l'application.**
   - hayro dépasse sa pile en ouvrant `qpdf/issue-202.pdf`, un fichier que le
     noyau répare. C'est un abandon du processus, que `catch_unwind` ne
     rattrape pas, alors qu'il dessine la page sans erreur depuis la
     réécriture du noyau.
   - Il met 55 s à dessiner la première page de `pdfjs/bug1978317.pdf`, que
     PDFium dessine en 0,53 s, et 80 s pour `pdfjs/issue16263.pdf`, contre
     2,2 s.

   Dans l'application, le fil de rendu vit dans le processus de la fenêtre. Il
   faut au moins trois choses : donner à hayro la réécriture du noyau pour tout
   fichier réparé, borner le temps d'un dessin, et pouvoir l'abandonner. Au
   mieux, isoler le rendu dans un processus à part ou dans la sandbox
   WebAssembly, où hayro compile. Chaque cas devient une fixture et un test
   (CLAUDE.md).
3. **Une garde de non-régression active.** hayro est en version 0.x, avec un
   seul auteur déclaré dans ses manifestes. Chaque montée de version doit donc
   passer le banc, avec des seuils par page mesurés contre hayro sur le runner
   de la CI (`docs/banc-rendu.md`, « Garde en CI »).

Réserves qui n'empêchent pas de basculer, à suivre après :

- **Les pics de temps.** Six pages du jeu dépassent 250 ms, contre deux avec
  PDFium. Le temps y part dans les masques souples rendus à la taille de la
  page, les nuances évaluées pixel par pixel, la conversion ICC d'images et la
  réduction d'une image de 212 millions de pixels. L'annulation qu'exige déjà
  l'ADR 0004, un rendu progressif et le rendu multifil de vello_cpu (non
  mesuré) sont les leviers.
- **Les écarts imparfaits mais lisibles.** Couleurs CMJN, traits fins plus
  clairs sur les images réduites, glyphes de substitution des polices non
  incorporées, fonctions de transfert ignorées.

Ce qui ferait renoncer :

- un correctif des groupes non isolés trop invasif pour être porté en amont ou
  gardé dans une copie ;
- une lenteur des deux fichiers du corpus qui révélerait un défaut structurel
  que l'amont ne corrige pas ;
- un isolement du rendu dont le coût casse le budget de l'ADR 0004.

## Le moteur ajouté au banc

`tools/render_bench/engines/hayro` suit le protocole du banc comme le moteur
PDFium, avec une entrée de plus dans `engines.toml` ; le banc n'a pas changé.
Son module `src/render.rs` a la forme du service de pages : ouvrir, dessiner,
encoder, chronométrés séparément. Pour lire ses temps :

- l'ouverture compte celle du noyau, plus la réécriture en clair d'un fichier
  chiffré. En médiane, elle prend 0,01 ms de moins que celle de PDFium, et au
  plus 2,06 ms de plus ;
- le cache de hayro (polices, contours de glyphes) est gardé d'un dessin à
  l'autre pour un même document, comme PDFium garde un document chargé. La
  première répétition prend 1,06 fois le temps des suivantes en médiane, 4,43
  au plus ;
- vello_cpu dessine sur un seul fil ;
- la hauteur de l'image est arrondie comme le fait PDFium : aucune page du jeu
  n'a de tailles différentes ;
- le PNG est encodé comme par le service de pages, et un test le vérifie.

Ajouter le moteur au workspace a ajouté 27 paquets à `Cargo.lock` sans changer
aucune version. L'arbre résolu de `fyp-app`, paquets et fonctionnalités pour
toutes les cibles, est inchangé. Seul un build du workspace entier compile
quatre crates partagées avec des fonctionnalités de plus : `bytemuck`,
`hashbrown`, `yoke` et `zerofrom`. `cargo deny check` passe. La rust-version la
plus haute de l'arbre de hayro est 1.92, sous le MSRV du produit (1.95).

## Chiffrement

Le moteur ouvre le document avec `fyp-core`, qui reçoit le mot de passe. Si le
fichier est chiffré, hayro lit la réécriture en clair qu'en fait le noyau
(`fyp_core::writer`) ; sinon il lit les octets du fichier. Le mot de passe ne
lui parvient jamais. Les dix pages chiffrées du jeu sont rendues :

| Chiffrement | Pages | Distance maximale |
|---|---|---:|
| RC4 | 3, 103, 116 | 0,0001 |
| AES-128 | 29, 86 | 0,0148 (page 29 : widgets sans apparence, pas le chiffrement) |
| AES-256 | 2, 36, 151 | 0 |
| Révision 6, filtres `/Identity`, mot de passe utilisateur « attachment » (`qpdf/enc-XI-R6,V5,U=attachment,encrypted-attachments.pdf`, pages 1 et 2) | 93, 94 | 0,0001 |

Les tests du moteur vérifient qu'un mauvais mot de passe est refusé par le
noyau avant que hayro lise quoi que ce soit. Le mot de passe utilisateur et le
mot de passe propriétaire y donnent le même PNG.

**Une prémisse à corriger.** hayro sait ouvrir un PDF protégé :
`hayro-syntax` 0.7.2 a `Pdf::new_with_password` et son propre RC4 et AES, même
si sa documentation dit encore le contraire. Essayé à part sur les dix pages
et sur les fichiers bruts, ce déchiffrement donne des images identiques à
celles du chemin par le noyau, au pixel près. Passer par le noyau n'est donc
pas une nécessité mais un choix. Le mot de passe reste dans le noyau, et une
seule implémentation déchiffre : `fyp-crypto`, sur les crates RustCrypto, avec
ses tolérances. La conséquence de l'ADR 0005 « Mot de passe transmis à
PDFium » disparaîtrait avec ce chemin. Elle disparaîtrait aussi avec PDFium si
`app/src/render.rs` lui passait la réécriture du noyau.

## Fidélité

| | Pages |
|---|---:|
| Jeu | 162 |
| Comparées | 156 |
| Identiques au pixel | 41 |
| PDFium échoue, hayro rend | 5 |
| Les deux échouent | 1 |
| Distance > 0,001 | 53 |
| Distance > 0,01 | 23 |
| Distance > 0,02 | 13 |
| Distance > 0,05 | 5 |
| Distance > 0,1 | 4 |

La distance est `ssim` : médiane 0,00012, 90e centile 0,0173, maximum 0,7054.
Selon le jeu :

- **24 pages tirées au hasard** : médiane 0,0001, maximum 0,036 ;
- **16 pages les plus lourdes** : médiane 0,0017, maximum 0,040 ;
- **14 fixtures comparées** : toutes à distance nulle, dont 12 identiques au
  pixel.

La métrique voit mal un détail isolé. 83 pages ont donc été regardées, chacune
à côté de son image PDFium et de la différence, avec des recadrages en pleine
résolution. Ce sont 49 des 53 pages à plus de 0,001, et celles dont les
étiquettes visaient une cause. Cet examen visuel a été fait pendant la mesure
par l'agent qui la conduisait, sans relecture humaine. Les classements
ci-dessous reposent sur lui, et sur les objets PDF lus par le noyau pour les
pages litigieuses. Des quatre pages restantes, deux sont la
jumelle d'une page vue (112 de 111, 140 de 141) et deux n'ont été vues que
dans l'image de hayro (69, 84). Une recherche des zones denses de pixels
écartés de plus de 128 niveaux, sur les 156 pages, n'en a trouvé aucune hors
des pages classées ci-dessous.

hayro contre hayro, dans une seconde exécution, rend les 161 pages qu'il
dessine identiques au pixel d'un processus à l'autre et d'une répétition à
l'autre.

## Écarts par cause

Les pages sont numérotées comme dans `pages.toml` et le rapport du banc. Les
étiquettes sont celles du scanner du banc (`select::scan_file`), complètes : le
champ `why` de `pages.toml` n'en garde qu'une partie. Sur les 156 pages
comparées, 128 ne diffèrent que par le bruit (anticrénelage, placement au
sous-pixel, rééchantillonnage) ; les 28 autres se répartissent ainsi.

| Cause | Pages étiquetées | Pages touchées | Effet | Gravité |
|---|---:|---|---|---|
| Annotations sans apparence | 27 | 11 : 23, 26, 27, 29, 30, 32, 47, 135, 140, 141, 147 | hayro ne dessine rien : surlignage, soulignement, barré, ondulé, carré, cercle, texte libre, icône de note, cases et fonds de champs. Sur les 16 autres pages, PDFium ne dessine rien non plus. | Inutilisable pour ces annotations |
| États des widgets (`/AS`) | 6 (`annotation:Widget`) | 1 : 115 | Une apparence `/AP /N` faite d'états n'est pas choisie : ni coche ni bouton radio sélectionné | Inutilisable pour un formulaire rempli |
| Modes de fusion | 10 | 0 en eux-mêmes | Les 16 modes sont justes sur `blendmode.pdf` (page 24, distance 0,0033), à quelques niveaux près sur ColorDodge et ColorBurn | Imparfait |
| Groupes isolés ou à élimination | 9 groupes, 3 à élimination, 11 groupes de page | 4 : 48, 50, 157 ; 52 | Un groupe non isolé, ou la page elle-même, est traité comme isolé : un mode de fusion ne voit pas le fond. Page 157, hayro dessine en noir les 12 carrés, PDFium 5 ; sur une page blanche, la plupart de ces modes doivent laisser le blanc. Page 50, un surlignage `Multiply` dans un groupe masque le mot « Trace-based », alors que le carré `Multiply` sans groupe de la même page est juste. Page 48, des formes `Screen` à 20 et 41 % deviennent sombres, probablement par le même mécanisme. Page 52, masques de luminosité et groupes à élimination en CMJN : un dégradé orange remplace des pictogrammes, sans que la cause exacte soit isolée. Le groupe à élimination de la page 76 est juste. | Inutilisable (texte masqué) à faux |
| JPEG 2000 | 3 : 44, 56, 75 | 1 non tranché : 75 | Logo (44) et image de 12 608 × 16 806 pixels (56) justes. Page 75, un carré sur huit (`SMaskInData 1`) diffère, sans troisième moteur pour dire lequel a raison. | Imparfait au plus |
| Polices | Type 1 : 52, TrueType : 12, CID : 15, Type 3 : 2, MM : 2, OpenType : 2, non incorporées : 47 | 2 : 121, 130 | Substitut différent pour un programme de police absent ou inconnu : forme des glyphes. Type 3, Multiple Master, CFF, CID et OpenType sont au niveau du bruit. | Imparfait, lisible |
| Espaces de couleur | 30 hors Device | 3 : 133 ; 101 ; 41 non tranché | Page 133, un motif non coloré (`PaintType 2`) sur un espace CMJN avec `DefaultCMYK` ICC : rien n'est dessiné. Page 101, image CMJN aux couleurs un peu autres (profil ICC contre formule). Page 41, nuances de fonction en CMJN et Separation aux couleurs très différentes, non tranché. Lab, CalRGB, CalGray, ICC à 1, 3 et 4 composantes, Indexed et DeviceN sont justes ailleurs. | Inutilisable (133) à imparfait |
| Ombrages | 11 (types 1 à 7) | 0 hors la page 41 | Justes au bruit près, mais lents : évalués pixel par pixel (« Performance ») | Juste |
| Autre : fonctions de transfert | 2 | 1 : 85 | `/TR` et `/TR2` ignorés : 7 carrés sur 16 d'une autre couleur | Couleurs fausses |
| Autre : image 16 bits avec prédicteur PNG | 2 | 1 : 113 | Le bas de l'image manque ; la même image sans prédicteur (114) est juste | Contenu partiel |
| Autre : traits fins | — | 2 : 21, 61 | Page 21 (plan A1), quatre images JPEG de 2 480 × 2 630 pixels, réduites environ sept fois, gardent des traits plus clairs que chez PDFium ; page 61, grille plus claire | Imparfait, lisible |
| Autre : widgets sans `/AcroForm` | — | 2 : 46, 87 | hayro dessine l'apparence des champs, que PDFium omet sans formulaire déclaré. hayro a raison (ISO 32000-2, 12.5.5) ; l'application a donc aujourd'hui ce défaut (`docs/backlog-technique.md`) | En faveur de hayro |

Sont au niveau du bruit : les filtres (Flate, LZW, ASCII, RunLength, DCT,
JBIG2 dont la « bombe » de 100 000 × 100 000, CCITT) ; les images à 1, 4 et
16 bits, à masque, à clé de couleur, à tableau `/Decode`, en ligne ; les masques
souples alpha ; la surimpression et les trames ; les modes de rendu du texte
de 1 à 7 ; les XObjects PostScript ; les rotations, `CropBox`, `UserUnit` ; les
fichiers réparés et chiffrés.

En résumé : 18 pages perdent ou faussent du contenu (11 annotations, 1
formulaire, 4 groupes ou masques, 133, 113), 1 a des couleurs fausses, 5 sont
imparfaites et lisibles, 2 non tranchées, 2 meilleures avec hayro. Le jeu
surreprésente ces cas : il a été choisi pour couvrir les étiquettes, avec
PDFium comme référence, si bien qu'aucune page que PDFium ne rend pas n'y est
entrée.

## Les six fixtures que PDFium refuse

Ce sont des pages vides au format A4, sans contenu. « Rendre » y signifie
trouver l'arbre des pages et la taille de la page. Elles ne disent rien du
dessin.

| Fixture | PDFium, fichier brut | hayro, fichier brut | PDFium, réécriture du noyau | hayro, réécriture du noyau |
|---|---|---|---|---|
| `garbage-xref.pdf` | refus (`FormatError`) | rendue | rendue | rendue |
| `hybrid.pdf` | page en échec (`Unknown`) | rendue | rendue | rendue |
| `no-header.pdf` | refus | rendue | rendue | rendue |
| `no-header-junk.pdf` | refus | rendue | rendue | rendue |
| `root-dangling.pdf` | refus | rendue | rendue | rendue |
| `root-direct.pdf` | refus | refus (`Invalid`) | rendue | rendue |

hayro rend cinq des six fichiers bruts, par sa propre reconstruction. Ses cinq
images sont identiques au pixel à celles que les deux moteurs tirent de la
réécriture du noyau (`fyp rewrite`) : une page blanche de 1 400 × 1 981
pixels. Par la réécriture, les six passent dans les deux moteurs. La promesse
« ouvre les PDF cassés » atteint donc l'image dès que le moteur dessine ce que
le noyau a lu, avec hayro comme avec PDFium.

**Sur tout le corpus**, hors jeu de référence, la première page des
4 529 fichiers a été demandée à chaque moteur, 60 s au plus par fichier, sur
quatre fils. Le noyau en refuse 36 : 26 mots de passe inconnus, 4 sans
en-tête, 5 tables inutilisables, 1 `/Encrypt` invalide. Sur les 4 493 qu'il
ouvre :

| | Fichiers |
|---|---:|
| Rendus par les deux | 4 453 |
| Par hayro seul | 15 |
| Par PDFium seul | 4 |
| Par aucun | 21 |

- **Les échecs de hayro (25)** : 16 fichiers qu'il n'ouvre pas (`Invalid`),
  5 où il ne trouve aucune page, 1 page de taille dégénérée, 1 abandon par
  dépassement de pile (`qpdf/issue-202.pdf`), et 2 délais dépassés. Ces deux
  derniers sont `pdfjs/bug1978317.pdf` (65 563 objets) et
  `pdfjs/issue16263.pdf`. Mesurés seuls, hayro dessine leur première page en
  55 et 80 s, PDFium en 0,53 et 2,2 s.
- **Les échecs de PDFium (36)** : 20 refus (`FormatError`), 9 pages hors de
  l'arbre, 4 erreurs internes, 2 mots de passe refusés, 1 délai dépassé
  (`qpdf/shared-unnamed-field.pdf`).
- **Paniques** : aucune de hayro, dans le jeu comme dans le corpus.

**Par la réécriture du noyau.** Les fichiers qu'un seul moteur ne rendait pas
ont été réécrits par `fyp rewrite` et redonnés aux moteurs.

Les 25 échecs de hayro, redonnés aux deux moteurs :

- 9 n'ont aucun catalogue lisible : le noyau ne peut pas les réécrire, et
  PDFium ne les rend pas non plus ;
- 6 n'ont de première page dessinable pour aucun moteur, avant comme après
  réécriture : arbre vide ou trop profond, page de taille dégénérée ;
- 4 restent refusés par hayro une fois réécrits, et PDFium ne rend pas non
  plus leur première page ;
- 3 que hayro refuse bruts sont rendus par les deux moteurs une fois
  réécrits : `pdfjs/issue15893_reduced.pdf`, `pdfjs/issue9105_other.pdf`
  (d'où vient la fixture `root-direct.pdf`) et `qpdf/issue-202.pdf`, celui du
  dépassement de pile ;
- 2 sont les fichiers lents ;
- 1 est refusé par hayro brut comme réécrit et rendu par PDFium :
  `pdfjs/issue15590.pdf`, 191 octets, dont la racine de l'arbre des pages n'a
  pas de `/Count`.

Les 15 fichiers que PDFium seul ne rendait pas, redonnés à PDFium :

- 9 sont rendus une fois réécrits ;
- 3 gardent une première page hors de l'arbre ;
- 3 ne peuvent pas être réécrits, faute de catalogue lisible :
  `qpdf/bad-content.pdf`, `qpdf/fuzz-16214.pdf` et `qpdf/issue-99b.pdf`.
  hayro rend pourtant ces trois-là bruts, en retrouvant des pages sans passer
  par le catalogue.

Faire lire au moteur la réécriture du noyau ferme donc la plus grande part de
l'écart entre les deux lecteurs d'un même fichier, dans les deux sens, comme
l'annonçait l'ADR 0005. Avec hayro, il ne reste qu'un refus propre au moteur
et deux fichiers très lents.

## Performance

En release, sur les 156 pages comparées :

| | PDFium | hayro |
|---|---:|---:|
| Rendu, total | 3 791 ms | 6 180 ms |
| Encodage PNG, total | 211 ms | 220 ms |
| Rendu médian par page | 3,1 ms | 5,5 ms |
| Pages au-delà de 16 ms | 23 | 35 |
| Pages au-delà de 50 ms | 15 | 22 |
| Pages au-delà de 100 ms | 6 | 15 |
| Pages au-delà de 250 ms | 2 | 6 |
| Pages au-delà de 500 ms | 1 | 2 |
| Pages au-delà de 1 s | 1 | 1 |

Rapport des temps de rendu hayro / PDFium par page : médiane 1,74, quartiles
1,55 et 1,88, 90e centile 2,81, de 0,15 à 78.

Le bruit est du même ordre que celui que documente le banc :

- **PDFium** : d'une exécution à l'autre, par rapport à
  `timings/pdfium.toml`, 2,6 % en médiane, 8,9 % au 90e centile, 49 % au plus
  (une page de quelques millisecondes) ;
- **hayro** : d'un processus à l'autre, 2,6 % en médiane, 8,0 % au 90e
  centile, 29,7 % au plus (une page de 6 ms). D'une exécution à l'autre, 3,7 %
  en médiane et 9,6 % au 90e centile.

Les pages de plus de 50 ms dans l'une ou l'autre exécution y varient de 0,1 à
5 %, sauf `issue13520.pdf` : 855 ms dans la mesure, 690 à 700 ms dans deux
autres.

| Page | PDFium | hayro | hayro / PDFium | Où passe le temps chez hayro |
|---|---:|---:|---:|---|
| 21 `22060_A1_01_Plans.pdf` | 1 346 ms | 328 ms | 0,24 | 264 ms de décodage de 4 JPEG 2 480 × 2 630 avec masque, 87 ms de réduction |
| 56 `issue19517.pdf` | 416 ms | 1 013 ms | 2,4 | image JPEG 2000 de 12 608 × 16 806 : 410 ms de décodage, 620 ms de réduction et de prémultiplication |
| 48 `issue13520.pdf` | 153 ms | 855 ms | 5,6 | 634 ms de nuances évaluées pixel par pixel (13) |
| 67 `issue6931_reduced.pdf` | 12,5 ms | 391 ms | 31 | 371 ms de décodage d'une image 1 608 × 546 en espace ICC |
| 41 `function_based_shading_cmyk.pdf` | 175 ms | 369 ms | 2,1 | 333 ms de nuances de fonction |
| 64 `issue5044.pdf` | 3,4 ms | 265 ms | 78 | 231 ms de masques souples rendus à la taille de la page, avec fonction de transfert |
| 52 `issue18032.pdf` | 8,4 ms | 249 ms | 29 | 182 ms pour 27 masques souples, 48 ms de nuances |
| 28 `bug1795263.pdf` | 6,5 ms | 195 ms | 30 | 105 ms de motifs répétés, 76 ms d'images |
| 29 `bug1815476.pdf` | 66 ms | 186 ms | 2,8 | 124 ms de dessin et 51 ms de décodage pour 35 images |
| 111 `pclm-in.pdf` | 147 ms | 168 ms | 1,1 | 415 bandes d'image : 118 ms de dessin, 27 ms de décodage |
| 35 `coons-allflags-withfunction.pdf` | 33 ms | 127 ms | 3,9 | 127 ms de nuance de Coons |
| 78 `mesh_shading_empty.pdf` | 2,4 ms | 55 ms | 23 | 51 ms de nuance à maillage |
| 84 `tracemonkey_with_annotations.pdf` | 8,0 ms | 22 ms | 2,8 | 16 ms pour remplir 4 293 glyphes |

**Méthode du profil.** Le `render` de hayro 0.7.1 et son périphérique
`Renderer` ont été copiés hors du dépôt, avec un chronomètre au début de
chaque sorte de travail. Chaque catégorie compte son temps propre, sans les catégories
imbriquées, en médiane des répétitions chaudes. Sur les 21 pages profilées, la
copie donne les mêmes pixels que `hayro::render`. L'interprétation du contenu
n'y dépasse jamais 3,4 ms ; le temps part dans les images, les nuances, les
masques souples et la composition des calques.

Hors du jeu, deux fichiers du corpus sont bien plus lents chez hayro que
tout ce tableau. Mesurés seuls, première page de `pdfjs/bug1978317.pdf` en
55 s (PDFium 0,53 s) et de `pdfjs/issue16263.pdf` en 80 s (PDFium 2,2 s). Ils
n'ont pas été profilés.

**Le budget de l'ADR 0004.** L'ADR veut une page « assez rapide pour
redessiner à chaque réglage modifié », sans chiffre. La page la plus lourde de
PDFium, le plan A1, passe de 1 346 ms à 328 ms, et hayro ne la dessine pas
plus lentement la première fois (337 ms). La plus lourde de hayro dans le jeu,
`issue19517.pdf`, prend 1,0 s, contre 0,42 s avec PDFium. Au seuil de 100 ms,
hayro dépasse sur 15 pages, PDFium sur 6. Le budget tient mieux sur la page
qui le mettait en échec, moins bien sur les masques souples, les nuances et
les images ICC. Il ne tient pas du tout sur les deux fichiers lents du
corpus.

## Texte : position des glyphes

**Oui, exploitable.** `hayro-interpret` interprète une page vers un
périphérique abstrait (`Device`). Sa méthode `draw_glyph` reçoit, pour chaque
glyphe montré :

- le glyphe, la matrice courante et celle du glyphe ;
- la couleur et le mode : remplissage, contour ou invisible.

Le glyphe (`Glyph`, `OutlineGlyph`) donne :

- son Unicode, par `as_unicode` : `ToUnicode`, puis nom du glyphe (liste
  d'Adobe), puis `uniXXXX` ; seulement `ToUnicode` pour les polices CID et
  Type 3 ;
- son contour (`outline`), d'où sa boîte d'encre, et son avance
  (`advance_width`) ;
- son identifiant dans la police.

`begin_marked_content` donne les étiquettes et les MCID du contenu balisé.
`interpret_page` avec son propre périphérique donne le texte sans dessiner :
10,5 ms pour les 4 293 glyphes de `tracemonkey_with_annotations.pdf`, page 1,
dont la page complète se dessine en 22 ms. Le texte invisible (mode 3,
couches d'OCR) est signalé avec le mode `Invisible`.

Vérifié sur dix pages en dessinant les boîtes sur l'image de hayro : elles
tombent sur les glyphes, pages tournées de 90° et 180° comprises. Limites, lues
dans le code puis constatées quand c'est indiqué :

- **mode de rendu 7 (texte en découpe seule)** : aucun appel, le texte est
  invisible pour l'extraction (constaté pages 60 et 83) ;
- **modes à remplissage et contour** : chaque glyphe est signalé deux fois
  (constaté page 72) ;
- **contenu optionnel masqué** : pas signalé ;
- **Type 3** : Unicode par `ToUnicode` seulement, et pas d'avance ;
- **police remplacée par défaut** : seuls les octets ASCII sont montrés ;
- **mise en page** : ni taille de police, ni ascendante, ni descendante, et
  aucun regroupement en mots, lignes ou ordre de lecture ; tout cela reste à
  faire ;
- **statut** : la documentation de la crate dit l'extraction Unicode
  expérimentale.

Une page qui n'est qu'une image numérisée n'a pas de glyphe (page 69).

## WebAssembly

**Oui, hayro compile pour wasm32-wasip1.** Un programme d'essai lit un PDF sur
l'entrée standard, appelle `hayro::render` sur sa première page à 1 400 pixels
et écrit le PNG sur la sortie standard. Compilé en release, il donne un module
de 5,0 Mio qui n'importe que six fonctions WASI : `fd_read`, `fd_write`,
`environ_get`, `environ_sizes_get`, `random_get` et `proc_exit`. Ce sont celles
du module de fusion (`docs/architecture.md`), toutes fournies par `fyp-host`.
Son exécution n'a pas été mesurée.

## Sûreté et licences

- **`unsafe` dans hayro.** Sept des huit crates de hayro interdisent `unsafe`
  (`#![forbid(unsafe_code)]`). `hayro-syntax` ne le fait pas, et
  `hayro-interpret` la compile avec sa fonctionnalité `unsafe`, que nous ne
  pouvons pas retirer. Elle apporte `flate2`, `memchr` et le SIMD des
  décodeurs JPEG, JBIG2 et JPEG 2000.
- **Ce que hayro ajoute au moteur.** 46 crates, dont 20 contiennent du code
  `unsafe`, soit 5 335 occurrences comptées dans les sources hors tests.
  `fearless_simd` (3 408) et `pic-scale` (1 348) en font 90 % : du SIMD. Aucune
  crate `-sys`, aucun code C ou C++. À titre de comparaison, `pdfium-render`
  compte 17 181 occurrences de `unsafe`, devant PDFium en C++.
- **Licences.** Apache-2.0 ou MIT pour hayro. Les données embarquées : les
  polices de substitution Foxit, sous BSD-3-Clause (copyright « PDFium
  Authors ») ; les CMap d'Adobe, sous BSD-3-Clause ; un profil ICC, sous CC0.
  `cargo deny check` voit les licences des crates, pas celles de ces
  fichiers.
- **Maturité.** Versions 0.x, un seul auteur déclaré dans les manifestes, et
  une documentation en retard sur le code : elle se dit sans chiffrement, sans
  fusion ni élimination et sans clé de couleur, qui fonctionnent au moins en
  partie.

## Ce qui n'a pas été mesuré

- **L'exécution sous WASI**, et l'identité des pixels entre le code natif et
  WebAssembly : les chemins SIMD diffèrent.
- **Linux et macOS** : polices système, SIMD, déterminisme.
- **La mémoire consommée**, par exemple pour l'image de 212 millions de pixels.
- **Le rendu multifil** de vello_cpu.
- **D'autres largeurs** que 1 400 pixels : vignettes, zoom jusqu'à 4 096
  pixels.
- **La couleur juste** des pages 41 et 75 : aucun troisième moteur n'est
  installé sur la machine.
- **La cause exacte des écarts de la page 52.**
- **Le profil des deux fichiers lents du corpus.**
- **L'intégration dans l'application** : latence ressentie, annulation,
  mémoire avec deux lecteurs du même fichier.
- **Les pages au-delà de la huitième** d'un fichier dans le jeu, et de la
  première dans le balayage du corpus.
- **La justesse de l'Unicode extrait** au-delà des dix pages regardées.
- **Le partage du temps à l'intérieur de vello_cpu** : génération des bandes
  ou remplissage fin.
- **La réactivité de l'amont** et sa feuille de route.

## Reproduire

Les mesures du banc :

```
cargo run --release -p fyp-render-bench -- run --a pdfium --b hayro
cargo run --release -p fyp-render-bench -- run --a hayro --b hayro --out <dossier>
```

La première écrit dans `target/render-bench/` et met `latest.html` à jour ;
la seconde, qui vérifie le déterminisme et mesure le bruit de hayro, écrit
dans le dossier donné.

Les autres mesures sont faites par des programmes jetables, hors du dépôt,
comme celle de « La métrique » dans `docs/banc-rendu.md` :

- le profil, sur une copie instrumentée du rendu de hayro 0.7.1 ;
- le périphérique qui relève les glyphes ;
- le déchiffrement par hayro lui-même ;
- l'essai WebAssembly ;
- le balayage du corpus et la reprise par la réécriture du noyau, qui lancent
  les programmes des moteurs du banc selon leur protocole ;
- la recherche des zones de fortes différences.
