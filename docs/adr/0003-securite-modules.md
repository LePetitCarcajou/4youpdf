# ADR 0003 — Modèle de sécurité des modules

**Statut** : accepté — 2026-09 ; mise en œuvre précisée au jalon 0.2 (voir
la dernière section)

## Contexte
L'extensibilité par des tiers est voulue, mais « très sécuritaire sinon on ne
le fait pas ». Un module est considéré hostile par défaut.

## Décision
1. **Sandbox WebAssembly (Wasmtime + WASI)** pour tout module tiers. Aucune
   autorité ambiante : pas de disque, pas de réseau, pas d'horloge, pas
   d'aléa, pas de mémoire de l'hôte, pas d'accès aux autres modules.
2. **Permissions par capacités**, déclarées dans `manifest.toml`, accordées
   par exécution, jamais implicites (`fyp_plugin_api::Permission`). Réseau
   limité à des hôtes exacts ; sous-processus limité à un programme nommé ;
   dossiers choisis par l'utilisateur au moment de l'exécution. Ce qu'une
   permission ouvre, c'est l'hôte qui le fournit lui-même : jamais un accès
   direct au système.
3. **Re-validation** : tout document renvoyé par un module repasse par le
   parseur du noyau avant d'être accepté, et seule sa réécriture par le
   noyau en sort.
4. **Limites de ressources** par invocation (temps, mémoire, taille de
   sortie), appliquées par le runtime. Dépassement = arrêt propre avec une
   erreur qui nomme la limite, document source intact, processus hôte
   jamais bloqué. Toute opération travaille sur une copie et remplace
   atomiquement à la fin.
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

## Mise en œuvre (jalon 0.2)

Ces précisions ne changent pas la décision ; elles fixent comment `fyp-host`
la tient.

**Runtime.** Wasmtime 48 (branche LTS), compilateur Cranelift, sans les
fonctions de Wasmtime inutiles ici (texte WAT, GC, threads, async, modèle
de composants). La crate `wasmtime-wasi` n'est **pas** utilisée : elle
donne accès à l'horloge, à l'aléa du système et à `poll_oneoff`, qui
peut bloquer l'hôte hors de portée de l'interruption. L'hôte implémente
lui-même le sous-ensemble de WASI preview 1 dont un module a besoin, sans
aucun appel au système :

| Fonction WASI | Ce que le module obtient |
|---|---|
| `fd_read` sur 0 | la requête encodée, puis fin de fichier |
| `fd_write` sur 1 | la réponse, jusqu'à la limite de sortie |
| `fd_write` sur 2 | des diagnostics, 64 Kio conservés |
| `fd_fdstat_get`, `fd_close`, `fd_seek` sur 0 à 2 | un périphérique caractère, sans positionnement |
| `fd_prestat_get`, `fd_prestat_dir_name` | `EBADF` : aucun dossier préouvert |
| `args_*`, `environ_*` | vides |
| `random_get` | une suite fixe, identique à chaque exécution : aucune entropie |
| `sched_yield`, `proc_exit` | rien à céder ; fin de l'exécution avec son code |

Toute autre importation (`path_open`, `sock_accept`, `clock_time_get`,
`poll_oneoff`, une fonction d'un autre espace de noms) est liée à un piège
qui porte son nom : le module s'arrête à son premier appel
(`HostError::CapabilityDenied`). Une importation qui n'est pas une
fonction, une mémoire partagée ou une signature WASI fausse font refuser le
module au chargement, avant l'exécution de la moindre instruction. Tout
pointeur venu du module est vérifié contre sa mémoire : une adresse fausse
vaut `EFAULT` pour le module, jamais une panique de l'hôte. La suite fixe de
`random_get` existe parce que les tables de hachage de la bibliothèque
standard de Rust demandent une graine au démarrage.

**Contrat d'appel.** Un module est une commande WASI (`_start` et `memory`
exportés), compilée pour `wasm32-wasip1`. L'hôte écrit sur son entrée
standard l'action, ses paramètres et les documents
(`fyp_plugin_api::exchange`, format binaire borné, décodage qui ne fait
confiance à aucune longueur) ; le module répond sur sa sortie standard par
un document ou un message d'erreur. Pas d'export `#[no_mangle]` : un module
en Rust reste sous `forbid(unsafe_code)` (`fyp_plugin_api::module::serve`).
Les paramètres sont déclarés dans le manifeste et vérifiés par l'hôte avant
le démarrage (nom, type, bornes, présence des obligatoires).

**Permissions en 0.2.** Seules `read_document` et `write_document`
existent dans l'hôte. Un module qui demande `read_dir`, `write_dir`,
`network` ou `subprocess` est refusé au chargement plutôt qu'exécuté sans
ce qu'il demande (`HostError::PermissionUnavailable`). Sans
`read_document`, un module ne reçoit aucun document ; sans
`write_document`, le document qu'il renvoie est refusé.

**Limites.** Chaque exécution a son propre `Store` Wasmtime et tourne sur
un thread dédié ; un second thread fait avancer l'époque du moteur toutes
les 10 ms.

- Temps : à chaque tic, le rappel d'échéance du store compare le temps
  écoulé à `timeout_ms` et arrête le module au-delà, y compris dans une
  boucle sans appel à l'hôte ou pendant l'instanciation.
- Mémoire : un `ResourceLimiter` refuse toute croissance au-delà de
  `memory_mib`, mémoire initiale comprise, et arrête le module
  (`HostError::MemoryExceeded`) au lieu de lui laisser un `-1`.
- Sortie : la sortie standard est plafonnée à `max_output_mib` (plus 64 Kio
  pour l'enveloppe de la réponse), vérifiée à chaque écriture ; un octet de
  trop arrête le module. Le tampon grandit par `try_reserve` : un hôte à
  court de mémoire arrête le module au lieu d'avorter.
- Plafonds de l'hôte, quel que soit le manifeste : 4 Gio de mémoire et de
  sortie (limite de wasm32), pile WebAssembly de 2 Mio, `module.wasm` de
  64 Mio au plus (la compilation ne s'interrompt pas, sa taille est donc
  bornée).

**Re-validation.** Le document renvoyé est ouvert par `Document::open`,
reconstruit par scan si sa table est inutilisable, puis réécrit par le
writer du noyau. La réécriture doit se rouvrir sans réparation et compter
au moins une page ; c'est elle, et jamais les octets du module, que l'hôte
remet à l'appelant. Un objet illisible, des octets parasites ou une section
de mise à jour supplémentaire ne franchissent donc pas la frontière. Une
reconstruction est signalée à l'appelant.

**Déterminisme.** Canonicalisation des NaN, SIMD relâché déterministe et
aléa fixe : un module donne le même résultat sur toutes les machines. Le
module de fusion produit les mêmes octets que `ops::merge` appelé
directement (testé).

**Modules du dépôt et noyau.** Un module peut embarquer `fyp-core` comme
bibliothèque : le code est compilé dans son binaire WebAssembly et s'exécute
dans la sandbox, il n'y apporte aucune autorité. C'est le cas du module de
fusion. Pour cela le noyau compile en 32 bits : les constantes au-delà de
`usize` sur wasm32 sont en `u64`, et l'`/ID` des fichiers écrits est un
MD5 du contenu (ISO 32000-2, 14.4), identique sur toutes les plateformes.
