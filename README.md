# 4YouPDF

**Le VLC du PDF.** Un logiciel PDF libre, gratuit, sans compte, sans pub, sans
envoi de fichiers sur Internet depuis son cœur, et hors du contrôle de tout
organisme lucratif.

- Ouvre tout, même les PDF cassés — et explique pourquoi quand il ne peut pas.
- Un noyau en Rust (`#![forbid(unsafe_code)]`), fuzzé en continu.
- Un cœur, tout ce qui s'exécute hors de la sandbox des modules, qui n'accède
  jamais au réseau : ni télémétrie, ni mise à jour automatique, ni réglage pour
  l'activer.
- Des modules sandboxés (WebAssembly) avec permissions explicites : un module de
  fusion ne peut pas parler au réseau, et un module qui en a besoin ne pourra
  joindre que les serveurs exacts qu'il déclare, affichés et soumis à votre
  accord (voir `docs/adr/0006-fonctionnement-local.md`).
- Conformité visible en permanence : PDF/A, PDF/X, PDF/E, PDF/UA, PDF/VT, PAdES,
  avec correction assistée.
- Écriture PDF 2.0 (ISO 32000-2:2020) native.

## Installer

Pour Windows 10 et 11 (x64), la page
[Releases](https://github.com/LePetitCarcajou/4youpdf/releases) propose deux
fichiers par version, au choix :

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

`SHA256SUMS.txt`, sur la même page, donne l'empreinte de chaque fichier.
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

Ce n'est pas une confiance aveugle : le code est public, et ces fichiers sont
construits publiquement par la CI du dépôt, à partir du tag de la version et
sans intervention manuelle (`.github/workflows/release.yml` ; chaque
exécution et son journal sont visibles dans l'onglet Actions). Pour vérifier
que le fichier téléchargé est bien celui-là, comparez son empreinte
(`Get-FileHash <fichier>` dans PowerShell) à `SHA256SUMS.txt`, ou vérifiez
l'attestation de provenance que GitHub signe au moment de la construction :
`gh attestation verify <fichier> --repo LePetitCarcajou/4youpdf`.

Si le contrôle intelligent des applications de Windows 11 est activé, il
bloque les programmes non signés sans proposer de passer outre : 4YouPDF ne
s'y lance pas tant qu'il n'est pas signé.

## État

Jalon 0.1 en cours : noyau syntaxique (lexer, parseur d'objets, détection de
version) et contrat des modules. Pas encore d'interface graphique — elle
arrive quand le noyau réécrit 100 % du corpus de test sans perte.

Compilation : Rust 1.95 au minimum pour le produit (`rust-version` de
`Cargo.toml`, imposé par Wasmtime 48). Le contrat des modules,
`fyp-plugin-api`, que compilent les auteurs de modules, se contente de
Rust 1.85.

```
cargo build --workspace
cargo test --workspace
cargo run -p fyp-cli -- info tests/fixtures/minimal.pdf
cargo run -p fyp-cli -- modules plugins --trusted
python tools/build_modules.py      # modules de plugins/ -> plugins/<nom>/module.wasm
cargo run --release --manifest-path tools/bench_host/Cargo.toml -- hog 8   # mesures du chargeur (ADR 0003, « Limites connues ») ; sans argument : toutes les commandes
cargo run -p fyp-cli -- run merge tests/fixtures/minimal.pdf tests/fixtures/objstm.pdf -o fusion.pdf
python tools/fetch_ui_tools.py && python tools/fetch_pdfium.py && python tools/build_ui.py
cargo run -p fyp-app
python tools/package_app.py        # Windows : installeur et archive portable -> target/release/bundle/
```

## Structure

| Dossier | Rôle |
|---|---|
| `crates/fyp-core` | syntaxe PDF, modèle objet, xref, filtres, écriture |
| `crates/fyp-crypto` | chiffrement standard (RC4, AES, révision 6) |
| `crates/fyp-conformance` | moteur de règles PDF/A, X, E, UA, VT |
| `crates/fyp-plugin-api` | **contrat des modules** — versionné séparément |
| `crates/fyp-host` | chargement des modules, permissions, limites |
| `crates/fyp-cli` | binaire `fyp` |
| `plugins/` | modules officiels |
| `app/` | application desktop Tauri (jalon 0.3) : voir `app/README.md` |
| `tests/` | fixtures et corpus (git-lfs) |
| `fuzz/` | cibles cargo-fuzz |
| `docs/` | architecture, ADR, format de manifeste |

## Licence

AGPL-3.0-or-later. Contributions sous DCO (`git commit -s`), pas de CLA :
personne ne peut changer la licence de ce projet sans l'accord de chaque
contributeur. Voir `CONTRIBUTING.md`.
