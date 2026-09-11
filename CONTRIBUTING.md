# Contribuer à 4YouPDF

## Règle n°1 : DCO, pas de CLA

Chaque commit doit être signé avec `git commit -s`, ce qui ajoute une ligne
`Signed-off-by: Nom <email>`. Cela signifie que vous certifiez le
[Developer Certificate of Origin](https://developercertificate.org/) : vous
avez le droit de soumettre ce code sous AGPL-3.0-or-later.

Nous n'utilisons **pas** de CLA. Un CLA transfère des droits à une entité qui
peut ensuite changer la licence. Avec le DCO, chaque contributeur garde ses
droits ; relicencier exigerait l'accord de tous. C'est la garantie que le
projet reste libre.

## Flux de travail

1. Ouvrez une issue avant tout changement non trivial.
2. Branche depuis `main` : `feat/<sujet>` ou `fix/<sujet>`.
3. Commits en [Conventional Commits](https://www.conventionalcommits.org/) :
   `feat(core): parse xref streams`, `fix(cli): ...`, `docs(adr): ...`.
   Un changement cassant porte un `!` : `refactor(plugin-api)!: ...`.
4. PR vers `main`, fusionnée en *squash*. La CI doit être verte :
   `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo test --workspace`, `cargo deny check`.
5. Une PR qui touche `crates/fyp-plugin-api` doit dire si le changement est
   compatible ou cassant et ajuster la version de ce crate en conséquence.

## Règles de code

- `unsafe` est interdit dans tout le workspace.
- Le noyau ne panique jamais sur une entrée : toute erreur est une valeur.
  Pas de `unwrap()` / `expect()` hors tests.
- Tout nouveau cas de fichier pathologique donne une fixture + un test.
- Anglais pour le code et les commits ; français bienvenu dans les issues et
  la documentation utilisateur.

## Décisions d'architecture

Les choix structurants sont consignés dans `docs/adr/`. Proposer un changement
d'architecture = proposer un nouvel ADR.
