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

## Compiler sous Linux

Le workspace contient l'application desktop (`app/`, crate `fyp-app`),
construite sur Tauri 2 et liée à WebKitGTK. `cargo clippy --workspace` et
`cargo test --workspace` la compilent, donc il faut d'abord installer ses
bibliothèques système. Sous Debian ou Ubuntu (la CI tourne sur Ubuntu 24.04) :

```
sudo apt-get update
sudo apt-get install libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libsoup-3.0-dev pkg-config
```

Sans ces paquets, la compilation s'arrête sur une erreur de `pkg-config`
(`glib-2.0`, `gobject-2.0`…). Pour les autres distributions, voir les
[prérequis de Tauri 2](https://v2.tauri.app/start/prerequisites/) et
`app/README.md`. Pour travailler sur le noyau ou la CLI sans les installer,
`cargo test --workspace --exclude fyp-app` suffit localement ; la CI, elle,
compile tout.

Windows (WebView2, présent sur Windows 11) et macOS ne demandent rien de plus.

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
