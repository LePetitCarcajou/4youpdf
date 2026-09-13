# Backlog technique

Travaux sans effet visible pour l'utilisateur, consignés le 12 septembre
2026 : les garde-fous qui font tenir les décisions des ADR. Rien n'est
commencé.

- [ ] **Faire tenir l'ADR 0006 par l'outillage.** L'ADR 0006 réserve le
  réseau au relais de l'hôte qui sert un module autorisé, mais seules la CSP
  de la fenêtre et l'absence actuelle de code réseau le garantissent, sans
  qu'aucun contrôle de la CI n'empêche que cela change.
  - [ ] **Bannir les clients HTTP, TLS et WebSocket dans `deny.toml`, hors
    du crate qui implémente le relais.** La section `bans` n'interdit encore
    aucun crate et aucun client de ce genre n'est dans l'arbre des cibles que
    vérifie `cargo deny` (Tauri 2 ne tire `reqwest` et `hyper` que pour
    Android et iOS), donc l'interdiction peut entrer tout de suite ; le
    relais, qui n'existe pas encore, gagnera à vivre dans un crate à lui
    plutôt que dans tout `fyp-host`, car l'exception (`wrappers`) ne tolère
    que des dépendants directs nommés, et sa liste exacte dépendra du client
    qu'il choisira.
  - [ ] **Refuser `std::net` hors de ce même crate.** Aucun code du produit
    ne l'utilise ; un `clippy.toml`, absent du dépôt, qui liste ses types et
    fonctions dans `disallowed-types` et `disallowed-methods` en ferait une
    erreur dans la CI, qui passe déjà clippy avec `-D warnings`, seul le
    crate du relais portant l'autorisation, ce qui résiste mieux qu'une
    recherche textuelle aux alias d'import.
  - [ ] **Vérifier que la fenêtre ne peut pas naviguer vers une adresse
    extérieure, et sinon l'imposer côté Rust.** La CSP `default-src 'self'`
    ne régit que les ressources et les connexions, pas la navigation de la
    page elle-même, et Tauri 2.11 n'en bloque aucune tant qu'aucun
    `on_navigation` n'est fourni : à confirmer dans la fenêtre réelle, puis
    à imposer par un `on_navigation` (celui d'un plugin couvre aussi la
    fenêtre déclarée dans `tauri.conf.json`) qui n'accepte que l'origine de
    l'application, `tauri://localhost` sous Linux et macOS mais
    `http://tauri.localhost` sous Windows, si bien que l'exemple de la
    documentation de Tauri, qui ne teste que le schéma `tauri`, bloquerait
    l'application sous Windows.
