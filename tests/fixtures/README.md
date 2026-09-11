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

À ajouter au fil du développement : xref stream, object streams, fichier
chiffré RC4/AES, xref cassée (offsets faux), `startxref` manquant, `/Length`
indirect.
