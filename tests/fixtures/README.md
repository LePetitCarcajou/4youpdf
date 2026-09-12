# Fixtures

Petits PDF construits à la main, lisibles dans un éditeur de texte, un cas par
fichier. Ils servent aux tests unitaires du noyau.

- `minimal.pdf` — une page A4 vide, xref classique, PDF 1.7. Le plus petit PDF
  valide que doit accepter le noyau.
- `incremental.pdf` — `minimal.pdf` suivi d'une mise à jour incrémentale : une
  nouvelle version de l'objet 3 (MediaBox 612 × 792 au lieu de 595 × 842) et
  une section xref dont le trailer renvoie à la première par `/Prev 209`.
- `prev-loop.pdf` — `minimal.pdf` dont le trailer contient `/Prev 209`,
  l'offset de sa propre table xref : la chaîne `/Prev` boucle sur elle-même.

- `xrefstream.pdf` — les trois objets de `minimal.pdf` et un flux xref
  (ISO 32000-2, 7.5.8) non compressé, `/W [1 2 2]`, qui se liste lui-même.
  Le contenu du flux est binaire : 25 octets, cinq entrées de cinq octets.
- `objstm.pdf` — objets 1 et 2 (catalogue, arbre des pages) dans un object
  stream (7.5.7) compressé Flate ; la page reste un objet ordinaire ; flux xref
  Flate avec prédicteur PNG (`/Predictor 12 /Columns 5`).
- `hybrid.pdf` — fichier hybride (7.5.8.4) : la table classique ne connaît que
  les objets 1 et 2 ; la page (objet 3) est dans un object stream que seul le
  flux xref désigné par `/XRefStm` référence. Ce flux est encodé en
  ASCIIHex pour rester lisible.

Fichiers cassés, dérivés de `minimal.pdf`, que le noyau doit ouvrir en
reconstruisant la xref par scan (`Document::reconstructed()` non vide) :

- `bad-offsets.pdf` — les trois offsets de la table sont permutés : chaque
  entrée pointe sur le mauvais objet.
- `no-startxref.pdf` — les lignes `startxref` / `209` ont été supprimées.
- `garbage-xref.pdf` — la table et le trailer sont remplacés par du texte
  quelconque de même longueur ; `startxref` pointe toujours dessus.
- `prev-loop.pdf` — voir ci-dessus : la boucle `/Prev` est une table
  inutilisable, donc un cas de reconstruction.

`xrefstream.pdf`, `objstm.pdf` et `hybrid.pdf` sont produits par les tests
`#[ignore]` de `crates/fyp-core/tests/fixtures_gen.rs`, qui calculent les
offsets et écrivent des fichiers identiques à chaque exécution :

```
cargo test -p fyp-core --test fixtures_gen -- --ignored
```

À ajouter au fil du développement : fichier chiffré RC4/AES, `/Length`
indirect, LZWDecode.
