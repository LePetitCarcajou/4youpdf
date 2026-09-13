# Corpus de test

Les PDF « de terrain » sur lesquels le round-trip du noyau doit finir par
passer à 100 %. Deux origines :

- **Suites publiques**, téléchargées localement par `tools/fetch_corpus.py`
  dans un sous-dossier par lot (`pdfjs/`, `qpdf/`, `verapdf/`). Ces
  sous-dossiers sont ignorés par Git : la CI ne les a pas, chaque poste les
  récupère. La provenance, le commit et la licence de chaque lot sont dans
  `SOURCES.md`, généré par le script.
- **Fichiers maison**, prévus mais absents pour l'instant : exports ERP,
  scans d'atelier, sorties PDF24 / LibreOffice / Word / Chrome, anonymisés,
  à versionner ici avec git-lfs (`.gitattributes` y envoie déjà les PDF). Ne
  jamais commiter un document contenant des données réelles non anonymisées.
  Chaque fichier aura un `.expect` à côté, que le test ne lit pas encore :

  ```toml
  # nom.pdf.expect
  version = "1.4"
  pages = 14
  encrypted = false
  producer = "PDF24 Creator 11.3"
  ```

## Récupérer les suites publiques

```
python tools/fetch_corpus.py            # télécharge les lots absents
python tools/fetch_corpus.py --update   # retélécharge si le dépôt amont a bougé
python tools/fetch_corpus.py --list     # état des lots
```

Le script est idempotent : un lot déjà présent n'est pas retéléchargé. Il ne
garde que les `*.pdf` du sous-dossier de test de chaque dépôt, plus les
fichiers de licence d'origine dans `<lot>/_upstream/`.

| Source | Licence | Ce qu'elle apporte |
|---|---|---|
| pdf.js `test/pdfs` | Apache-2.0 | plusieurs centaines de fichiers réels, beaucoup de cas pathologiques |
| qpdf `qpdf/qtest/qpdf` | Apache-2.0 | xref streams, object streams, chiffrement, fichiers cassés |
| veraPDF corpus | CC-BY-4.0 | PDF/A et PDF/UA valides et invalides, un cas par règle ; contient la suite Isartor |
| Ghent Output Suite | conditions propres | PDF/X, impression ; à importer à la main dans `ghent/` |

## Le rapport

```
cargo test -p fyp-core --test corpus --release -- --nocapture
```

Le test `corpus_survey` ouvre chaque PDF du dossier, l'écrit dans les deux
styles de xref, relit le résultat et le compare à l'original. Il n'échoue
jamais sur un fichier : il écrit `target/corpus-report.md` (chemin modifiable
par `FYP_CORPUS_REPORT`), avec les totaux, puis les problèmes groupés par
cause et classés par priorité : paniques et délais dépassés, refus à
l'ouverture, round-trip en échec, tables reconstruites. Un dossier vide donne
un rapport vide et un test vert. Variables : `FYP_CORPUS_TIMEOUT_SECS` (60 par
défaut, par fichier) et `FYP_CORPUS_STRICT=1` pour faire échouer le test au
moindre problème.
