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
| 3. Fichier | `version`, `xref` (à venir) | 7.5 | `version` fait |
| 4. Filtres | `filters` (à venir) | 7.4 | — |
| 5. Chiffrement | `fyp-crypto` | 7.6 | types |
| 6. Document | `document` (à venir) | 7.7 | — |
| 7. Écriture | `writer` (à venir) | 7.5.5, 7.5.8 | — |

Principe de tolérance : la lecture accepte ce que les lecteurs majeurs
acceptent (xref reconstruite par scan, `/Length` faux, `endobj` manquant,
en-tête décalé). L'écriture est stricte et produit toujours un fichier
conforme.

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
