# ADR 0008 — Le rendu dans un processus à part

**Statut** : accepté — 2026-10 ; remplace le point 4 de l'ADR 0005 (un
thread pour PDFium) et précise son point 2 (où vit `pdfium-render`) ;
complété par la session v0.5.1-B (points 8 à 10, « Le gardien de la
mémoire », « Mesures »)

## Contexte
Jusqu'à la version 0.5.0, PDFium tournait dans le processus de la fenêtre,
sur un thread de `app/src/render.rs` (ADR 0005, point 4). PDFium est du
C++ qui lit des fichiers hostiles : un plantage, une pile dépassée ou une
mémoire épuisée du moteur emportaient l'application et le travail non
enregistré, et un dessin sans fin ne pouvait pas être abandonné, un thread
ne s'arrêtant pas de l'extérieur. C'était la première des dettes connues de
`docs/feuille-de-route.md`, et la deuxième condition de bascule de
`docs/mesure-hayro.md` (« un rendu qui ne fait ni tomber ni figer
l'application ») la pose pour tout moteur, hayro compris, qui dépasse sa
pile sur `qpdf/issue-202.pdf`.

## Décision
1. **Même exécutable, mode travailleur.** Le processus de rendu est
   `fyp-app` relancé avec l'argument réservé `--fyp-render-worker`, seul.
   `main()` le teste en toute première instruction
   (`render::worker::asked`), avant tout code de Tauri, de WebView2 ou de
   plugin ; le mode travailleur ne charge que PDFium. Pas de second
   binaire : rien de plus à construire en développement (`cargo run -p
   fyp-app` suffit) ni à empaqueter. L'argument n'est reconnu que premier
   et seul : `fyp-app document.pdf` ouvre toujours un fichier. Un fichier
   nommé `--fyp-render-worker` ne s'ouvre pas par la ligne de commande.
2. **Canal : l'entrée et la sortie standard du travailleur**, en trames
   binaires écrites à la main (`app/src/render/protocol.rs`), sans
   dépendance de sérialisation. La sortie d'erreur du travailleur va à
   `Stdio::null()`. Sous Windows, il est lancé sans fenêtre de console
   (`CREATE_NO_WINDOW`). Rien d'un document ne passe par la ligne de
   commande ni par l'environnement, que d'autres processus peuvent lire :
   les octets et le mot de passe voyagent dans la même trame.
3. **Le processus de la fenêtre ne fait pas confiance au travailleur.** Il
   vérifie chaque trame avant de s'en servir (« Ce que la fenêtre
   vérifie ») et encode lui-même le PNG (`app/src/render/png.rs`), à
   partir de pixels bruts dont il a vérifié les dimensions : WebView2 ne
   décode jamais des octets compressés fabriqués par un processus qui a pu
   être compromis.
4. **Une demande à la fois**, comme avant : PDFium n'est pas réentrant. Le
   service de la fenêtre (`RenderService`) sérialise les demandes ;
   l'encodage PNG se fait hors de ce tour, pendant que le travailleur peut
   déjà dessiner la page suivante.
5. **Le document est envoyé une fois par identifiant**, comme le cache du
   thread d'avant, et renvoyé, avec son mot de passe, au travailleur relancé.
   Un document que PDFium refuse d'ouvrir est refusé une fois, pas renvoyé
   pour chaque page.
6. **`pdfium-render` est confiné à `app/src/render/pdfium.rs`**, que seul
   le travailleur (`worker.rs`) appelle. Le banc de fidélité
   (`tools/render_bench`) compile ce fichier et `png.rs` tels quels : il
   mesure le même dessin et le même encodage.
7. **`fyp-app` a une cible bibliothèque** (`app/src/lib.rs`), qui ne
   contient que le rendu, pour que les tests de `app/tests/` lancent le
   service de la fenêtre contre le vrai exécutable `fyp-app`
   (`CARGO_BIN_EXE_fyp-app` n'existe que pour les tests d'intégration). Le
   service prend ce qui lance le travailleur par un trait (`Launch`) : les
   tests unitaires lui donnent de faux travailleurs, en mémoire, qui
   meurent, se taisent ou mentent, sans PDFium.
8. **Sous Windows, le travailleur meurt avec la fenêtre.** Aussitôt lancé,
   et avant d'être rendu au service, donc avant de recevoir `Hello`, le
   travailleur est placé dans un Job Object « tuer à la fermeture »
   (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`), dont seule la fenêtre tient la
   poignée, non héritable. Quand la fenêtre disparaît, de quelque façon
   que ce soit, le système ferme cette poignée et arrête le travailleur,
   même au milieu d'un dessin. Un travailleur qui ne peut pas y être placé
   est tué, et ce lancement échoue comme un démarrage raté. Le Job ne porte
   rien d'autre. L'appel système passe par `win32job` 2.0.3, dépendance
   ciblée Windows (`[target.'cfg(windows)'.dependencies]`), la seule que la
   session ajoute (go de Martin, 3 octobre 2026) : pas d'`unsafe` dans
   notre code, et rien de nouveau dans `Cargo.lock` hors d'elle (`windows`
   0.61 et `thiserror` 1 y étaient). Sous Linux, la fin de l'entrée
   standard reste le seul signal (« Limites connues »).
9. **La mémoire du travailleur est bornée par un gardien de la fenêtre**,
   sous Windows et sous Linux, qui le tue au-delà de 1 Gio plus trois fois
   la taille du document qu'il a reçu en dernier (« Le gardien de la
   mémoire »).
10. **Chargement de PDFium.** Un paquet (`cargo tauri build`) ne cherche
    la bibliothèque que dans le dossier de son exécutable. Un build
    compilé depuis le dépôt (`cargo run`, `cargo build --release`) cherche
    dans le dossier `FYP_PDFIUM_DIR`, puis dans `app/pdfium/`, puis dans le
    dossier de son exécutable : une copie périmée laissée dans
    `target/release/` n'est plus chargée (`library_candidates`, ADR 0005).

## Protocole (version 1)
Une trame : un octet qui la nomme, la longueur de sa charge sur quatre
octets, la charge. Tous les entiers sont petit-boutistes. La fenêtre envoie
des demandes, le travailleur une réponse par demande, dans l'ordre.

| Octet | Trame | Sens | Charge |
|---|---|---|---|
| `0x01` | `Hello` | fenêtre → travailleur | `FYPR`, version (u32), puis les dossiers où chercher PDFium : nombre (u32), et pour chacun longueur (u32) et chemin |
| `0x02` | `Document` | fenêtre → travailleur | numéro de demande (u64), identifiant (u64), longueur du mot de passe (u32), mot de passe, octets du fichier |
| `0x03` | `Draw` | fenêtre → travailleur | numéro de demande (u64), identifiant (u64), page à partir de 0 (u32), largeur en pixels (u32) |
| `0x81` | `Ready` | travailleur → fenêtre | version (u32), PDFium chargé ou non (un octet, 0 ou 1), texte : d'où, ou bien où il a été cherché |
| `0x82` | `Refused` | travailleur → fenêtre | version du travailleur (u32) |
| `0x83` | `Opened` | travailleur → fenêtre | numéro de demande (u64) |
| `0x84` | `Bitmap` | travailleur → fenêtre | numéro de demande (u64), largeur (u32), hauteur (u32), pixels RGBA, 8 bits par canal, ligne par ligne depuis le haut |
| `0x85` | `Failed` | travailleur → fenêtre | numéro de demande (u64), raison, en texte pour l'utilisateur |

- **Poignée de main.** La première trame est `Hello`. Le travailleur qui
  parle une autre version répond `Refused` et s'arrête : un numéro inconnu
  est un refus, des deux côtés (la fenêtre refuse de même un `Ready` d'une
  autre version). Un `Hello` d'une autre version n'est lu que jusqu'à sa
  version. Le travailleur qui ne trouve pas PDFium répond `Ready` avec
  « non » et la liste des chemins essayés, puis s'arrête : sans
  bibliothèque, aucun processus ne reste.
- **Chemins.** Sous Windows, un chemin voyage en unités de 16 bits, octet
  de poids faible d'abord ; ailleurs, en ses octets. Un nom qui n'est pas
  de l'Unicode passe tel quel.
- **Numéro de demande.** Tiré au hasard sur 64 bits pour chaque demande
  (les clés de `RandomState`, seul hasard de la bibliothèque standard) et
  repris par la réponse : une réponse écrite d'avance, ou celle d'une autre
  demande, ne porte pas le bon numéro. Ce n'est pas une frontière de
  sécurité, le travailleur étant de toute façon celui qui répond ; c'est ce
  qui empêche de prendre une réponse pour une autre.
- **Fin.** Le travailleur s'arrête quand il lit la fin de son entrée
  standard (code 0), ce qu'il ne fait qu'entre deux demandes, et sur toute
  trame qu'il ne comprend pas (code 2), sans répondre.

Ce que le protocole reprend de celui du banc (`tools/render_bench`,
`src/protocol.rs`), sans lier l'application au banc : un numéro de version
dont l'inconnu est refusé, une réponse par demande, et l'échec d'une page
distinct de l'échec du moteur. Le reste diffère par nécessité : le banc
lance un processus par document, lui écrit une demande en JSON et lit des
lignes ; l'application garde un travailleur, lui envoie les octets du
document et reçoit des pixels.

## Ce que la fenêtre vérifie
Dans `protocol::read_reply`, avant toute allocation :

- l'octet de la trame est l'un des cinq qu'un travailleur envoie ;
- la longueur annoncée est sous le plafond de ce genre de trame ; au-delà,
  la trame est refusée sur ses cinq premiers octets, sans rien lire de
  plus ;
- pour une image, les seize octets de numéro et de dimensions sont lus
  d'abord : largeur et hauteur non nulles, largeur d'au plus 4096, hauteur
  d'au plus 16 384, au plus 33 554 432 pixels, et longueur égale à
  largeur × hauteur × 4, en arithmétique sur 64 bits. Sinon l'image est
  refusée avant que ses pixels ne soient lus ;
- la charge est lue dans un tampon qui grandit avec ce qui arrive : une
  trame qui annonce beaucoup et apporte peu coûte peu ;
- un texte qui n'est pas de l'UTF-8 est lu avec des caractères de
  remplacement ; il ne sert qu'à être affiché comme texte.

Puis, dans `RenderService` : le numéro de la réponse est celui de la
demande, le genre de la réponse est l'un de ceux que la demande admet, et
la largeur de l'image est celle demandée. Les réponses lues attendent dans
une file d'une place : un travailleur qui répond sans qu'on lui demande
remplit son tuyau, la fenêtre n'en gardant que ce que disent les « Limites
connues ».

Le travailleur, lui, refuse en mots (`Failed`) une largeur hors de 16 à
4096 pixels et une page dont l'image dépasserait les plafonds (« la page N
est trop haute pour être dessinée à cette largeur »), et reste en service.

## Pannes et réponses
| Panne | Ce que fait la fenêtre | Ce que voit l'utilisateur |
|---|---|---|
| Le travailleur meurt pendant une demande (plantage, pile dépassée, tué) | la demande échoue ; un autre travailleur est lancé à la demande suivante, et reçoit de nouveau le document et son mot de passe | « le moteur de rendu s'est arrêté ; il sera relancé » sur la page demandée |
| Le travailleur est trouvé mort entre deux demandes | il est remplacé avant la demande, qui n'échoue pas ; la relance compte | rien |
| Pas de réponse dans le délai | le travailleur est tué, puis comme ci-dessus | « le moteur de rendu n'a pas répondu en 30 s ; il a été arrêté et sera relancé » |
| Le travailleur occupe plus que son plafond de mémoire | le gardien le tue ; puis comme un travailleur mort : pendant une demande, elle échoue ; entre deux demandes, il est remplacé avant la suivante | « le moteur de rendu a dépassé son plafond de mémoire (1024 Mio) ; il a été arrêté et sera relancé », le plafond en vigueur entre parenthèses |
| Réponse qui n'est pas celle de la demande, trame hors plafond ou mal formée | le travailleur est tué, puis comme ci-dessus | « le moteur de rendu a donné une réponse inattendue (…) ; il a été arrêté et sera relancé » |
| La même page fait tomber le travailleur deux fois | elle est refusée ensuite sans qu'un travailleur soit lancé pour elle, jusqu'à ce qu'une demande concerne un autre document | « la page N a arrêté le moteur de rendu deux fois ; elle n'est plus dessinée » |
| L'ouverture d'un document fait tomber le travailleur deux fois | toutes ses pages sont refusées de même | « ce document a arrêté le moteur de rendu deux fois à l'ouverture ; ses pages ne sont plus dessinées » |
| Plus de huit relances en une minute | plus aucun travailleur n'est lancé de la session ; `renderer_status` répond « indisponible » | la raison sur chaque page demandée, et dans la barre d'état dès qu'une page échoue |
| PDFium refuse le document ou la page | réponse `Failed` ; le travailleur reste | la raison donnée par le travailleur, les pages citées à partir de 1 |
| La fenêtre se ferme | l'entrée du travailleur est fermée, il est attendu deux secondes, tué sinon ; une demande en cours échoue aussitôt, son travailleur tué, et aucun travailleur n'est plus lancé ; une demande surprise pendant une poignée de main échoue à la fin de celle-ci, 10 s au plus | rien |
| La fenêtre disparaît (plantage, tuée) | sous Windows, le système ferme le Job et arrête le travailleur aussitôt, qu'il dessine ou non ; sous Linux, il ferme l'entrée du travailleur, qui s'arrête à la fin du dessin en cours s'il y en a un, aussitôt sinon (« Limites connues ») | rien |
| Le premier travailleur ne démarre pas, meurt ou se tait avant la poignée de main | le rendu reste « disponible » avec la raison en détail ; un travailleur est lancé à la première page demandée, et cette relance compte | « Aperçus : PDFium » dans la barre d'état, dont l'infobulle garde la raison du premier échec même après une relance réussie, l'interface ne relisant l'état qu'après une page qui échoue (`docs/backlog-ui.md`) ; si la relance échoue, la raison sur la page demandée |
| Le travailleur n'a pas de PDFium, ou parle une autre version | aucun autre n'est lancé de la session | « Aperçus indisponibles » et la raison dans la barre d'état, dès le démarrage |

« Tomber » couvre la mort, le silence, le mensonge et le dépassement du
plafond de mémoire.
Un autre document, pour ce compte, est un autre identifiant : une rotation
ou une fusion, qui réécrivent le document, repartent de zéro. L'interface
ne redemande pas d'elle-même une page qui a échoué : la deuxième chute
vient d'une autre demande de la même page, la vue après la vignette par
exemple.

Aucun composant d'interface nouveau : un échec passe par la vignette vide
qui porte sa raison, par le message de la vue d'une page et par la barre
d'état, que l'interface relit (`renderer_status`) après chaque page qui
échoue.

## Plafonds
| Plafond | Valeur | Pourquoi |
|---|---|---|
| Largeur d'une image | 16 à 4096 pixels | celle d'avant, inchangée (`app/README.md`, « Zoom ») |
| Hauteur d'une image | 16 384 pixels | quatre fois la largeur maximale : une page quatre fois plus haute que large à 4096 pixels, ou seize fois à 1024 |
| Pixels d'une image | 4096 × 8192 = 33 554 432, soit 128 Mio en RGBA | une page deux fois plus haute que large à la largeur maximale ; une page A4 y fait 4096 × 5793 pixels, 95 Mio, mesurée à 4096 × 5796 sur `encrypted-rc4.pdf` |
| Texte d'une trame | 8 Kio | un état qui cite trois chemins essayés, ou une raison d'échec ; coupé entre deux caractères par celui qui écrit |
| `Hello` | 64 Kio | trois chemins, chacun pouvant atteindre 32 767 unités sous Windows, n'y tiennent pas tous ; un chemin réel fait quelques centaines d'octets |
| `Document` | 4 Gio moins un octet, mot de passe compris | la longueur d'une trame tient sur quatre octets ; au-delà, « document trop grand pour le moteur de rendu », sans rien envoyer |
| Délai de la poignée de main | 10 s | charger une bibliothèque de quelques mégaoctets, antivirus compris ; elle a lieu avant que la fenêtre ne s'ouvre |
| Délai d'une demande (ouvrir un document, ou dessiner une page) | 30 s | plus de vingt fois la page la plus lente du banc en release (1 388 ms, le plan A1, `docs/banc-rendu.md`) et de dix fois la page A4 à 4096 pixels d'un build de développement (2,1 s de bout en bout, `docs/backlog-technique.md`) ; la moitié des 60 s après lesquelles `docs/mesure-hayro.md` tient une page pour non rendue |
| Chutes d'une page, ou de l'ouverture d'un document | 2 | une chute peut venir d'ailleurs (travailleur tué, mémoire du système) ; deux désignent la page |
| Relances | 8 par fenêtre glissante de 60 s | une page nuisible coûte au plus deux relances : quatre pages nuisibles vues en une minute passent, une boucle de relances s'arrête en moins d'une minute |
| Adieu à la fermeture | 2 s | le travailleur s'arrête dès que son entrée se ferme ; passé ce temps il est tué |
| Mémoire du travailleur | 1 Gio + 3 × la taille du document reçu en dernier ; 1 Gio avant tout document ; pendant l'ouverture d'un document, le plus grand de son plafond et de celui du précédent | décision de Martin, 3 octobre 2026. Pics mesurés en build release (mémoire engagée) : 185 Mio pour une page A4 à 4096 pixels (`mixed12.pdf`, `encrypted-rc4.pdf`), 169 Mio pour `pdfjs/bomb_giant.pdf` à 4096, 296 Mio pour le plan A1 de 6 Mio (`pdfjs/22060_A1_01_Plans.pdf`) à 4096, dont 17 Mio une fois le document ouvert ; 25 Mio pour une A4 à 1400 pixels. 1 Gio laisse plus de cinq fois le pic A4 ; les trois fois la taille couvrent la trame reçue, la copie de PDFium et ce qu'il en lit (2,7 fois la taille pour le plan A1) |
| Lecture de la mémoire | toutes les 50 ms | la fenêtre de dépassement (« Le gardien de la mémoire ») contre le coût d'une lecture, un appel système |

Les plafonds du protocole sont des constantes de `protocol.rs` ; les délais,
le nombre de relances, le plafond de mémoire et la cadence du gardien,
`Limits::default()` dans `render.rs` ; les deux chutes, `FALLS`.

## Le gardien de la mémoire
Un thread de la fenêtre, `render-guard`, démarré avec le service avant le
premier travailleur, lit toutes les 50 ms la mémoire que le travailleur
occupe, par le système et sans rien lui demander
(`app/src/render/memory.rs`), et le tue quand elle dépasse son plafond. La
demande en cours échoue alors comme pour un travailleur mort, en disant
pourquoi ; la relance et le compte des chutes sont ceux de « Pannes et
réponses ». Un seul mécanisme sur les deux systèmes, sans dépendance de
plus, avec un seuil qui suit la taille du document, que la fenêtre connaît
puisqu'elle l'envoie : décision de Martin (3 octobre 2026), plutôt qu'une
limite que le travailleur s'appliquerait (`RLIMIT_AS` sous Linux) ou qu'un
plafond de mémoire physique sur le Job, qui ne tue pas.

- **Ce qui est lu.** Sous Windows, la mémoire engagée du processus
  (`PagefileUsage` de `GetProcessMemoryInfo`, par
  `win32job::utils::get_process_memory_info`) : ce qu'il a réservé et que
  le système doit pouvoir lui fournir, même s'il n'y a pas encore écrit.
  Sous Linux, `RssAnon` plus `VmSwap` de `/proc/<pid>/status` : sa mémoire
  privée, en mémoire ou échangée. Pas `VmRSS` seul, qui compte aussi les
  pages de la bibliothèque PDFium, partagées et sans coût, et ignore ce qui
  est parti dans l'échange, où une page qui ne fait qu'allouer grandirait
  sans être vue ; un noyau sans `RssAnon` (avant 4.5) donne `VmRSS`. Un
  processus fini et pas encore attendu n'a plus ces lignes : rien n'est lu.
  Ailleurs (macOS), rien n'est lu, et le travailleur n'a pas de plafond
  (`docs/backlog-technique.md`, avant de livrer macOS).
- **Quand.** Le gardien tourne avant que le premier travailleur ne soit
  lancé. Chaque travailleur est tenu au plafond sans document, 1 Gio, dès
  que le service le connaît, avant `Hello` ; le plafond d'un document est
  posé avant que son premier octet ne parte. Le plafond est donc en place
  avant que le travailleur ne lise le premier octet d'un document, sur
  chaque système où il existe.
- **Changement de document.** Le travailleur garde le document précédent
  jusqu'à ce qu'il ait lu la trame du suivant : jusqu'à sa réponse, le
  plafond est le plus grand des deux, puis celui du nouveau. Un travailleur
  qui garde ensuite plus que ce plafond (mémoire non rendue au système) est
  tué ; entre deux demandes, il est remplacé sans que la demande suivante
  échoue.
- **Fenêtre de dépassement.** Entre deux lectures, rien ne retient le
  travailleur. Mesuré le 3 octobre 2026 (build release, Intel Core
  i7-11700KF, 32 Gio, Windows 11) par une sonde Python qui n'est pas dans
  le dépôt : elle lance `target/release/fyp-app.exe --fyp-render-worker`,
  lui envoie `Hello` (`app/pdfium/`), `mixed12.pdf` puis la demande de sa
  première page à 4096 pixels, et, sur un autre thread, lit sa mémoire
  engagée (`GetProcessMemoryInfo`, `PagefileUsage`, par `ctypes`) toutes
  les 50 ms, le tue au-delà du plafond et note la valeur lue, le pic
  (`PeakPagefileUsage`) et les instants ; 10 essais par plafond :
  - plafond de 16 ou de 64 Mio : le dépassement est vu à la première
    lecture, 50 à 51 ms après la demande ; le travailleur occupait déjà
    94 Mio (le bitmap alloué) et avait atteint son pic de 185 Mio ; il
    était mort 20 ms plus tard au plus ;
  - plafond de 128 Mio : 5 essais sur 10 ont échappé au gardien, la page
    ayant été dessinée, envoyée et sa mémoire rendue entre deux lectures ;
    les 5 autres l'ont vu à 185 Mio.

  Un processus qui ne fait qu'écrire en mémoire en remplit environ 8 Gio/s
  sur cette machine (Python, `b'\x01' * (1 << 30)` : 1 Gio en 124 ms) :
  en 50 ms de lecture et 20 ms pour mourir, un travailleur hostile peut
  écrire de l'ordre de 0,5 Gio au-delà de son plafond, et en engager
  davantage sans y écrire, jusqu'à la limite de mémoire engagée du
  système, où ses allocations échouent. Un plafond posé sur le Job
  (`JOB_OBJECT_LIMIT_PROCESS_MEMORY`) refuserait l'allocation elle-même ;
  `win32job` ne l'expose pas, et l'appel direct demanderait `unsafe` : il
  est au backlog, comme seconde barrière.

## Limites connues
- **Ce n'est pas encore un bac à sable.** Le travailleur garde les droits
  de l'utilisateur : un défaut de PDFium exploité y lit et écrit les
  fichiers de l'utilisateur et peut atteindre le processus de la fenêtre
  par les moyens du système. L'isolation protège aujourd'hui du plantage,
  du blocage et des réponses fausses, pas d'un code hostile qui s'exécute.
  Restreindre ses droits est au backlog, avec sa condition
  (`docs/backlog-technique.md`).
- **Un plafond fait pour des pages simples.** Les 185 Mio d'une page A4
  à 4096 pixels sont ceux d'une page sans effets. Le testeur de la
  session B a vu une page de 4,9 Kio, faite de 10 groupes de transparence
  imbriqués avec masque doux, tuée par le gardien à 4096 pixels ; 8
  niveaux passent à 980 Mio, et 16 niveaux ne prennent que 192 Mio à 1400
  pixels. D'après cette mesure, environ 80 Mio par niveau à 4096 pixels ;
  vraisemblablement un bitmap de la page par groupe, ce que rien n'a
  profilé. Une telle page s'affiche à la
  taille de la vue et est refusée au zoom maximal
  (`docs/backlog-technique.md`).
- **Une mémoire bornée par lecture, pas par le système.** Le gardien voit
  le travailleur toutes les 50 ms : une page qui alloue et rend sa mémoire
  entre deux lectures lui échappe, et un travailleur hostile peut dépasser
  son plafond de l'ordre de 0,5 Gio avant d'être tué (« Le gardien de la
  mémoire »). Le travailleur dessine l'image avant de la comparer aux
  plafonds du protocole. Sous macOS, aucun plafond. Sous Linux, le gardien
  n'a été exécuté que sans PDFium (la CI ne le télécharge pas, et la
  session a été menée sous Windows) : son effet sur un vrai dessin n'y est
  pas vérifié (`docs/backlog-technique.md`).
- **Sous Linux, un travailleur peut survivre à la fenêtre, le temps du
  dessin en cours** : il ne s'arrête qu'en lisant la fin de son entrée, ce
  qu'il ne fait qu'entre deux demandes. Oisif, il s'arrête aussitôt.
  Pendant un dessin sans fin, il n'a pas de borne : la fenêtre n'est plus
  là pour appliquer le délai de 30 s ni le plafond de mémoire. Le testeur
  de la session A l'a mesuré encore vivant 90 s après la fermeture de son
  entrée, sous Windows, sur une page de 3,4 Kio faite de formulaires
  imbriqués (10⁶ remplissages). Sous Windows, le Job l'arrête aussitôt
  depuis la session B ; sous Linux, l'attacher à la fenêtre
  (`PR_SET_PDEATHSIG` ou un équivalent sans `unsafe`) est au backlog,
  avant de livrer l'application sous Linux.
- **Des démarrages ratés lents ne coupent pas le rendu.** Une poignée de
  main muette coûte 10 s : six relances par minute au plus, sous la limite
  de huit, et chaque page demandée relance alors un travailleur
  (`docs/backlog-technique.md`).
- **Une inondation de réponses non demandées** coûte à la fenêtre jusqu'à
  trois images au plafond, environ 400 Mio, et non une : une dans la file,
  une en lecture, et la croissance du tampon de lecture. C'est borné ; le
  reste attend dans le tuyau du travailleur.
- **Les poignées du canal et les autres processus enfants.** La
  bibliothèque standard crée les bouts du canal destinés au travailleur
  comme héritables, le temps de le lancer. Un autre processus lancé au même
  instant par un autre code (WebView2 lance les siens) pourrait en hériter,
  et la fin du travailleur ne fermerait alors plus sa sortie. La fenêtre
  n'attend jamais un tuyau, seulement une file avec délai : le pire cas est
  une demande qui échoue au bout de 30 s au lieu d'aussitôt.
- **Le coût.** Les pixels traversent un tuyau : 11 Mio pour une page A4 de
  1400 pixels de large, 95 Mio à 4096. Quelques millisecondes par page
  rapide, rien de mesurable sur une page lente (« Mesures »).
- **Deux copies du document**, comme avant (ADR 0005), mais dans deux
  processus : celle du noyau dans la fenêtre, celle de PDFium dans le
  travailleur, plus, le temps de l'ouverture, la trame reçue.
- **Très hautes pages.** Une page plus de deux fois plus haute que large
  n'est plus dessinée à la largeur maximale, ni une page dont l'image
  dépasserait 16 384 pixels de haut : la vue dit pourquoi. Avant, PDFium
  tentait un bitmap de la taille demandée, quelle qu'elle soit.

## Mesures
Temps de bout en bout d'une demande de rendu, de l'appel de
`RenderService::render` au PNG, en build release : avant, v0.5.0, PDFium
sur un thread de la fenêtre ; après, le travailleur de cette ADR, avec son
Job et son gardien. Page 1 de chaque fichier, en vignette (160 pixels de
large) et en page de la vue (1400), 30 demandes au même service, trois
passes où les deux versions alternent : médiane de 90 demandes par case.
Machine : Intel Core i7-11700KF, 32 Gio, Windows 11 ; rustc 1.98.1 ;
PDFium chromium/8044 (`app/pdfium/`) ; corpus pdf.js au commit
`fd453c2ce3e1` (`tests/corpus/SOURCES.md`), que `fetch_corpus.py`, qui
suit la branche par défaut, peut ne plus donner. Pris le 3 octobre 2026
sur l'arbre de travail de la session v0.5.1-B (base `390eeec`), avant que
le plafond ne suive aussi le document précédent pendant une ouverture, ce
qui ne touche pas une demande sur un même document.

| Fichier | Largeur | v0.5.0 | Après | Écart | Critère |
|---|---|---|---|---|---|
| `tests/fixtures/mixed12.pdf` (12 Kio) | 160 | 0,68 ms | 0,80 ms | +0,12 ms (+18,7 %) | ≤ +10 ms : tenu |
| `tests/fixtures/mixed12.pdf` | 1400 | 8,50 ms | 15,28 ms | +6,78 ms (+79,7 %) | ≤ +10 ms : tenu |
| `pdfjs/22060_A1_01_Plans.pdf` (6,0 Mio) | 160 | 1 419,32 ms | 1 407,16 ms | −0,9 % | ≤ +15 % : tenu |
| `pdfjs/22060_A1_01_Plans.pdf` | 1400 | 1 481,80 ms | 1 471,46 ms | −0,7 % | ≤ +15 % : tenu |
| `pdfjs/issue3188.pdf` (7,7 Mio) | 160 | 0,35 ms | 0,46 ms | +0,11 ms (+28,7 %) | ≤ +10 ms : tenu |
| `pdfjs/issue3188.pdf` | 1400 | 3,08 ms | 7,19 ms | +4,11 ms (+133,1 %) | ≤ +10 ms : tenu |

**Critère** (Martin, 3 octobre 2026 ; il remplace le seuil de 15 % du
brief, relatif partout) : au plus 15 % au-dessus de v0.5.0 là où le dessin
domine, une médiane de v0.5.0 au-delà de 100 ms par page ; ailleurs, au
plus 10 ms de surcoût absolu, à 160 comme à 1400 pixels. Un seuil relatif
punit les pages que PDFium dessine en 3 ms, où quelques millisecondes de
transport font plus que doubler le temps.

**Hypothèse, non vérifiée** : le surcoût vient de la traversée du tuyau
par les pixels bruts (11 Mio pour une page A4 à 1400 pixels, 141 Kio
pour une vignette), introduite par la session A, et non du Job ni du
gardien : il est d'un dixième de milliseconde en vignette et de 4 à 7 ms à
1400 pixels. Rien n'a été profilé. Réduire ce coût est au backlog, rattaché
au chemin PNG puis base64 vers l'interface, qui coûte bien plus
(`docs/backlog-technique.md`).

**Reproduire**, depuis la racine du dépôt :

```
python tools/fetch_pdfium.py
python tools/fetch_corpus.py
python tools/render_timing.py
```

`tools/render_timing.py` tire `git archive v0.5.0` dans
`../4YouPDF-v0.5.0`, hors du dépôt, s'il n'y est pas, y ajoute
`tools/render_timing/baseline_v0_5_0.rs` comme exemple de `fyp-app`, qui
compile le `render.rs` de v0.5.0 tel quel, compile les deux côtés en
release, fait alterner `--rounds` passes de `--requests` demandes, puis
imprime ce tableau et applique le critère (code de sortie 1 s'il n'est pas
tenu). Le côté d'après est `app/examples/render_timing.rs`. D'autres
fichiers se donnent en arguments. Les temps ne valent que pour la machine
où ils sont pris.

## Conséquences
- Tuer le processus de rendu depuis le Gestionnaire des tâches ne ferme
  pas la fenêtre, et le rendu reprend à la page suivante.
- Sous Windows, tuer la fenêtre depuis le Gestionnaire des tâches arrête
  aussi son processus de rendu.
- Une page qui fait enfler le moteur au-delà de son plafond est refusée
  après deux chutes, comme une page qui le fait planter.
- Le Gestionnaire des tâches montre deux processus `fyp-app` quand PDFium
  est là, un seul sinon.
- L'état du rendu n'est plus fixé au démarrage : `renderer_status` peut
  passer à « indisponible » en cours de session.
- Les messages de rendu citent les pages à partir de 1.
- Remplacer PDFium revient toujours à réécrire un fichier, `pdfium.rs` ; le
  protocole, le service et ses pannes ne connaissent pas le moteur.

## Tests
- `app/src/render/protocol.rs` : aller et retour de chaque trame ; trames
  hostiles (longueur énorme, refusée sur cinq octets lus ; dimensions
  fausses, refusées sur seize ; trame tronquée à chaque octet ; genre
  inconnu ; version inconnue ; 4 000 suites d'octets au hasard, d'une graine
  fixe), chacune une erreur, sans panique.
- `app/src/render/tests.rs` : le service contre de faux travailleurs en
  mémoire : mort pendant un dessin, silence, huit mensonges, réponse non
  demandée, deux chutes d'une page ou d'une ouverture, relances comptées
  sur une fenêtre de temps, rendu coupé, poignées de main refusées,
  fermeture, fermeture pendant une demande, demandes concurrentes.
- `app/tests/render_worker.rs` : le vrai exécutable. Sans PDFium : refus
  d'une autre version, arrêt à la fermeture de l'entrée, arrêt sur ce qui
  n'est pas le protocole, bibliothèque absente dite. Avec PDFium
  (`tools/fetch_pdfium.py`) : les rendus d'avant à travers le travailleur,
  travailleur tué pendant un dessin puis demande suivante servie, fichier
  chiffré redessiné après une relance, mot de passe absent de la ligne de
  commande, page trop haute refusée.
- `tools/ui_smoke/render_worker.py` : la fenêtre réelle, son travailleur
  tué entre deux demandes puis pendant un dessin, la page refusée après
  deux chutes, le rendu coupé après huit relances et la barre d'état qui le
  dit, aucun processus `fyp-app` après la fermeture ni après la mort de la
  fenêtre, et, depuis la session B, la fenêtre tuée pendant que son
  travailleur dessine une page de plusieurs minutes : le travailleur
  arrêté aussitôt.
- Session B. `app/src/render/memory.rs` : la lecture de
  `/proc/<pid>/status`, sur tous les systèmes. `app/src/render/tests.rs` :
  le gardien contre de faux travailleurs qui grossissent pendant un
  dessin, avant tout document, avec un grand document puis un petit, ou
  dont le système ne dit pas la mémoire ; le plafond qui sature ; l'ordre
  de `library_candidates` pour un paquet et pour un build compilé depuis
  le dépôt. `app/tests/render_worker.rs` : la mémoire d'un vrai
  travailleur lue sans PDFium ; avec PDFium, un vrai travailleur tué
  au-delà d'un plafond abaissé à 64 Mio puis la demande suivante servie,
  et la même page dessinée sous le plafond par défaut ; sous Windows, une
  fenêtre de substitution tuée pendant que son travailleur dessine, le
  travailleur arrêté avec elle (ce test échoue si le Job n'a plus « tuer à
  la fermeture »). La CI exécute sous Linux tout ce qui n'a pas besoin de
  PDFium.
