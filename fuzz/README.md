# Fuzzing

Les cibles de `fuzz_targets/`, lancées par cargo-fuzz (libFuzzer) sur une
toolchain nightly. Chaque nuit, `.github/workflows/fuzz.yml` les exécute
toutes, chacune pour une durée fixe, et garde en artefact l'entrée qui fait
échouer l'une d'elles. En local :

```
cargo install cargo-fuzz --version =0.13.2 --locked
cargo +nightly fuzz run document fuzz/corpus/document tests/fixtures tests/corpus -- -max_total_time=600 -max_len=65536 -timeout=60 -rss_limit_mb=4096
```

| Cible | Ce qu'elle appelle | Amorces |
|---|---|---|
| `document` | `Document::open` sur des octets arbitraires, avec le mot de passe vide puis, s'il est refusé, le mot de passe propriétaire des fixtures : en-tête, tables et flux de références croisées, chaîne `/Prev`, object streams, reconstruction par scan, `/Encrypt` et fyp-crypto ; puis chaque objet de la table, chaque flux décodé par ses filtres, l'arbre des pages, `ops::rotate` et `ops::merge`, et l'écriture dans les deux styles, chaque sortie rouverte et relue | `tests/fixtures`, `tests/corpus` |
| `filters` | les filtres seuls : la chaîne d'un dictionnaire de flux écrit en tête de l'entrée, puis chaque décodeur et les prédicteurs appelés directement avec des paramètres hostiles | `tests/fixtures` |
| `parse_object` | le lexer et le parseur d'un objet isolé | aucune |
| `quick_info` | l'en-tête, puis `startxref`, `/Encrypt` et `%%EOF` dans les derniers kilo-octets | aucune |
| `plugin_api` | le contrat des modules : un `manifest.toml` lu et validé, la requête et la réponse de l'échange décodées comme l'hôte et un module les décodent, puis réencodées | `plugins/merge` |
| `host_wasi` | les quatorze fonctions WASI de l'hôte, appelées avec des pointeurs, des longueurs et des compteurs hostiles, confrontées à un modèle de référence (ADR 0003) | aucune |

Le premier dossier reçoit les entrées trouvées ; les suivants ne servent qu'à
amorcer, tronqués à `-max_len` : un PDF coupé avant sa table est une amorce
de reconstruction. `tests/corpus` vient de `tools/fetch_corpus.py` ; jamais
`tests/corpus-private`. `document` et `filters` plafonnent le décodage à 16
et 4 Mio au lieu des 256 Mio du produit : même vérification, bombe attrapée
plus tôt.

Sous Windows, cargo-fuzz ne lie une cible qu'avec `--sanitizer address`, et
la lance si `clang_rt.asan_dynamic-x86_64.dll` (outils MSVC,
`bin\Hostx64\x64`) est dans le `PATH`.

Tout crash produit un fichier dans `fuzz/artifacts/`. Le réflexe : le copier
dans `tests/fixtures/` sous un nom parlant, écrire le test de non-régression,
corriger, et seulement ensuite supprimer l'artefact.
