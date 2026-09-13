# Vérification différentielle d'un changement sans effet voulu

Pour un changement qui ne doit rien changer au comportement (correction de
lint, refactorisation, montée de dépendance) dans du code où un écart
passerait inaperçu : chiffrement, parseurs, filtres. Employée le
13 septembre 2026 sur les 14 corrections de clippy du MSRV 1.95.

Le principe : compiler le code de `HEAD` à côté de celui de l'arbre de
travail, leur donner les mêmes entrées, et s'arrêter au **premier écart de
comportement**, pas au premier plantage. Un fuzz ordinaire ne voit qu'une
panique ; un octet de décalage dans un déchiffrement n'en produit aucune.
Puis vérifier par la couverture LLVM que les lignes modifiées ont réellement
tourné.

L'outillage n'entre pas dans le dépôt : trois crates jetables, dans un
dossier hors du dépôt (`$S` ci-dessous), à reconstruire à chaque fois.

## 1. Les deux versions dans un même binaire

```sh
S=/un/dossier/jetable
mkdir -p "$S/old-head"
git archive --format=tar HEAD Cargo.toml crates/fyp-core crates/fyp-crypto \
  | tar -x -C "$S/old-head"
```

Dans `$S/old-head/Cargo.toml`, réduire `members` aux crates extraites et
retirer `exclude`. Si la copie porte la même version que l'arbre de travail,
Cargo refuse les deux (« package collision in the lockfile ») : lui donner
`0.0.0`, dans `[workspace.package]` comme dans `[workspace.dependencies]`.
Chaque harnais est une crate autonome (`[workspace]` vide) qui dépend des
deux versions sous deux noms :

```toml
[dependencies]
new-core = { package = "fyp-core", path = "<dépôt>/crates/fyp-core" }
old-core = { package = "fyp-core", path = "<S>/old-head/crates/fyp-core" }
new-crypto = { package = "fyp-crypto", path = "<dépôt>/crates/fyp-crypto" }
old-crypto = { package = "fyp-crypto", path = "<S>/old-head/crates/fyp-crypto" }
```

Une macro écrit une seule fois le code d'observation et l'instancie pour
chaque nom. Elle consigne tout ce qui s'observe : résultats avec leur
`Debug`, octets produits, empreinte des gros objets. Le harnais compare les
deux listes et panique sur la première différence, en la nommant. N'observer
que du déterministe : pas d'ordre d'itération d'une `HashMap`.

## 2. Aux bornes, exhaustivement

Un binaire ordinaire, compilé en `--release` avec `debug-assertions` et
`overflow-checks` dans son `[profile.release]`, parcourt les bornes du code
modifié :
- toutes les longueurs de 0 à quelques blocs (vide, bloc partiel, dernier
  bloc incomplet) ;
- les extrêmes des entiers ;
- toutes les entrées courtes quand leur nombre le permet.

Chaque section compte aussi ses issues (acceptées, refusées, sorties vides ou
non). Une comparaison dont tous les cas finissent sur la même erreur ne
prouve rien.

## 3. Par fuzz différentiel

Les cibles de `fuzz/` n'appellent pas `Document::open` (voir
`docs/backlog-technique.md`). La cible différentielle vit donc dans une crate
cargo-fuzz jetable (`[package.metadata] cargo-fuzz = true`). Des deux côtés,
elle ouvre le document avec le mot de passe vide (déchiffrement compris),
lit chaque objet, décode chaque flux et lance les opérations de pages.

```sh
cargo +nightly fuzz run --fuzz-dir "$S/fuzz-diff" --sanitizer address diff_document \
  "$S/fuzz-diff/corpus/diff_document" tests/fixtures tests/corpus \
  -- -max_total_time=600 -max_len=65536 -timeout=30 -rss_limit_mb=4096
```

Le premier dossier reçoit les entrées trouvées, les suivants ne servent
qu'à amorcer. Jamais `tests/corpus-private`. Sous Windows, la cible ne se lie
qu'avec `--sanitizer address` et demande `clang_rt.asan_dynamic-x86_64.dll`
dans le `PATH` (outils MSVC, `bin\Hostx64\x64`).

## 4. Les lignes modifiées ont-elles tourné ?

libFuzzer ne le dit pas de façon fiable : `-print_coverage=1` ne liste que
ce qui n'a **pas** été couvert, et l'inlining brouille l'attribution. On
rejoue donc toutes les entrées, corpus trouvé **et** dossiers d'amorce
tronqués à `-max_len`. Le rejeu passe par un troisième binaire jetable qui
fait les mêmes appels sur le seul code de l'arbre de travail, compilé avec la
couverture source de LLVM. Il prend en arguments ce qu'il rejoue (`document`),
la longueur maximale et les dossiers d'entrées. Depuis la racine du dépôt :

```sh
rustup component add llvm-tools-preview
TOOLS="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin"

RUSTFLAGS="-C instrument-coverage" cargo build --release --manifest-path "$S/cov-replay/Cargo.toml"
LLVM_PROFILE_FILE="$S/cov/%p.profraw" "$S/cov-replay/target/release/fyp-cov-replay" \
  document 65536 "$S/fuzz-diff/corpus/diff_document" tests/fixtures tests/corpus

"$TOOLS/llvm-profdata" merge -sparse "$S"/cov/*.profraw -o "$S/cov/all.profdata"
git diff -U0 HEAD -- crates/fyp-crypto/src/lib.rs | grep '^@@'   # lignes modifiées
"$TOOLS/llvm-cov" show "$S/cov-replay/target/release/fyp-cov-replay" \
  -instr-profile="$S/cov/all.profdata" -show-line-counts -use-color=false \
  "$PWD/crates/fyp-crypto/src/lib.rs" | grep -E '^ *(314|759|806)\|'
```

Chaque ligne sort sous la forme `  759|  30.0k|    for chunk in …` : le
deuxième champ est le nombre d'exécutions. Sous Windows, le binaire rejoué
porte l'extension `.exe` dans les deux commandes où il apparaît.

Un compte à `0` signifie que ni le corpus ni le fuzz n'ont atteint la ligne.
Il faut le dire, et ne s'appuyer alors que sur le point 2 ; c'était le cas
du prédicteur TIFF sur 16 bits.

## Ce que cela prouve, et rien de plus

Un comportement identique à `HEAD` sur les entrées essayées. Pas la justesse
de `HEAD`, qui reste l'affaire des tests, ni l'absence d'écart hors de ces
entrées. Le compte rendu donne, pour chaque site, l'ancien et le nouveau code
côte à côte, les bornes vérifiées, le nombre d'exécutions et les écarts
trouvés.
