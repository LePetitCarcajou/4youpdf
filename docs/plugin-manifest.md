# Format du manifeste de module

Fichier `manifest.toml` à la racine du module. Parsé et validé par `fyp-host`
**avant** tout chargement de code (`fyp_plugin_api::Manifest::validate`).

```toml
id = "org.4youpdf.merge"          # unique, style DNS inversé
name = "Fusionner"                # nom affiché
version = "0.1.0"                 # SemVer du module
api_version = "0.1.0"             # SemVer de fyp-plugin-api ciblée
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

[limits]                          # facultatif, valeurs par défaut sinon
timeout_ms = 60000
memory_mib = 512
max_output_mib = 2048

[[actions]]
id = "merge"
label = "Fusionner"
category = "pages"                # pages | process | security | conformance
min_inputs = 2
```

## Règles de validation

| Règle | Conséquence |
|---|---|
| `api_version` incompatible avec l'hôte | refusé (`IncompatibleApi`) |
| `runtime = "native"` hors du dépôt | refusé (`NativeNotAllowed`) |
| `network` sans hôte ou avec `*` | refusé (`BadNetworkHosts`) |
| `source` absent | accepté localement, refusé au catalogue |

## Permissions sensibles

`network`, `subprocess`, `write_dir` sont affichées en orange dans l'interface
et confirmées par l'utilisateur à la première exécution. `read_dir` et
`write_dir` ne donnent accès qu'au dossier choisi au moment de l'exécution,
jamais à un chemin fixé par le module.

## Compatibilité de version

Tant que `fyp-plugin-api` est en `0.x`, le **mineur** est la frontière de
compatibilité (convention SemVer pré-1.0). À partir de `1.0`, c'est le majeur.
