# Architecture

## Vue d'ensemble

```
┌──────────────────────────────────────────────────────────┐
│  app/ (Tauri)          fyp-cli                            │  interfaces
├──────────────────────────────────────────────────────────┤
│  fyp-host   — découverte, sandbox WASM, permissions       │  hôte
├──────────────────────────────────────────────────────────┤
│  fyp-plugin-api — manifeste, traits, types d'échange      │  CONTRAT (versionné à part)
├──────────────────────────────────────────────────────────┤
│  fyp-conformance   fyp-crypto                             │  services
├──────────────────────────────────────────────────────────┤
│  fyp-core — lexer, objets, xref, filtres, writer          │  noyau
└──────────────────────────────────────────────────────────┘
        plugins/* ──dépendent uniquement de──▶ fyp-plugin-api
```

Les flèches de dépendance vont toujours vers le bas. `fyp-core` ne connaît ni
les plugins, ni l'hôte, ni l'interface.

## Couches du noyau (`fyp-core`)

| Couche | Module | Norme | État |
|---|---|---|---|
| 1. Lexique | `lexer` | ISO 32000-2, 7.2 | fait, testé |
| 2. Objets | `object`, `parser` | 7.3 | fait, testé |
| 3. Fichier | `version`, `xref` | 7.5 | fait, testé (table classique, flux xref, chaîne `/Prev`, `/XRefStm` des fichiers hybrides) |
| 4. Filtres | `filters` | 7.4 | fait, testé (Flate + prédicteurs TIFF/PNG, ASCIIHex, ASCII85, RunLength) ; LZW, filtres image et `/Crypt` nommés à venir |
| 5. Chiffrement | `fyp-crypto` | 7.6 | types |
| 6. Document | `document`, `recover` | 7.7 | fait, testé (objets via la xref et les object streams, catalogue, nombre de pages, xref reconstruite par scan) |
| 7. Écriture | `writer` | 7.5.5, 7.5.8 | fait, testé (table classique ou flux xref, round-trip sur toutes les fixtures, `fyp rewrite`) |

Principe de tolérance : la lecture accepte ce que les lecteurs majeurs
acceptent (xref reconstruite par scan, `/Length` faux, `endobj` manquant,
en-tête décalé, données Flate tronquées ou sans en-tête zlib). L'écriture est
stricte et produit toujours un fichier conforme.

### Filtres et limites

`filters::decode_stream(dict, data, resolve)` applique la chaîne `/Filter`
(nom ou tableau) avec les `/DecodeParms` correspondants. Toute sortie est
plafonnée par `DecodeLimits::max_output` (256 Mio par défaut) : un flux qui
se décompresse au-delà donne `Error::LimitExceeded`, jamais une allocation
démesurée. `Document::open_with_limits` propage ce plafond aux flux xref,
aux object streams et à `Document::decoded`.

Les décodeurs ne dimensionnent rien d'après une valeur lue dans le fichier
(`/N`, `/Size`, `/Index`, `/W`, `/Columns`) : les boucles s'arrêtent à la
fin des données réellement présentes, et les largeurs absurdes sont refusées
(`Error::BadXref`).

### Object streams

Un objet de type 2 dans la xref (`XrefEntry::InStream { stream_num, index }`)
est lu dans son object stream (7.5.7). Le flux est décodé une seule fois par
`Document` et gardé en cache ; seul le parsing de l'objet demandé est
refait. Deux règles de la norme sont vérifiées et donnent
`Error::BadObjectStream` : un object stream ne contient pas de stream, et
n'est pas lui-même dans un object stream. Si `index` ne désigne pas le bon
numéro d'objet, la liste `numéro offset` de l'en-tête fait foi.

Fichiers hybrides (7.5.8.4) : les entrées de la table classique priment,
puis celles du flux `/XRefStm`, puis `/Prev`. Une entrée libre de la table
n'occulte pas le flux `/XRefStm` de la même section, car c'est ainsi que les
objets compressés sont cachés aux lecteurs antérieurs à PDF 1.5.

### Reconstruction de la xref par scan

`Document::open` lit d'abord la table déclarée par `startxref`, puis la
vérifie : chaque entrée `n` doit avoir son en-tête `N G obj` à l'offset
annoncé (trois jetons, pas de parsing complet), chaque objet compressé doit
désigner un object stream stocké dans le fichier, et le trailer doit avoir
un `/Root`. Si la lecture ou la vérification échoue (`startxref` absent,
offset faux, table malformée, boucle `/Prev`), `recover::reconstruct`
reconstruit l'index en scannant le fichier. C'est un recours, jamais le
chemin normal.

Règles du scan :

- Il cherche les mots-clés `obj` et `trailer` du début à la fin. Un `obj`
  est un en-tête s'il est précédé de `N G` et suivi d'une frontière de
  jeton ; les lignes de commentaire `%` sont ignorées.
- Un candidat n'est retenu que si l'objet se parse. Le parsing se fait sur
  une tranche coupée à son `endobj` (ou au prochain en-tête si `endobj`
  manque), puis une seconde fois jusqu'au vrai `endobj` si la première
  coupe échoue. Cette seconde passe gère `N G obj` dans une chaîne ou dans
  les données d'un stream : le scan reprend après l'objet accepté, donc ces
  faux en-têtes ne sont jamais examinés.
- Plusieurs définitions d'un même numéro : la dernière du fichier gagne.
- Les object streams trouvés sont ouverts et leur contenu indexé ; ils
  comptent à la position du stream qui les contient pour la règle
  précédente. Un object stream indécodable est ignoré.
- Le trailer est la fusion, dans l'ordre du fichier, des dictionnaires
  `trailer` et des dictionnaires de flux xref (`/Root`, `/Info`, `/ID`,
  `/Encrypt`), sans `/Prev` ni `/XRefStm`, `/Size` recalculé. Si `/Root` ne
  désigne aucun objet trouvé, le dernier objet `/Type /Catalog` (au premier
  niveau ou dans un object stream) le remplace.
- Le travail est linéaire : les positions des prochains mots-clés sont
  mises en cache, et un budget global d'octets parsés (huit fois la taille
  du fichier) arrête le scan sur les fichiers construits pour rendre chaque
  candidat coûteux. Un fichier sans aucun objet valide donne
  `Error::Unrecoverable`, qui porte l'erreur de la table déclarée.

Un fichier réparé ne se fait jamais passer pour sain :
`Document::reconstructed()` renvoie la cause, `Xref::kind()` vaut
`SectionKind::Reconstructed`, et `fyp info` affiche « xref reconstruite par
scan ».

### Écriture

`writer::Writer::new(version).xref_style(style).write(&document)` sérialise
un document ouvert, sain ou réparé, en un fichier conforme à une seule
section : en-tête `%PDF-x.y` suivi du commentaire binaire (quatre octets
≥ 128), tous les objets indirects, la section xref, le trailer, `startxref`
et `%%EOF`. La lecture est tolérante, l'écriture est stricte :

- Chaque objet lisible est écrit au premier niveau sous son numéro et sa
  génération d'origine. Les objets compressés sortent de leur object stream
  (génération 0). Les object streams et les flux xref de la source ne sont
  pas recopiés : ils décrivent la disposition de l'ancien fichier, pas le
  document. Un objet illisible est omis ; une référence vers lui vaut
  `null`, comme dans la source.
- Chaînes littérales échappées (`\(`, `\)`, `\\`, `\n`, `\r`, `\t`), ou
  hexadécimales dès qu'un octet n'est pas de l'ASCII imprimable. Noms avec
  `#xx` pour tout octet hors `!`..`~`, les délimiteurs et `#`. Réels sans
  notation scientifique (`Display` de `f64` n'en produit jamais), avec
  `.0` pour garder réel un réel entier ; un réel infini ou NaN est
  `Error::Unwritable`. Dictionnaires triés par clé (`BTreeMap`), donc
  sortie déterministe.
- Streams recopiés tels quels, données encodées et `/Filter` conservés,
  `/Length` remplacé par la longueur exacte, directe.
- Trailer réduit aux clés qui décrivent le document (table 15) : `/Root`,
  `/Info` s'il mène à un objet écrit, `/ID`, plus `/Size`. Ni `/Prev` ni
  `/XRefStm`. `/ID` : la première chaîne est conservée si la source en a
  une, sinon dérivée du contenu ; la seconde est toujours recalculée
  d'après le contenu (14.4), donc réécrire un fichier déjà réécrit donne
  exactement les mêmes octets.
- `XrefStyle::Table` : une seule sous-section à partir de 0 (7.5.4), une
  entrée de 20 octets par numéro, liste des entrées libres chaînée
  (0 → premier trou → … → 0), générations des entrées libres de la source
  conservées. Les numéros trop épars (plus de `MAX_TABLE_PADDING`
  entrées de remplissage, 1 Mi) sont refusés : c'est la protection contre
  un numéro d'objet hostile qui ferait grossir la sortie sans limite.
- `XrefStyle::Stream` : flux xref Flate, `/W [1 n 2]`, `/Index` par plages
  de numéros contigus (pas de remplissage), qui se liste lui-même sous le
  numéro suivant le plus grand numéro écrit. La version d'en-tête est
  portée à 1.5 au minimum.
- Refus explicites : fichier chiffré (`Error::Unsupported`, les objets
  sortis d'un object stream seraient en clair, 7.6), `/Root` qui ne mène à
  aucun objet écrit, offset au-delà des dix chiffres d'une entrée de table.

`fyp rewrite entrée.pdf sortie.pdf [--xref-stream]` applique le writer avec
la version de l'entrée, puis relit le fichier produit : la commande échoue
si cette relecture a besoin d'une reconstruction.

Test central (`crates/fyp-core/tests/roundtrip.rs`) : chaque fixture est
ouverte, écrite dans les deux styles, rouverte. Le second document n'a pas
été reconstruit, a le même nombre de pages, les mêmes numéros d'objets
lisibles (object streams et flux xref exclus des deux côtés), et chaque
objet est égal au modèle près (`/Length` mis à part pour les streams) ;
une seconde écriture reproduit les mêmes octets. Le même parcours
s'applique à `tests/corpus-private/` quand ce dossier local existe.

## Modules

Un module = un dossier avec `manifest.toml` + code. Voir `plugin-manifest.md`.
Deux runtimes :

- `wasm` : sandbox Wasmtime + WASI. Obligatoire pour tout module tiers.
- `native` : crate Rust compilé dans l'hôte. Réservé aux modules du dépôt,
  revus, pour les traitements lourds (OCR, rendu).

L'hôte re-parse et valide tout document renvoyé par un module.

## Feuille de route

- **0.1** — noyau syntaxique + xref + filtres + writer ; `fyp info`,
  `fyp rewrite` ; round-trip sur 100 % du corpus ; fuzzing sans crash.
- **0.2** — chargement WASM (Wasmtime), permissions, limites ; premier
  module réel (`merge`) ; `fyp merge`.
- **0.3** — application Tauri : ouvrir, organiser, pipeline, panneau de
  conformité PDF/A (validation veraPDF externe puis moteur interne).
- **0.4** — chiffrement R6, PDF 2.0 en écriture, PDF/X.
- **0.5** — OCR (module natif Tesseract), PAdES.
- **1.0** — API des modules gelée, catalogue signé, PDF/E, PDF/UA.
