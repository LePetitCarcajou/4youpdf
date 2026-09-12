# Format du manifeste de module

Fichier `manifest.toml` à la racine du module. Parsé et validé par `fyp-host`
**avant** tout chargement de code (`fyp_plugin_api::Manifest::validate`).
Le code est dans `module.wasm`, à côté du manifeste : une commande WASI
compilée pour `wasm32-wasip1` (voir « Exécution » plus bas).

```toml
id = "org.4youpdf.merge"          # unique, style DNS inversé
name = "Fusionner"                # nom affiché
version = "0.1.0"                 # SemVer du module
api_version = "0.2.0"             # SemVer de fyp-plugin-api ciblée
license = "AGPL-3.0-or-later"     # SPDX ; doit être compatible AGPL pour le catalogue
source = "https://…"              # dépôt public obligatoire pour le catalogue
runtime = "wasm"                  # "wasm" (tiers) ou "native" (dépôt uniquement)

permissions = [
  { kind = "read_document" },
  { kind = "write_document" },
  # { kind = "read_dir" }, { kind = "write_dir" }
  # { kind = "network", hosts = ["api.example.org"] }   # hôtes exacts, pas de joker
  # { kind = "subprocess", program = "tesseract" }
]

[limits]                          # facultatif ; sinon 60000, 512, 1024
timeout_ms = 60000
memory_mib = 512
max_output_mib = 2048

[[actions]]
id = "merge"
label = "Fusionner"
category = "pages"                # pages | process | security | conformance
min_inputs = 2                    # 1 par défaut

[[actions]]                       # exemple d'action avec paramètres
id = "split"
label = "Découper"
category = "pages"

[[actions.params]]
id = "every"                      # unique dans l'action
label = "Pages par partie"
kind = "integer"                  # boolean | integer | text
required = true                   # false par défaut
min = 1                           # bornes : entiers seulement, facultatives
max = 10000
```

## Règles de validation

| Règle | Conséquence |
|---|---|
| `api_version` incompatible avec l'hôte | refusé (`IncompatibleApi`) |
| `runtime = "native"` hors du dépôt | refusé (`NativeNotAllowed`) |
| `network` sans hôte ou avec `*` | refusé (`BadNetworkHosts`) |
| une limite à zéro | refusé (`BadLimits`) |
| action ou paramètre sans identifiant, ou déclaré deux fois | refusé (`BadIdentifier`) |
| bornes sur un paramètre non entier, `min` au-dessus de `max` | refusé (`BadParam`) |
| `source` absent | accepté localement, refusé au catalogue |

Au chargement, `fyp-host` refuse en plus un `module.wasm` qui n'est pas une
commande WASI (pas d'export `_start` ou `memory`, mémoire partagée,
importation autre qu'une fonction) et, au jalon 0.2, toute permission autre
que `read_document` et `write_document`, que l'hôte ne sait pas encore
fournir.

## Permissions sensibles

`network`, `subprocess`, `write_dir` sont affichées en orange dans l'interface
et confirmées par l'utilisateur à la première exécution. `read_dir` et
`write_dir` ne donnent accès qu'au dossier choisi au moment de l'exécution,
jamais à un chemin fixé par le module.

`read_document` est nécessaire pour recevoir des documents,
`write_document` pour que le document renvoyé soit accepté.

## Exécution

Un module ne voit rien d'autre que ce que l'hôte lui passe :

1. L'hôte vérifie l'action, le nombre de documents (`min_inputs`) et les
   paramètres (déclarés, du bon type, dans leurs bornes, obligatoires
   présents), puis écrit la requête sur l'entrée standard du module.
2. Le module lit la requête, fait son travail et écrit sa réponse sur la
   sortie standard : un document, ou un message d'erreur pour
   l'utilisateur. La sortie d'erreur sert aux diagnostics (64 Kio gardés).
3. L'hôte relit le document par le noyau et n'en garde que la réécriture.

Le format de la requête et de la réponse est décrit dans
`fyp_plugin_api::exchange`. En Rust, `fyp_plugin_api::module::serve` s'en
charge :

```rust
fn main() -> std::process::ExitCode {
    fyp_plugin_api::module::serve(|request| {
        // request.action, request.params, request.documents
        Err(format!("action inconnue : {}", request.action))
    })
}
```

Le module ne dispose ni de fichiers, ni de réseau, ni d'horloge, ni d'aléa
(`random_get` rend une suite fixe) ; toute fonction WASI hors de ce que
l'hôte fournit l'arrête. Les limites sont appliquées pendant l'exécution :
dépasser `timeout_ms`, `memory_mib` ou `max_output_mib` arrête le module
avec une erreur qui nomme la limite. Détails dans l'ADR 0003.

Les modules de ce dépôt se construisent avec
`python tools/build_modules.py`, qui compile chaque crate de `plugins/` et
copie le binaire en `plugins/<nom>/module.wasm`.

## Compatibilité de version

Tant que `fyp-plugin-api` est en `0.x`, le **mineur** est la frontière de
compatibilité (convention SemVer pré-1.0). À partir de `1.0`, c'est le majeur.
La version 0.2 ajoute les paramètres d'action et le format d'échange : un
module écrit pour 0.1 est refusé.
