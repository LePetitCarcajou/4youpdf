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

Écarts à la norme fréquents dans le corpus, dérivés de `minimal.pdf` à
longueur constante (les offsets restent justes), que le noyau doit accepter
**sans** reconstruction :

- `gen-65536.pdf` — l'entrée libre de tête de la table porte la génération
  `65536` au lieu de `65535` (113 fichiers du corpus pdf.js).
- `inuse-offset-zero.pdf` — `xrefstream.pdf` plus une ligne de type 1 à
  l'offset 0 pour un objet 5 inexistant ; le lecteur la prend pour une entrée
  libre (22 fichiers du corpus). Produit par `fixtures_gen.rs`.
- `no-minor-version.pdf` — en-tête `%PDF-1.` sans chiffre mineur (suivi d'une
  ligne vide pour garder la longueur) ; lu comme PDF 1.0.
- `no-header.pdf` — aucun `%PDF` : la première ligne est un commentaire de
  huit octets au-dessus de 127. Le noyau tente le fichier avec la version
  supposée 1.4 et `quick_info` le signale (`header_present == false`).

Fichiers cassés, dérivés de `minimal.pdf`, que le noyau doit ouvrir en
reconstruisant la xref par scan (`Document::reconstructed()` non vide) :

- `bad-offsets.pdf` — les trois offsets de la table sont permutés : chaque
  entrée pointe sur le mauvais objet.
- `no-startxref.pdf` — les lignes `startxref` / `209` ont été supprimées.
- `garbage-xref.pdf` — la table et le trailer sont remplacés par du texte
  quelconque de même longueur ; `startxref` pointe toujours dessus.
- `prev-loop.pdf` — voir ci-dessus : la boucle `/Prev` est une table
  inutilisable, donc un cas de reconstruction.

Seconde série d'écarts, issue du nettoyage du rapport corpus (voir
`docs/architecture.md`, « Tolérances ») :

- `no-header-junk.pdf` — `minimal.pdf` dont la ligne `%PDF-1.7` est
  remplacée par huit octets quelconques (`XXXXXXXX`) : aucun en-tête, aucun
  commentaire, mais des objets. Accepté sans reconstruction.
- `root-dangling.pdf` — `minimal.pdf` avec `/Root 9 0 R` : la table est
  saine mais son `/Root` ne mène à rien ; reconstruction, le catalogue est
  retrouvé par son `/Type`.
- `startxref-off.pdf` — `minimal.pdf` avec `startxref 205` au lieu de
  `209` : la table est trouvée à côté, sans reconstruction
  (`Document::relocated_startxref() == Some(209)`).
- `object-zero.pdf` — `minimal.pdf` précédé d'un objet `0 0 obj` que la
  table liste en usage : entrée lue comme libre, objet ignoré, pas de
  reconstruction. Produit par `fixtures_gen.rs`.
- `root-direct.pdf` — objets 2 et 3 de `minimal.pdf`, catalogue écrit
  directement dans le trailer (`/Root << … >>`) : accepté, promu en objet
  indirect `4 0` par le writer. Produit par `fixtures_gen.rs`.

Fichiers chiffrés par le handler de sécurité standard (ISO 32000-2, 7.6),
mot de passe utilisateur vide, mot de passe propriétaire `owner`, produits
par `fixtures_gen.rs` avec `fyp-crypto` (vecteurs d'initialisation et sels
fixes, donc fichiers identiques à chaque exécution). Chacun contient la page
de `minimal.pdf` avec un flux de contenu Flate puis chiffré, et un
dictionnaire `/Info` dont le `/Title` est une chaîne chiffrée :

- `encrypted-rc4.pdf` — révision 3, `/V 2`, RC4 128 bits, PDF 1.4.
- `encrypted-aes256.pdf` — révision 6, `/V 5`, AES-256 par le crypt filter
  `/StdCF` (`/CFM /AESV3`), PDF 2.0.
- `encrypted-user-password.pdf` — `encrypted-rc4.pdf` avec le mot de passe
  utilisateur `user` : sans mot de passe, `Error::WrongPassword`. Le seul
  fichier du dépôt qui ne s'ouvre pas sans mot de passe, pour les cas où
  l'application en rencontre un sans pouvoir le demander (fusion).

`xrefstream.pdf`, `objstm.pdf`, `hybrid.pdf`, `inuse-offset-zero.pdf`,
`object-zero.pdf`, `root-direct.pdf`, `encrypted-rc4.pdf`,
`encrypted-aes256.pdf` et `encrypted-user-password.pdf` sont produits par les tests
`#[ignore]` de `crates/fyp-core/tests/fixtures_gen.rs`, qui calculent les
offsets et écrivent des fichiers identiques à chaque exécution :

```
cargo test -p fyp-core --test fixtures_gen -- --ignored
```

À ajouter au fil du développement : `/Length` indirect, LZWDecode.
