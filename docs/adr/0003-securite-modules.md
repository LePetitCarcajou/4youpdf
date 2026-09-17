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

**Découverte.** `manifest.toml` est lu sur 1 Mio au plus. Des modules d'un
même dossier qui déclarent le même identifiant sont tous refusés
(`HostError::DuplicateId`) : l'ordre d'un listage de dossier ne choisit pas
le code qui répond à un nom. `DiscoveredModule::trusted` n'est que ce que
l'appelant dit du dossier (voir « Limites connues »).

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
  trop arrête le module. Le tampon double sans jamais dépasser la limite,
  par `try_reserve_exact` : un hôte à court de mémoire arrête le module au
  lieu d'avorter. La réponse est décodée sur place
  (`Response::decode_owned`), sans seconde copie.
- Plafonds de l'hôte (`HostLimits`), quel que soit le manifeste : un
  manifeste qui déclare plus est refusé au chargement
  (`HostError::LimitAboveCeiling`). Par défaut 10 minutes, 4 Gio de mémoire
  (sans dépasser le budget commun ci-dessous) et 4 Gio de sortie (limite de
  wasm32). Sans ce plafond, `timeout_ms` acceptait 292 millions d'années.
  Pile WebAssembly de 2 Mio, `module.wasm` de 64 Mio au plus.
- Ce que partagent les exécutions d'un hôte (`budget.rs`, partagé par les
  clones de `Host`) : les limites d'un manifeste bornent une exécution, pas
  l'hôte. Au plus `max_concurrent_runs` exécutions à la fois (8 par défaut,
  moins sur une machine qui a moins de threads) ; les suivantes attendent et
  leur délai ne court qu'à leur démarrage. Chaque croissance de mémoire, de
  table ou du tampon de réponse est d'abord prise sur un budget commun
  (`memory_budget_mib`, 4 Gio par défaut) ; celle que le budget ne couvre
  pas arrête le module qui la demande (`HostError::HostMemoryExhausted`),
  et la part est rendue à la fin de l'exécution. La re-validation, qu'on ne
  peut pas interrompre, réserve six fois la taille de la réponse avant de
  commencer (5,6 mesuré sur le pire document trouvé). Mesure : sans ces
  règles, 16 modules aux limites du module de fusion (512 Mio chacun)
  occupaient 8,2 Gio ensemble et rien ne bornait leur nombre ; avec, 4,1
  Gio, en deux vagues de 8. Trente-deux fusions réelles simultanées
  (entrées de 10 et 8 Mio) sont passées de 2,9 à 0,9 Gio, dans le même
  temps (0,9 s).
- Texte : ce qu'un module fait lire à une personne (message d'erreur,
  sortie d'erreur, noms d'importation, messages de Wasmtime qui le citent)
  est tronqué (4 Kio, 64 Kio pour les diagnostics) et ses caractères de
  contrôle et de mise en forme bidirectionnelle sont remplacés par U+FFFD :
  ni séquence d'échappement pour le terminal, ni `fdp.exe` affiché
  `exe.pdf`. Un manifeste qui en porte dans un champ texte est refusé.
  Avant : un message d'1 Mio avec échappements passait intact.

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

## Limites connues

Audit du chargeur de modules du 2026-09-12 (Wasmtime 48.0.2). Chaque point
est une dette de sécurité assumée, classée par priorité, avec ce qui la
montre. Mesures sur un i7-11700KF (8 cœurs, 16 threads), 32 Gio, Windows 11,
build release, avec `tools/bench_host` (commandes en tête de son
`src/main.rs`).

### Priorité 1

**La compilation n'est pas bornée.** `Host::load` compile avec Cranelift
sur le thread de l'appelant et sur tous les cœurs. Rien ne l'interrompt ; ni
les plafonds ni le budget ne la couvrent. La limite de 64 Mio ne suffit
pas, car le coût dépend de la forme du code et non de sa taille :

| `module.wasm` hostile | Temps | Mémoire engagée |
|---|---|---|
| une fonction de 7,3 Mio : `local.get`/`i32.add`/`local.set` | 0,26 s | +125 Mio |
| une fonction de 7,3 Mio : `br_table` de 7 millions de cibles | 0,56 s | +302 Mio |
| une fonction de 7,3 Mio : 2,4 millions de blocs imbriqués | 4,6 s | +5,5 Gio |
| une fonction de 7,3 Mio : une boucle de `if`/`else` sur 1 000 locales | plus de 240 s (arrêtée) | +1,8 Gio à l'arrêt |
| 8 fonctions de ce dernier type (58 Mio) | plus de 240 s sur 8 cœurs (arrêtée) | +2 Gio à l'arrêt |
| 1 000 000 de fonctions de code mort (58 Mio) | 11,7 s, 95 s de CPU | +5,3 Gio |

Selon la taille d'une seule fonction :

| Motif | 64 Kio | 256 Kio | 1 Mio | 7,3 Mio |
|---|---|---|---|---|
| boucle de `if`/`else` sur 1 000 locales | 0,3 s, +296 Mio | 1,4 s, +1,2 Gio | 9 s, +4,7 Gio | plus de 240 s |
| blocs imbriqués | | 0,1 s, +181 Mio | 0,46 s, +716 Mio | 4,6 s, +5,5 Gio |

Et selon le nombre de fonctions de code mort (58 octets chacune) : 100 000
en 0,9 s (9 s de CPU, +546 Mio), 250 000 en 2,2 s (+1,3 Gio), 1 000 000 en
11,7 s (+5,3 Gio). Le temps croît plus vite que la taille pour le premier
motif ; la mémoire, d'environ 4,6 Gio par Mio de ce motif, se multiplie par
le nombre de fonctions compilées en parallèle.

Un module de moins de 64 Mio peut donc faire avorter l'hôte faute de
mémoire, ou l'occuper plusieurs minutes sur tous ses cœurs, avant
d'exécuter la moindre instruction. Un plafond de taille par fonction ne
suffit pas à le borner : seize fonctions de 1 Mio compilées en parallèle
demanderaient environ 75 Gio. Pour comparer, le module de fusion : 546 Kio,
529 fonctions, 19,6 Kio pour la plus grande, imbrication 60, 54 locales.
À faire, dans cet ordre : compilation hors du thread de l'interface ;
compilation dans un processus séparé, avec plafond de mémoire et de temps
du système, qu'on peut tuer (seule façon de borner Cranelift), puis
cache de l'artefact compilé ; plafonds structurels vérifiés avant par un
parcours linéaire (taille d'une fonction, imbrication, nombre de fonctions
et de locales), et `module.wasm` plafonné bien plus bas que 64 Mio.

**Pas de signature, pas de provenance** (point 5 de la décision, non
implémenté). Le chargement vérifie la syntaxe et la cohérence du manifeste
et la forme du binaire, rien d'autre. `DiscoveredModule::trusted` est ce que
l'appelant dit du dossier, et `Host::load` ne s'en sert pas. Un module qui
reprend l'identifiant et le manifeste du module de fusion est découvert,
chargé et exécuté comme lui ; le test
`known_gap_an_impostor_of_a_repository_module_is_not_told_apart` le montre
et devra être inversé. Les identifiants en double ne sont refusés qu'à
l'intérieur d'un même dossier : un appelant qui combine plusieurs dossiers
doit le vérifier lui-même. À faire : signature des modules du catalogue,
espace de noms `org.fouryoupdf.*` réservé aux modules signés, modules non
signés refusés hors du mode développeur.

### Priorité 2

**La re-validation n'est pas bornée dans le temps.** Elle tourne sur le
thread de l'appelant et ne s'interrompt pas. Sur un document de petits
objets vides (le pire cas trouvé), environ 60 ms et 5,6 Mio par Mio de
réponse : 64 Mio en 3,8 s, 256 Mio en 15 s, donc de l'ordre de 4 minutes
au plafond de sortie par défaut (4 Gio). Sa mémoire est réservée dans le
budget (six fois la réponse), mais ce facteur vient du pire cas trouvé,
pas d'une preuve. À faire : plafond de sortie par défaut plus bas, limites
propres au noyau (nombre d'objets), ou re-validation dans un processus
séparé.

**Pas d'annulation.** Une exécution ne s'arrête qu'à son délai (10 minutes
au plus par défaut) ; l'hôte n'offre pas à l'appelant de l'arrêter plus tôt.

**Identifiants libres.** `Manifest::validate` accepte tout identifiant non
vide. L'hôte refuse les caractères de contrôle, mais ni la forme
(nom de domaine inversé), ni la longueur. Les contraindre dans le contrat
est un changement cassant de `fyp-plugin-api`, avec changement de version.

### Priorité 3

- Hors budget : la requête (copie encodée des documents que l'appelant a
  choisis), les structures internes de Wasmtime par instance, les deux
  threads d'une exécution (16 Mio de pile réservés pour celui du module),
  la mémoire de compilation. Une croissance accordée par le limiteur puis
  ratée par Wasmtime garde sa part jusqu'à la fin de l'exécution.
- Texte du noyau : `HostError::Rejected` et `RunOutput::reconstructed`
  portent des `fyp_core::Error`, dont certaines citent le document (nom
  d'un filtre). L'interface doit les afficher de façon inerte ; c'est vrai
  aussi de tout PDF ouvert directement. Même chose pour les chemins que
  citent `HostError::Io`, `Manifest` et `DuplicateId` : sous Linux, un nom
  de dossier peut contenir un caractère de contrôle, et ils ne sont pas
  rendus inertes (il faut pouvoir écrire dans le dossier des modules).
- Époque : un thread d'époque par exécution. Avec N exécutions, chaque store
  est interrompu N fois par tic de 10 ms, ce qui reste négligeable à 8.
- Fuzzing : la cible `host_wasi` n'avait tourné qu'une fois, en local
  (148 668 scripts en 8 minutes avec ASan, sans divergence du modèle ni
  plantage), avant le 16 septembre 2026, où `fuzz.yml` s'est mis à la
  lancer chaque nuit pendant dix minutes, à côté du test à graine fixe
  `random_scripts_agree_with_the_model` de `cargo test` ; libFuzzer n'a
  pas dépassé des scripts de 261 octets, et la mémoire des scripts reste
  de 1 à 4 pages. Sous Windows, cargo-fuzz ne lie la cible qu'avec
  `--sanitizer address`, et la lance si `clang_rt.asan_dynamic-x86_64.dll`
  (outils MSVC) est dans le `PATH`.
- Wasmtime : aucun avis RustSec ne touche 48.0.2 dans la base du
  2026-09-09. Les 44 avis sur `wasmtime` sont tous corrigés dans une
  version antérieure, et plusieurs portent sur ce que l'hôte n'utilise
  pas : Winch, modèle de composants, WASI de Wasmtime, allocateur en pool ;
  les 3 avis sur `wasmtime-wasi` visent une crate absente de l'arbre. À
  revérifier à chaque mise à jour (`cargo deny check advisories`, en CI).
