# ADR 0003 — Modèle de sécurité des modules

**Statut** : accepté — 2026-09

## Contexte
L'extensibilité par des tiers est voulue, mais « très sécuritaire sinon on ne
le fait pas ». Un module est considéré hostile par défaut.

## Décision
1. **Sandbox WebAssembly (Wasmtime + WASI)** pour tout module tiers. Aucune
   autorité ambiante : pas de disque, pas de réseau, pas de mémoire de l'hôte,
   pas d'accès aux autres modules.
2. **Permissions par capacités**, déclarées dans `manifest.toml`, accordées
   par exécution, jamais implicites (`fyp_plugin_api::Permission`). Réseau
   limité à des hôtes exacts ; sous-processus limité à un programme nommé ;
   dossiers choisis par l'utilisateur au moment de l'exécution.
3. **Re-validation** : tout document renvoyé par un module repasse par le
   parseur du noyau avant d'être accepté.
4. **Limites de ressources** par invocation (temps, mémoire, taille de
   sortie). Dépassement = arrêt propre, document source intact. Toute
   opération travaille sur une copie et remplace atomiquement à la fin.
5. **Signature et provenance** : les modules du catalogue sont signés ; code
   source public obligatoire ; modules non signés refusés sauf en mode
   développeur, affiché en permanence dans l'interface.
6. **Défense en profondeur** : `unsafe` interdit, fuzzing continu, mode
   « noyau seul » désactivant tout module tiers.

## Conséquences
- Les modules tiers sont ~1,5-3× plus lents qu'en natif : acceptable pour la
  manipulation de PDF ; les traitements lourds (OCR, rendu) restent des
  modules officiels natifs, revus.
- Les auteurs de modules ciblent WASM (Rust, C, Go, AssemblyScript…).
- L'interface doit exposer les permissions de façon lisible (orange pour les
  permissions sensibles, confirmation à la première exécution).
