# Provenance du corpus

Généré par `tools/fetch_corpus.py` ; ne pas modifier à la main. Les lots
téléchargés vivent dans des sous-dossiers de `tests/corpus/` ignorés par
Git : chaque poste les récupère avec le script. Seuls les fichiers `*.pdf`
sont conservés, avec les fichiers de licence du dépôt d'origine dans
`<lot>/_upstream/`.

| Lot | Dépôt | Sous-dossier | Commit | Date du commit | Licence | Fichiers | Téléchargé le |
|---|---|---|---|---|---|---|---|
| `pdfjs/` | [mozilla/pdf.js](https://github.com/mozilla/pdf.js) | `test/pdfs` | `fd453c2ce3e1` (master) | 2026-09-11 | Apache-2.0 | 982 | 2026-09-12 |
| `qpdf/` | [qpdf/qpdf](https://github.com/qpdf/qpdf) | `qpdf/qtest/qpdf` | `54d6053af283` (main) | 2026-09-06 | Apache-2.0 | 639 | 2026-09-12 |
| `verapdf/` | [veraPDF/veraPDF-corpus](https://github.com/veraPDF/veraPDF-corpus) | `/` | `01e40281d48e` (staging) | 2026-08-28 | CC-BY-4.0 | 2908 | 2026-09-12 |

## Description des lots

- **pdfjs** — Suite de test du lecteur pdf.js : plusieurs centaines de fichiers réels et pathologiques. Les fichiers `.link` (documents externes que pdf.js ne redistribue pas) sont ignorés.
- **qpdf** — Suite de test de qpdf : xref streams, object streams, fichiers chiffrés, mises à jour incrémentales, fichiers volontairement cassés.
- **verapdf** — Corpus veraPDF : PDF/A (1b à 4f) et PDF/UA valides et invalides, un fichier par règle, plus les fichiers de la suite Isartor et des cas ISO 32000-1/2. Licence indiquée dans le README du dépôt.

## Suites à importer à la main

- **Ghent Output Suite** — PDF/X et impression. Distribuée par le Ghent Workgroup (gwg.org) sous ses propres conditions : à télécharger à la main et à placer dans `tests/corpus/ghent/` ; ce dossier est ignoré par Git comme les autres lots.

## Licences

Les fichiers sont utilisés comme données de test, sans modification et sans
redistribution par ce dépôt. Apache-2.0 et CC BY 4.0 l'autorisent avec
mention de la provenance : c'est le rôle de ce fichier et des copies de
licence placées dans chaque lot.
