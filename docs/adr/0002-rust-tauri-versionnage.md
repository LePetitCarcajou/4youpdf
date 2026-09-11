# ADR 0002 — Rust pour le noyau, Tauri pour l'interface, versionnage séparé de l'API des modules

**Statut** : accepté — 2026-09

## Contexte
Exigences : sécurité, performance, modularité, interface soignée, binaire
léger, et un contrat stable pour des modules écrits par des tiers.

## Décision
- Noyau, hôte et CLI en **Rust** (`#![forbid(unsafe_code)]`).
- Interface desktop en **Tauri 2** (frontend TypeScript).
- Workspace Cargo avec une version commune pour l'application et le noyau,
  et une **version indépendante pour `fyp-plugin-api`**.
- Compatibilité des modules : même mineur en `0.x`, même majeur ensuite.
  L'hôte refuse un module hors de cette fenêtre.
- Conventional Commits, changelog généré (`git-cliff`), SemVer.

## Alternatives écartées
- Python : trop lent pour un parseur/rasteriseur, distribution lourde.
- Java (comme Stirling-PDF) : dépendance à un runtime, UI moins souple.
- Electron : ~150 Mo de Chromium embarqué, empreinte mémoire.

## Conséquences
- Barrière d'entrée plus haute pour les contributeurs (Rust), compensée par
  des modules possibles dans tout langage compilant vers WebAssembly.
- L'API des modules devient un artefact publié et documenté à part entière.
