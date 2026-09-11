# Fixtures

Petits PDF construits à la main, lisibles dans un éditeur de texte, un cas par
fichier. Ils servent aux tests unitaires du noyau.

- `minimal.pdf` — une page A4 vide, xref classique, PDF 1.7. Le plus petit PDF
  valide que doit accepter le noyau.

À ajouter au fil du développement : xref stream, object streams, fichier
chiffré RC4/AES, xref cassée (offsets faux), `startxref` manquant, `/Length`
indirect, mises à jour incrémentales.
