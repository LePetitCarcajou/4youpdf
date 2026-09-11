# ADR 0001 — Licence AGPL-3.0 et contributions sous DCO

**Statut** : accepté — 2026-09

## Contexte
4YouPDF veut être « le VLC du PDF » : libre, gratuit, et hors de tout contrôle
d'un organisme lucratif, durablement. Des projets comparables (Stirling-PDF)
sont passés en open-core après avoir collecté des CLA.

## Décision
- Licence : **AGPL-3.0-or-later** pour tout le dépôt.
- Contributions : **DCO** (`Signed-off-by`), **aucun CLA**.
- Dépendances : licences permissives ou compatibles AGPL uniquement,
  vérifiées par `cargo deny` (`deny.toml`).
- Hébergement : GitHub + miroir Codeberg.

## Conséquences
- Toute redistribution modifiée, y compris en service réseau, doit publier
  ses sources.
- Relicencier exigerait l'accord de chaque contributeur : pratiquement
  impossible, ce qui est le but.
- Les dépendances GPL-incompatibles (propriétaires, certaines licences
  « source-available ») sont exclues de fait.
