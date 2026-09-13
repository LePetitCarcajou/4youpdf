# 4YouPDF

**Le VLC du PDF.** Un logiciel PDF libre, gratuit, sans compte, sans pub, sans
envoi de fichiers sur Internet depuis son cœur, et hors du contrôle de tout
organisme lucratif.

- Ouvre les PDF, même cassés — et explique pourquoi quand il ne peut pas.
- Un noyau en Rust (`#![forbid(unsafe_code)]`). Chaque nuit, la CI en fuzze
  deux parties : le lexer et le parseur d'objets, et la lecture rapide de
  l'en-tête et de la fin du fichier. L'ouverture complète d'un document, les
  filtres, le chiffrement, l'écriture et les opérations de pages ne sont pas
  encore fuzzés (`docs/backlog-technique.md`).
- Un cœur, tout ce qui s'exécute hors de la sandbox des modules, qui n'accède
  jamais au réseau : ni télémétrie, ni mise à jour automatique, ni réglage pour
  l'activer.
- Des modules sandboxés (WebAssembly) avec permissions explicites : un module de
  fusion ne peut pas parler au réseau, et un module qui en a besoin ne pourra
  joindre que les serveurs exacts qu'il déclare, affichés et soumis à votre
  accord (voir `docs/adr/0006-fonctionnement-local.md`).
- Visées, pas encore là : la conformité visible en permanence (PDF/A, PDF/X,
  PDF/E, PDF/UA, PDF/VT, PAdES) avec correction assistée, et l'écriture native
  de PDF 2.0 (ISO 32000-2:2020).

## Installer

Aucune release publiée n'a encore de fichier à télécharger : celles de
v0.0.4 à v0.3.3 n'en ont aucun. Le workflow de release attachera aux
suivantes, sur la page
[Releases](https://github.com/LePetitCarcajou/4youpdf/releases), deux
fichiers par version pour Windows 10 et 11 (x64), au choix :

- **`4YouPDF_<version>_x64-setup.exe`**, l'installeur. Il installe 4YouPDF
  pour votre compte seulement, sans droits d'administrateur, dans
  `%LOCALAPPDATA%\4YouPDF`, avec un raccourci dans le menu Démarrer et, si
  vous laissez la case cochée, sur le bureau. La désinstallation, depuis
  « Applications installées », retire tout ce qu'il a mis en place. Si
  WebView2, le moteur d'affichage de Windows, manque (il fait partie de
  Windows 11), l'installeur le fait installer par Microsoft, ce qui demande
  une connexion à Internet.
- **`4YouPDF_<version>_x64_portable.zip`**, la version portable. Extrayez
  l'archive où vous voulez, clé USB comprise, puis lancez
  `4YouPDF\4YouPDF.exe`. Rien n'est installé et 4YouPDF n'écrit rien dans le
  registre : ce qu'il écrit, le cache du moteur d'affichage, reste dans
  `4YouPDF\data`, et supprimer le dossier supprime tout. WebView2 doit déjà
  être présent.

`SHA256SUMS.txt`, sur la même page, donnera l'empreinte de chaque fichier.
macOS et Linux n'ont pas encore de paquet : voir `app/README.md` pour
compiler l'application.

### « Windows a protégé votre ordinateur »

Au premier lancement d'un fichier téléchargé, l'installeur comme le
`4YouPDF.exe` de l'archive, Microsoft Defender SmartScreen affiche « Windows
a protégé votre ordinateur » : il « a empêché le démarrage d'une application
non reconnue ». C'est attendu : les fichiers ne sont pas signés. Signer
suppose un certificat de signature de code, payant et renouvelé chaque année,
que le projet n'a pas ; sans signature ni réputation établie auprès de
Microsoft, SmartScreen avertit par principe, sans avoir rien détecté. Pour
continuer : « Informations complémentaires », puis « Exécuter quand même ».

Ce n'est pas une confiance aveugle : le code est public, et ces fichiers
seront construits publiquement par la CI du dépôt, à partir du tag de la
version et sans intervention manuelle (`.github/workflows/release.yml` ;
chaque exécution et son journal sont visibles dans l'onglet Actions). Pour
vérifier que le fichier téléchargé est bien celui-là, comparez son empreinte
(`Get-FileHash <fichier>` dans PowerShell) à `SHA256SUMS.txt`, ou vérifiez
l'attestation de provenance que GitHub signe au moment de la construction :
`gh attestation verify <fichier> --repo LePetitCarcajou/4youpdf`.

Si le contrôle intelligent des applications de Windows 11 est activé, il
bloque les programmes non signés sans proposer de passer outre : 4YouPDF ne
s'y lance pas tant qu'il n'est pas signé.

## État

Version 0.3.3. Aucune release n'a encore de fichier à télécharger. Ce qui
existe :

- **Noyau** (`fyp-core`, `fyp-crypto`) : lecture des tables de références
  croisées (classiques, en flux, hybrides, chaîne `/Prev`), des object
  streams et des filtres Flate avec prédicteurs, ASCIIHex, ASCII85 et
  RunLength ; réparation par scan d'une table inutilisable ; déchiffrement
  du handler standard (RC4 et AES, révisions 2 à 6) ; écriture d'un fichier
  propre à une seule section, toujours en clair. Relevé du 13 septembre 2026
  sur le corpus public de 4 529 fichiers : 4 472 round-trips complets
  (ouverture, réécriture, relecture, comparaison), 44 refus à l'ouverture et
  13 échecs, chacun classé dans `docs/architecture.md`, aucune panique.
- **Opérations de pages** : fusion, extraction, découpage, rotation et
  suppression, par la ligne de commande `fyp`.
- **Modules** : modules WebAssembly exécutés dans une sandbox Wasmtime, avec
  limites de temps, de mémoire et de sortie, et re-validation par le noyau de
  ce qu'ils renvoient ; un module, la fusion, que lance `fyp run`. Seules les
  permissions de lecture et d'écriture de documents existent, et les modules
  ne sont pas signés (ADR 0003, « Limites connues »).
- **Application desktop** (`app/`, Tauri 2) : ouvrir un PDF, voir ses pages
  en vignettes ou une par une en grand, les réordonner, les faire pivoter,
  les supprimer, annuler et refaire, enregistrer le résultat. Les pages sont
  dessinées par PDFium (ADR 0005).
- **Empaquetage Windows** : un installeur NSIS et une archive portable, non
  signés, construits par `tools/package_app.py`.

Pas encore : chiffrement à l'écriture, écriture PDF 2.0, conformité
(`fyp-conformance` ne définit que des types), modules dans l'application,
palette de commandes, paquets pour macOS et Linux. La suite :
`docs/architecture.md`, « Feuille de route ».

## Compiler

Rust 1.95 au minimum pour le produit (`rust-version` de `Cargo.toml`, imposé
par Wasmtime 48), installé par rustup, et Python 3.11 ou plus pour les
scripts de `tools/`. Dans le dépôt, rustup prend la toolchain stable de
`rust-toolchain.toml`, avec la cible `wasm32-wasip1` des modules ; avec une
autre toolchain, Rust 1.95 compris, ajouter cette cible
(`rustup target add wasm32-wasip1 --toolchain 1.95`). Sous Linux,
l'application demande d'abord les bibliothèques système de `app/README.md`,
« Prérequis système ». Le contrat des modules, `fyp-plugin-api`, que
compilent les auteurs de modules, se contente de Rust 1.85.

Dans cet ordre, depuis la racine du dépôt (sous Linux et macOS, `python`
s'appelle souvent `python3`) :

```
cargo build --workspace
cargo test --workspace
cargo run -p fyp-cli -- info tests/fixtures/minimal.pdf
cargo run -p fyp-cli -- modules plugins --trusted
python tools/build_modules.py      # modules de plugins/ -> plugins/<nom>/module.wasm
cargo run --release --manifest-path tools/bench_host/Cargo.toml -- hog 8   # mesures du chargeur (ADR 0003, « Limites connues ») ; sans argument : la liste des commandes
cargo run -p fyp-cli -- run merge tests/fixtures/minimal.pdf tests/fixtures/objstm.pdf -o fusion.pdf
python tools/fetch_ui_tools.py
python tools/fetch_pdfium.py
python tools/build_ui.py
cargo run -p fyp-app
cargo run --release -p fyp-render-bench   # banc de fidélité du rendu, après fetch_pdfium.py et tools/fetch_corpus.py -> target/render-bench/ (docs/banc-rendu.md)
python tools/package_app.py        # Windows : installeur et archive portable -> target/release/bundle/
```

## Structure

| Dossier | Rôle |
|---|---|
| `crates/fyp-core` | syntaxe PDF, modèle objet, xref, filtres, écriture |
| `crates/fyp-crypto` | chiffrement standard (RC4, AES, révisions 2 à 6) |
| `crates/fyp-conformance` | types du futur moteur de règles PDF/A, X, E, UA, VT : aucune règle encore |
| `crates/fyp-plugin-api` | **contrat des modules** — versionné séparément |
| `crates/fyp-host` | chargement des modules, permissions, limites |
| `crates/fyp-cli` | binaire `fyp` |
| `plugins/` | modules officiels |
| `app/` | application desktop Tauri 2 : voir `app/README.md` |
| `tests/` | fixtures ; corpus public récupéré par `tools/fetch_corpus.py`, ignoré par Git |
| `fuzz/` | cibles cargo-fuzz |
| `docs/` | architecture, ADR, format de manifeste |

## Licence

AGPL-3.0-or-later. Contributions sous DCO (`git commit -s`), pas de CLA :
personne ne peut changer la licence de ce projet sans l'accord de chaque
contributeur. Voir `CONTRIBUTING.md`.
