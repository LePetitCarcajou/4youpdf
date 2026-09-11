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
| 6. Document | `document` | 7.7 | fait, testé (objets via la xref et les object streams, catalogue, nombre de pages) |
| 7. Écriture | `writer` (à venir) | 7.5.5, 7.5.8 | — |

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
