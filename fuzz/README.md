# Fuzzing

```
cargo install cargo-fuzz
cargo +nightly fuzz run parse_object -- -max_total_time=1800
```

Tout crash produit un fichier dans `fuzz/artifacts/`. Le réflexe : le copier
dans `tests/fixtures/` sous un nom parlant, écrire le test de non-régression,
corriger, et seulement ensuite supprimer l'artefact.
