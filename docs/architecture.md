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
│  fyp-conformance                                          │  services
├──────────────────────────────────────────────────────────┤
│  fyp-core — lexer, objets, xref, filtres, writer          │  noyau
├──────────────────────────────────────────────────────────┤
│  fyp-crypto — handler de sécurité standard (7.6)          │  primitives
└──────────────────────────────────────────────────────────┘
        plugins/* ──dépendent du contrat──▶ fyp-plugin-api
                  ··peuvent embarquer, dans leur binaire WASM··▶ fyp-core
```

Les flèches de dépendance vont toujours vers le bas. `fyp-core` ne connaît ni
les plugins, ni l'hôte, ni l'interface. Il s'appuie sur `fyp-crypto`, crate
feuille qui ne dépend d'aucune autre crate du projet : elle reçoit les valeurs
du dictionnaire `/Encrypt` et des octets, jamais des objets PDF.

Un module ne dépend jamais de l'hôte ni d'une interface. Il parle à l'hôte
par le seul contrat, et peut embarquer `fyp-core` comme bibliothèque : ce
code est compilé dans son binaire WebAssembly et s'exécute dans la sandbox,
sans rien lui accorder de plus (c'est le cas du module de fusion). Le noyau
compile donc aussi pour `wasm32-wasip1`, vérifié par la CI.

Quatre diagrammes Mermaid complètent ce document dans `diagrams.md` : les
couches et leurs dépendances, le parcours d'un fichier à l'ouverture, le
modèle objet face à la structure logique d'un document, et le cycle de vie
d'un module.

## Couches du noyau (`fyp-core`)

| Couche | Module | Norme | État |
|---|---|---|---|
| 1. Lexique | `lexer` | ISO 32000-2, 7.2 | fait, testé |
| 2. Objets | `object`, `parser` | 7.3 | fait, testé |
| 3. Fichier | `version`, `xref` | 7.5 | fait, testé (table classique, flux xref, chaîne `/Prev`, `/XRefStm` des fichiers hybrides) |
| 4. Filtres | `filters` | 7.4 | fait, testé (Flate + prédicteurs TIFF/PNG, ASCIIHex, ASCII85, RunLength, `/Crypt`) ; LZW et filtres image à venir |
| 5. Chiffrement | `encryption` + `fyp-crypto` | 7.6 | fait, testé (révisions 2 à 6 : RC4 40 à 128 bits, AES-128, AES-256 ; mot de passe utilisateur ou propriétaire ; crypt filters `/StmF`, `/StrF`, `/Identity` et nommés ; `/EncryptMetadata`) ; chiffrement à l'écriture à venir |
| 6. Document | `document`, `recover` | 7.7 | fait, testé (objets via la xref et les object streams, catalogue, nombre de pages, xref reconstruite par scan) |
| 7. Écriture | `writer` | 7.5.5, 7.5.8 | fait, testé (table classique ou flux xref, round-trip sur toutes les fixtures, `fyp rewrite`) |
| 8. Opérations de pages | `ops` | 7.7.3, 12.3 | fait, testé (fusion, extraction, découpage, rotation, suppression ; `fyp merge`, `fyp pages`, `fyp split`) |

Principe de tolérance : la lecture accepte ce que les lecteurs majeurs
acceptent (xref reconstruite par scan, `/Length` faux, `endobj` manquant,
en-tête décalé, données Flate tronquées ou sans en-tête zlib). L'écriture est
stricte et produit toujours un fichier conforme.

Tolérances issues du rapport corpus, chacune avec sa fixture dans
`tests/fixtures/` : génération `65536` sur l'entrée libre de tête (bornée à
65535 au lieu de rejeter la table) ; entrée « en usage » à l'offset 0, dans
une table ou un flux xref, lue comme libre ; en-tête `%PDF-1.` sans chiffre
mineur (version 1.0) ; fichier sans `%PDF` mais commençant par un
commentaire et contenant des objets, tenté avec la version supposée 1.4,
`QuickInfo::header_present` valant alors `false`.

Seconde série, issue du nettoyage du rapport (septembre 2026), chacune avec
sa fixture et son test :

- `no-header-junk.pdf` — aucun `%PDF` et une première ligne quelconque :
  un fichier sans en-tête est tenté dès qu'il contient un en-tête d'objet
  `N G obj` dans son premier kilo-octet, comme le font Poppler et qpdf
  (« may not be a PDF file, continuing anyway »).
- `object-zero.pdf` — un objet `0 0 obj` listé « en usage » dans la table :
  l'objet 0 est la tête de la liste libre (7.5.4), jamais un objet ; l'entrée
  est lue comme libre, l'objet ignoré, y compris par le scan (qpdf
  `obj0.pdf`, `issue-99.pdf` dont le `/Root 0 0 R` cède la place au
  catalogue trouvé).
- `root-dangling.pdf` — table saine dont le `/Root` ne mène à rien : la
  vérification de la table exige que `/Root` désigne une entrée existante
  dont l'objet se parse en dictionnaire (un parsing complet, pour ce seul
  objet) ; sinon la table est rejetée et le scan retrouve le catalogue par
  son `/Type`.
- `startxref-off.pdf` — `startxref` pointe à côté de la table : avant de
  scanner, `Document` cherche la section à l'offset décalé de la position de
  l'en-tête (déchets avant `%PDF`, qpdf `leading-junk.pdf`), puis le mot-clé
  `xref` ou un flux `/Type /XRef` le plus proche dans une fenêtre de
  512 octets. La section trouvée est vérifiée comme les autres ; un mauvais
  choix mène au scan, pas à une lecture fausse. `Document::relocated_startxref()`
  signale la correction, `fyp info` l'affiche, le rapport corpus la compte
  (13 fichiers).
- `root-direct.pdf` — catalogue écrit directement dans le trailer
  (`/Root << … >>`, pdf.js `issue9105_other.pdf`) : accepté à la lecture,
  conservé par le scan, promu en objet indirect par le writer.

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

`Document::open` lit d'abord la table déclarée par `startxref` (ou trouvée
à côté, voir `startxref-off.pdf` ci-dessus), puis la vérifie : chaque
entrée `n` doit avoir son en-tête `N G obj` à l'offset annoncé (trois
jetons, pas de parsing complet), chaque objet compressé doit désigner un
object stream stocké dans le fichier, et le `/Root` du trailer doit mener à
un dictionnaire lisible (ou être lui-même un dictionnaire). Si la lecture
ou la vérification échoue (`startxref` absent, offset faux, table
malformée, boucle `/Prev`, `/Root` pendant), `recover::reconstruct`
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
  exactement les mêmes octets. La dérivation est le MD5 du corps du
  fichier : les mêmes octets sur toutes les plateformes, wasm32 compris,
  si bien qu'un module qui embarque le noyau écrit ce qu'écrirait l'hôte.
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
- Refus explicites : `/Root` qui ne mène à aucun objet écrit, offset
  au-delà des dix chiffres d'une entrée de table.
- Catalogue direct dans le trailer de la source : écrit comme objet
  indirect sous le numéro suivant le plus grand numéro écrit, et référencé
  par le trailer (7.5.5 exige une référence).
- Entrée chiffrée : la sortie est **en clair**. `Document` fournit les
  chaînes et les flux déchiffrés, le dictionnaire `/Encrypt` n'est pas
  recopié et le trailer n'a pas d'entrée `/Encrypt`. Le chiffrement à
  l'écriture est un jalon ultérieur ; d'ici là, tout appelant qui réécrit
  un fichier chiffré doit le dire (`fyp rewrite` l'affiche avant d'écrire).

`fyp rewrite entrée.pdf sortie.pdf [--xref-stream] [--password …]` applique
le writer avec la version de l'entrée, puis relit le fichier produit : la
commande échoue si cette relecture a besoin d'une reconstruction.

Test central (`crates/fyp-core/tests/roundtrip.rs`) : chaque fixture est
ouverte, écrite dans les deux styles, rouverte. Le second document n'a pas
été reconstruit, a le même nombre de pages, les mêmes numéros d'objets
lisibles (object streams et flux xref exclus des deux côtés), et chaque
objet est égal au modèle près (`/Length` mis à part pour les streams) ;
une seconde écriture reproduit les mêmes octets. Le même parcours
s'applique à `tests/corpus-private/` quand ce dossier local existe.

### Chiffrement

`Document::open` lit le dictionnaire `/Encrypt` du trailer (handler de
sécurité standard, ISO 32000-2, 7.6) et essaie le mot de passe vide, en tant
que mot de passe utilisateur puis propriétaire : c'est le cas de la grande
majorité des fichiers chiffrés, qui restreignent l'usage sans interdire la
lecture. `Document::open_with_password` prend un autre mot de passe ;
`Document::open_with` combine limites et mot de passe. Ensuite tout est
transparent : les chaînes et les flux rendus par `get` sont en clair, y
compris les object streams (déchiffrés en bloc avant d'en extraire les
objets). `Document::encryption()` dit si le fichier était chiffré et
comment (`Encryption` : révision, longueur de clé, chiffre des flux et des
chaînes, `/EncryptMetadata`, mot de passe propriétaire ou non) ; `fyp info`
l'affiche.

Le module `encryption` de `fyp-core` traduit le dictionnaire en
`fyp_crypto::Params` (tables 20, 21 et 25) : `/V`, `/R`, `/Length`, `/O`,
`/U`, `/OE`, `/UE`, `/P`, `/EncryptMetadata`, et pour les révisions 4 et
plus les crypt filters de `/CF` choisis par `/StmF` et `/StrF`
(`/Identity` par défaut). `fyp-crypto` fait le reste : dérivation de la
clé (algorithmes 2 à 7 pour RC4 et AES-128, 2.A et 2.B pour AES-256),
validation du mot de passe, clé par objet (algorithme 1, numéro et
génération), RC4, AES-CBC avec vecteur d'initialisation en tête et
bourrage PKCS#5. Ses primitives viennent des crates RustCrypto auditées
(`aes`, `cbc`, `md-5`, `rc4`, `sha2`), acceptées par `cargo deny`.

Ce qui reste en clair, conformément à 7.6.3 : le dictionnaire `/Encrypt`
lui-même, l'`/ID` du trailer, les flux xref, et le flux de métadonnées
quand `/EncryptMetadata` vaut `false`. Un flux qui nomme son propre crypt
filter (`/Filter /Crypt`, `/DecodeParms /Name`, 7.4.10) est déchiffré avec
ce filtre à la lecture de l'objet, et le filtre est retiré du dictionnaire :
le modèle objet vu par l'appelant, et donc par le writer, est celui d'un
fichier non chiffré.

Tolérances : `/Length` d'un crypt filter inférieur à 40 lu en octets
(Acrobat écrit `/Length 16`), AES-128 imposant 128 bits quoi que dise
`/Length`, révision 2 toujours à 40 bits, `/P` non signé accepté, `/O`,
`/U`, `/OE` et `/UE` plus longs que prévu tronqués et plus courts complétés
par des zéros comme le fait qpdf (`short-O-U.pdf` du corpus : seuls les 16
premiers octets de `/U` comptent en révision 3 et 4), données AES sans vecteur
d'initialisation ou avec un bloc incomplet déchiffrées au mieux, bourrage
invalide conservé, crypt filter nommé mais absent de `/CF` traité comme le
chiffre des flux, `/Encrypt` qui ne mène à aucun objet ignoré.

Erreurs, jamais de panique : `Error::BadEncryption` pour un dictionnaire
inutilisable (révision inconnue, `/O`, `/U` ou `/P` absents ou vides, `/Length` qui
n'est pas une taille de clé, `/StmF` sans définition dans `/CF`, `/CFM`
inconnu), `Error::WrongPassword` quand ni le mot de passe utilisateur ni le
propriétaire ne correspond, `Error::Unsupported` pour un handler autre que
`/Standard` (clé publique, 7.6.5). Limites connues : SASLprep n'est pas
appliqué aux mots de passe des révisions 5 et 6 ; la reconstruction par
scan n'ouvre pas les object streams d'un fichier chiffré (leur contenu
n'est indexé qu'après, quand ils sont lus par `Document`).

### Opérations de pages (`ops`)

`ops::merge`, `extract_pages`, `delete_pages`, `rotate` et `split`
construisent toutes un document neuf à partir des pages conservées, puis le
confient au writer (`Writer::write_objects`, qui sérialise un ensemble
d'objets fourni par l'appelant avec son trailer). Indices de pages 0-based
dans l'API, 1-based dans la CLI (`1,3,5-8`, `8-5` pour l'ordre inverse),
avec une erreur nette hors limites.

- **Liste des pages** (`ops::pages`) : parcours de l'arbre 7.7.3 par pile
  explicite (un arbre profond ou dégénéré ne fait pas déborder la nôtre),
  ensemble des nœuds visités contre les boucles (qpdf `pages-loop.pdf`),
  nœud sans `/Type` pris pour une page sauf s'il a `/Kids`, page écrite
  directement dans `/Kids` acceptée. Les attributs héritables (table 30 :
  `/Resources`, `/MediaBox`, `/CropBox`, `/Rotate`) sont résolus dans
  chaque page, qui devient indépendante de son arbre. Une page listée deux
  fois dans l'arbre n'est prise qu'une fois.
- **Copie du graphe** : les pages conservées reçoivent leurs numéros
  d'abord (pour que les références entre pages conservées se résolvent
  quel que soit l'ordre), puis chaque objet atteignable est copié à la
  demande sous un nouveau numéro, par document source. Rien d'autre n'est
  embarqué : un balayage final ne garde que ce qui est atteignable depuis
  le trailer. Les nœuds de l'ancien arbre ne sont jamais copiés ; l'arbre
  de sortie est plat (un seul `/Pages`).
- **Références vers une page supprimée** : elles deviennent `null`, puis
  un nettoyage retire ce qui les portait, décidé sur un instantané pour
  ne pas dépendre de l'ordre de parcours : `/Dest` et actions `/GoTo` dont
  le tableau de destination commence par `null`, entrées des arbres de
  noms et du `/Dests` de PDF 1.1 vers ces pages, annotations de lien sans
  plus de destination ni d'action, entrées `null` de `/Annots`. Un signet
  vers une page supprimée garde son titre et perd sa destination.
- **Fusion** : catalogue, `/Info` et `/ID` du premier document ; version
  d'en-tête la plus haute des entrées ; signets enchaînés sous une racine
  unique (le dernier élément de niveau 1 de chaque document pointe sur le
  premier du suivant, `/Count` additionné) ; destinations nommées de tous
  les documents (`/Dests` 1.1 et arbre `/Names /Dests`) aplaties dans un
  seul nœud feuille trié, le premier document gagnant sur un nom en
  double ; champs de formulaire réunis dans un seul `/AcroForm` (celui du
  premier document qui en a un, `/Fields` concaténés).
- **Rotation** : `/Rotate` existant (hérité compris) plus l'angle, multiple
  de 90, ramené dans `0..360` ; `0` s'écrit par absence de clé.
- **Entrée chiffrée** : déchiffrée par `Document`, sortie en clair ; la CLI
  l'annonce avant d'écrire, comme `fyp rewrite`.

Pertes connues, par construction de cette première version : à la fusion,
seuls le `/PageLabels`, le `/StructTreeRoot`, le `/Metadata`, les
`/OCProperties` et les préférences d'affichage du premier document sont
conservés (pages, ressources, annotations, signets, destinations nommées et
champs des autres le sont). L'arbre des structures d'un document extrait
garde des entrées `/Pg null` pour les pages supprimées. Une page demandée
deux fois dans une extraction est refusée (`Error::BadOperation`) plutôt
que dupliquée.

Tests (`crates/fyp-core/tests/ops.rs`) : sur toutes les fixtures à arbre
de pages et sur une dizaine de fichiers du corpus (signets avec actions,
destinations `/Dests` 1.1, formulaires, arbre profond, arbre bouclé, page
dupliquée, object streams), chaque résultat s'ouvre sans reconstruction,
n'est pas chiffré, a le bon nombre de pages, chaque page conservée est
égale à son modèle source références suivies, aucune référence ne mène à
un objet absent et aucune destination à une page absente. Extraire toutes
les pages dans l'ordre donne un document équivalent et l'opération est
stable (la refaire sur le résultat rend les mêmes octets). Fusion de trois
fixtures de structures différentes (table classique, flux xref, object
streams) et des fichiers du corpus, avec vérification de la chaîne des
signets et du tri des destinations.

### Corpus public et rapport

`tools/fetch_corpus.py` télécharge les suites de test publiques (pdf.js,
qpdf, veraPDF) dans `tests/corpus/<lot>/`, dossiers ignorés par Git, et
consigne leur provenance dans `tests/corpus/SOURCES.md`. Le test
`crates/fyp-core/tests/corpus.rs` parcourt tout ce qui s'y trouve : chaque
fichier passe par ouverture, écriture dans les deux styles, relecture et
comparaison, sur un thread à part avec délai, si bien qu'une panique ou un
blocage du noyau devient une ligne du rapport au lieu d'arrêter le parcours.
Le rapport `target/corpus-report.md` groupe les problèmes par cause
normalisée (chiffres et chaînes citées effacés) et les classe : paniques et
délais, refus à l'ouverture, échecs de round-trip par étape, tables
reconstruites. C'est la liste de travail du jalon « round-trip sur 100 % du
corpus » ; le test ne devient bloquant qu'avec `FYP_CORPUS_STRICT=1`.

### État du corpus et limites assumées

Relevé du 12 septembre 2026, corpus public de 4529 fichiers (pdf.js 982,
qpdf 639, veraPDF 2908), build release : 4472 round-trips complets (4394
sans reconstruction, 78 après reconstruction), 44 refus à l'ouverture,
13 round-trips en échec, aucune panique ni délai. Chaque fichier restant a
été classé, Poppler (`pdftotext`) servant d'arbitre : il n'extrait rien
d'aucun d'eux sans mot de passe.

| Limite | Fichiers | Nature |
|---|---:|---|
| Chiffrés avec un mot de passe utilisateur non vide : `Error::WrongPassword` tant que l'appelant ne le fournit pas | 35 | Comportement voulu ; Poppler exige aussi le mot de passe. Avec le bon mot de passe (`fyp info --password`), les fichiers qpdf `enc-R2`, `enc-R3`, `enc-XI-R6` s'ouvrent. |
| Aucun objet lisible nulle part : `Error::BadHeader` (4 fichiers sans un seul `N G obj`), `Error::Unrecoverable` (5 fichiers : flux xref à `/W [0 0 0]`, fichiers fuzzés tronqués) ou `Error::BadEncryption` (1 fichier : `/Encrypt` à `/Length 160`, sans autre objet) | 10 | Fichiers réellement invalides (qpdf `bad1.pdf`, `issue-141b`, `issue-143`, `issue-147`, `issue-150`, `issue-263`, `issue-335a/b`, `bad-direct-root` ; pdf.js `bug1020226`). |
| Ouverts par scan mais sans aucun catalogue lisible (`/Root` absent ou pendant, aucun objet `/Type /Catalog` qui se parse) : `Error::Unwritable` à l'écriture | 12 | Fichiers fuzzés (qpdf `issue-99b`, `issue-100`, `issue-101`, `issue-146`, `issue-148`, `issue-141a`, `issue-1503`, `inspect`, `bad-content`, `fuzz-16214` ; pdf.js `REDHAT-1531897-0`, `poppler-742-0-fuzzed`). Le catalogue de `issue-99b` est l'objet 0, numéro que 7.5.4 réserve ; renuméroter un objet serait inventer un document. |
| Numérotation trop éparse pour une table classique (objet 2147483647) : `Error::Unwritable` en style table, écrit en style flux xref | 1 | pdf.js `bug1980958.pdf`. Limite de `MAX_TABLE_PADDING`, protection contre une sortie démesurée ; `fyp rewrite --xref-stream` fonctionne. |

Fichier le plus lent, `pdfjs/bug1978317.pdf` (1,5 Mio, 65 563 objets dans
deux object streams) : 2,0 s en release pour les huit passes du test
(ouverture, deux écritures de 13 Mio chacune, relectures, comparaisons,
secondes écritures), soit 2 à 5 µs par objet et par passe. Le temps est
linéaire ; les 11,5 s observées venaient d'un build debug (même rapport,
×6, sur le deuxième fichier le plus lent). La seule quadratique possible,
la recherche d'un objet par numéro dans un object stream dont l'index de la
table est faux, est désormais un `BTreeMap` construit au décodage
(`ObjectStream::by_number`).

## Application desktop (`app/`)

Tauri 2 côté Rust, interface en TypeScript et CSS sans framework, compilée
et testée par trois binaires autonomes (esbuild, tsgo, QuickJS-ng)
récupérés par `tools/fetch_ui_tools.py` : aucun Node.js requis. Voir
`app/README.md` pour construire et lancer.

Première étape du jalon 0.3 : une fenêtre qui ouvre un PDF, montre ses
pages en vignettes ou une par une en grand, permet de les réordonner et de
les supprimer, et enregistre le résultat. Répartition :

- **Côté Rust (`app/src/`)**, la seule partie qui touche au disque et au
  noyau. `session.rs` ouvre le fichier par `Document::open_with_password`,
  liste les pages par `ops::pages` (taille et rotation pour une vignette
  vide au bon format, avant l'image) et enregistre par
  `ops::extract_pages`, après relecture du résultat. `main.rs` expose
  sept commandes : ouvrir, fermer, état du rendu, rendre une page,
  enregistrer, et les deux sélecteurs de fichiers du système (appelés
  depuis Rust par le plugin `dialog`, pas depuis l'interface). Une
  ouverture qui échoue ne remplace pas le document en cours.
- **Rendu (`app/src/render.rs`)** : PDFium par `pdfium-render`, chargé à
  l'exécution, sur un thread dédié qui sert les demandes une par une.
  Dépendance temporaire et confinée à ce module (ADR 0005) : l'interface
  ne voit qu'un service « page N, largeur W → PNG » et son état ; sans la
  bibliothèque, tout fonctionne avec des vignettes vides. Pour une page à
  la taille de la fenêtre, le temps passe dans l'encodage PNG, pas dans
  PDFium : le filtre `Up` est plus de quatre fois plus rapide que le filtre
  adaptatif par défaut, pour des fichiers 12 à 14 % plus gros.
- **Interface (`app/ui/`)** : l'ordre des pages et l'historique
  annuler/refaire vivent dans l'interface (`history.ts`) ; le côté Rust ne
  connaît que le document ouvert. Les vignettes se chargent au fil du
  défilement (`thumbnails.ts`, `IntersectionObserver`, trois demandes à la
  fois, pages visibles d'abord) et sont mises en cache par page source, si
  bien que réordonner ne redessine rien. Le glisser-déposer des vignettes
  passe par les événements de pointeur, pas par le glisser-déposer HTML5 :
  Tauri l'intercepte pour le dépôt de fichiers natif, qui reste actif. La
  vue d'une page (`viewer.ts`) s'ouvre par-dessus la grille, qui reste en
  place : la page courante est rendue à la largeur qu'elle occupe à l'écran,
  puis ses deux voisines. Elle n'envoie qu'une demande à la fois et met les
  vignettes en pause, si bien que la page affichée ne passe jamais derrière
  une file d'attente du thread de rendu. Les bandeaux sont des données
  (`notices.ts`) : ceux d'un document ne sont remplacés qu'à l'ouverture
  réussie d'un autre fichier ; une tentative ratée, ou un fichier qui
  attend son mot de passe, laisse le document affiché et ses bandeaux en
  place. Cette logique, sans DOM, est testée dans `app/ui/tests/` par
  QuickJS-ng, que lance `tools/build_ui.py`.
- **ADR 0004 appliqué** : une seule fenêtre, aucune boîte modale (le mot
  de passe d'un fichier chiffré est demandé dans un bandeau, les erreurs et
  les avertissements aussi ; la vue d'une page est un état de la fenêtre,
  sous lequel ces bandeaux restent visibles), actions contextuelles sur les vignettes
  (bouton de suppression, menu du clic droit, clavier). Un document réparé
  ou chiffré est annoncé avec les mots de `fyp info`, et l'enregistrement
  d'un fichier chiffré est annoncé comme produisant un fichier en clair.

Ce qui manque encore et vient ensuite : la palette de commandes, le
panneau de conformité, les modules, la lecture par blocs et le budget
mémoire de l'ADR 0004 (un document ouvert est aujourd'hui entier en
mémoire, deux fois avec PDFium).

## Modules

Un module = un dossier avec `manifest.toml` + `module.wasm`. Voir
`plugin-manifest.md`. Deux runtimes :

- `wasm` : sandbox Wasmtime. Obligatoire pour tout module tiers.
- `native` : crate Rust compilé dans l'hôte. Réservé aux modules du dépôt,
  revus, pour les traitements lourds (OCR, rendu). Pas encore chargé.

### Chargement et exécution (`fyp-host`)

Détail et justification : ADR 0003, section « Mise en œuvre ».

- **Découverte** (`discover`) : chaque sous-dossier qui a un manifeste
  (1 Mio au plus), validé avant toute lecture de code ; les refus sont
  rendus, pas tus. Des modules qui déclarent le même identifiant sont tous
  refusés (`DuplicateId`). `trusted` est ce que l'appelant dit du dossier :
  rien n'est signé ni vérifié (ADR 0003, « Limites connues »).
- **Chargement** (`Host::load`) : `module.wasm` (64 Mio au plus) compilé par
  Wasmtime 48. Refus si ce n'est pas une commande WASI (`_start`, `memory`
  non partagée), si une importation n'est pas une fonction ou contredit la
  signature WASI de son nom, si le manifeste demande une permission que
  l'hôte ne fournit pas encore (toutes sauf `read_document` et
  `write_document`), déclare une limite au-dessus des plafonds de l'hôte
  (`HostLimits`, `LimitAboveCeiling`) ou porte un caractère de contrôle ou
  de mise en forme bidirectionnelle dans un champ texte. Chaque importation
  hors des quatorze fonctions WASI que l'hôte implémente (`wasi.rs`, aucune
  ne touche au système) est liée à un piège qui la nomme.
- **Exécution** (`LoadedModule::run`) : action, `min_inputs` et paramètres
  vérifiés ; attente d'une place parmi les `max_concurrent_runs` exécutions
  simultanées de l'hôte (le délai court à partir du démarrage) ; requête
  encodée sur l'entrée standard (`fyp_plugin_api::exchange`) ; store neuf
  avec les limites du manifeste ; `_start` sur un thread dédié, époque du
  moteur avancée toutes les 10 ms par un second thread. Les mémoires, tables
  et réponses de toutes les exécutions en cours sont prises sur un budget
  commun (`memory_budget_mib`, 4 Gio par défaut), ainsi que six fois la
  réponse pendant sa re-validation.
- **Arrêts**, tous des `HostError` : temps (`Timeout`), mémoire
  (`MemoryExceeded`), budget commun épuisé (`HostMemoryExhausted`), sortie
  (`OutputTooLarge`), capacité non accordée (`CapabilityDenied`), piège du
  module (`Trapped`, avec sa sortie d'erreur), code de sortie non nul
  (`Exited`), réponse illisible (`BadResponse`), erreur rendue par le module
  (`ModuleFailed`). Tout texte venu du module (message, sortie d'erreur,
  noms d'importation, messages de Wasmtime qui le citent) est borné et rendu
  inerte : caractères de contrôle et bidirectionnels remplacés par U+FFFD.
  Les documents d'entrée sont empruntés en lecture seule.
- **Re-validation** (`revalidate`) : `Document::open` (reconstruction si
  besoin), au moins une page, réécriture par le writer (table classique, ou
  flux xref si la numérotation est trop éparse), relecture sans
  réparation. Seule la réécriture est rendue (`RunOutput::document`) ; une
  reconstruction est signalée (`RunOutput::reconstructed`).

Le module de fusion (`plugins/merge`) est une crate à deux cibles : la
bibliothèque (`handle`, testée en natif) appelle `ops::merge`, le binaire
est la commande WASI (`fyp_plugin_api::module::serve`). Compilé pour
`wasm32-wasip1` (560 Kio en release), il n'importe que `fd_read`,
`fd_write`, `environ_get`, `environ_sizes_get`, `random_get` et
`proc_exit`. `python tools/build_modules.py` construit chaque module du
dépôt et le copie en `plugins/<nom>/module.wasm` (ignoré par Git).

`fyp run <action> entrées… -o sortie [--param nom=valeur] [--modules
dossier] [--module id] [--password …]` emprunte tout ce chemin : découverte,
choix du module qui déclare l'action, entrées chiffrées déchiffrées par
l'hôte (le module ne reçoit jamais le mot de passe), exécution,
re-validation, écriture et relecture du résultat.

Tests (`crates/fyp-host/tests/sandbox.rs`, modules hostiles écrits en WAT) :
boucle infinie simple, dans la section `start` et en appels à l'hôte,
arrêtées au délai ; croissance de la mémoire et mémoire initiale trop
grande ; sortie sans fin ; appels à `path_open`, `sock_accept`,
`clock_time_get`, `poll_oneoff` et à une importation `env` ; pointeurs hors
mémoire (`EFAULT` pour le module, pas de panique) ; aucun dossier
préouvert, environnement vide ; permissions non déclarées ou indisponibles ;
paramètres refusés avant le démarrage ; modules malformés refusés au
chargement ; documents corrompus rejetés à la re-validation, objet illisible
absent de la réécriture ; plafonds de l'hôte, budget mémoire partagé entre
exécutions et clones de `Host` (test unitaire de `sandbox.rs`, qui attend
sur le budget lui-même plutôt que sur un délai), réponse et re-validation comptées dans ce
budget, file d'attente des exécutions ; texte hostile (échappements de
terminal, U+202E, message d'1 Mio) borné et inerte ; imposteur d'un
identifiant du dépôt (limite connue, voir l'ADR), identifiants en double,
manifeste surdimensionné ; re-validation du document le plus imbriqué sur
une pile d'1 Mio. Les quatorze fonctions WASI sont confrontées à un modèle
de référence écrit d'après la spécification (`wasi/fuzzing.rs`) : scripts
aléatoires à graine fixe dans `cargo test`, cible `host_wasi` pour
cargo-fuzz (sous Windows : `cargo +nightly fuzz run host_wasi --sanitizer
address`, la cible ne se lie pas sans ASan). Avec `plugins/merge/module.wasm` : mêmes octets que
`ops::merge` sur trois jeux de fixtures (dont un fichier chiffré et un
hybride) et, quand le corpus est là, sur deux de ses fichiers ;
`crates/fyp-cli/tests/run.rs` vérifie la même chose par la ligne de
commande. Ces tests sautent sans `module.wasm`, sauf avec
`FYP_REQUIRE_MODULES=1` : c'est le cas du job `wasm` de la CI, qui construit
d'abord les modules.

## Feuille de route

- **0.1** — noyau syntaxique + xref + filtres + writer ; `fyp info`,
  `fyp rewrite` ; round-trip sur 100 % du corpus ; fuzzing sans crash.
- **0.2** — opérations de pages dans le noyau (`ops` : fusion, extraction,
  découpage, rotation, suppression ; `fyp merge`, `fyp pages`, `fyp split`),
  faites ; chargement WASM (Wasmtime 48, WASI fourni par l'hôte), permissions
  `read_document` et `write_document`, limites appliquées par le runtime,
  re-validation, module de fusion réel et `fyp run`, faits ; restent les
  permissions de dossier, de réseau et de sous-processus.
- **0.3** — application Tauri : ouvrir, organiser (fait : fenêtre,
  vignettes, vue d'une page, réordonner, supprimer, enregistrer), pipeline, panneau de
  conformité PDF/A (validation veraPDF externe puis moteur interne).
- **0.4** — chiffrement à l'écriture (révision 6), PDF 2.0 en écriture, PDF/X.
- **0.5** — OCR (module natif Tesseract), PAdES.
- **1.0** — API des modules gelée, catalogue signé, PDF/E, PDF/UA.
