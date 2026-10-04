# ADR 0008 — Le rendu dans un processus à part

**Statut** : accepté — 2026-10 ; remplace le point 4 de l'ADR 0005 (un
thread pour PDFium) et précise son point 2 (où vit `pdfium-render`)

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
| Réponse qui n'est pas celle de la demande, trame hors plafond ou mal formée | le travailleur est tué, puis comme ci-dessus | « le moteur de rendu a donné une réponse inattendue (…) ; il a été arrêté et sera relancé » |
| La même page fait tomber le travailleur deux fois | elle est refusée ensuite sans qu'un travailleur soit lancé pour elle, jusqu'à ce qu'une demande concerne un autre document | « la page N a arrêté le moteur de rendu deux fois ; elle n'est plus dessinée » |
| L'ouverture d'un document fait tomber le travailleur deux fois | toutes ses pages sont refusées de même | « ce document a arrêté le moteur de rendu deux fois à l'ouverture ; ses pages ne sont plus dessinées » |
| Plus de huit relances en une minute | plus aucun travailleur n'est lancé de la session ; `renderer_status` répond « indisponible » | la raison sur chaque page demandée, et dans la barre d'état dès qu'une page échoue |
| PDFium refuse le document ou la page | réponse `Failed` ; le travailleur reste | la raison donnée par le travailleur, les pages citées à partir de 1 |
| La fenêtre se ferme | l'entrée du travailleur est fermée, il est attendu deux secondes, tué sinon ; une demande en cours échoue aussitôt, son travailleur tué, et aucun travailleur n'est plus lancé ; une demande surprise pendant une poignée de main échoue à la fin de celle-ci, 10 s au plus | rien |
| La fenêtre disparaît (plantage, tuée) | le système ferme l'entrée du travailleur, qui s'arrête à la fin du dessin en cours s'il y en a un, aussitôt sinon (« Limites connues ») | rien |
| Le premier travailleur ne démarre pas, meurt ou se tait avant la poignée de main | le rendu reste « disponible » avec la raison en détail ; un travailleur est lancé à la première page demandée, et cette relance compte | « Aperçus : PDFium » dans la barre d'état, dont l'infobulle garde la raison du premier échec même après une relance réussie, l'interface ne relisant l'état qu'après une page qui échoue (`docs/backlog-ui.md`) ; si la relance échoue, la raison sur la page demandée |
| Le travailleur n'a pas de PDFium, ou parle une autre version | aucun autre n'est lancé de la session | « Aperçus indisponibles » et la raison dans la barre d'état, dès le démarrage |

« Tomber » couvre les trois premières pannes : mort, silence, mensonge.
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

Les plafonds du protocole sont des constantes de `protocol.rs` ; les délais
et le nombre de relances, `Limits::default()` dans `render.rs` ; les deux
chutes, `FALLS`.

## Limites connues
- **Ce n'est pas encore un bac à sable.** Le travailleur garde les droits
  de l'utilisateur : un défaut de PDFium exploité y lit et écrit les
  fichiers de l'utilisateur et peut atteindre le processus de la fenêtre
  par les moyens du système. L'isolation protège aujourd'hui du plantage,
  du blocage et des réponses fausses, pas d'un code hostile qui s'exécute.
  Restreindre ses droits est au backlog, avec sa condition
  (`docs/backlog-technique.md`).
- **Mémoire non bornée**, jusqu'à la session B du palier v0.5.1 : le
  travailleur dessine l'image avant de la comparer aux plafonds, et PDFium
  alloue ce que le fichier lui fait allouer. Une mémoire épuisée tue le
  travailleur, ce que la fenêtre sait traiter, mais peut peser sur la
  machine avant.
- **Un travailleur peut survivre à la fenêtre, le temps du dessin en
  cours** : il ne s'arrête qu'en lisant la fin de son entrée, ce qu'il ne
  fait qu'entre deux demandes. Oisif, il s'arrête aussitôt. Pendant un
  dessin sans fin, il n'a pas de borne : la fenêtre n'est plus là pour
  appliquer le délai de 30 s. Le testeur de la session l'a mesuré encore
  vivant 90 s après la fermeture de son entrée, sur une page de 3,4 Kio
  faite de formulaires imbriqués (10⁶ remplissages), et 11 s après la mort
  de la fenêtre réelle pour la vignette de cette page. L'attacher à la
  fenêtre par un Job Object est la session B.
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
  1400 pixels de large, 95 Mio à 4096. La mesure avant et après, et son
  seuil de 15 %, sont la session B.
- **Deux copies du document**, comme avant (ADR 0005), mais dans deux
  processus : celle du noyau dans la fenêtre, celle de PDFium dans le
  travailleur, plus, le temps de l'ouverture, la trame reçue.
- **Très hautes pages.** Une page plus de deux fois plus haute que large
  n'est plus dessinée à la largeur maximale, ni une page dont l'image
  dépasserait 16 384 pixels de haut : la vue dit pourquoi. Avant, PDFium
  tentait un bitmap de la taille demandée, quelle qu'elle soit.

## Conséquences
- Tuer le processus de rendu depuis le Gestionnaire des tâches ne ferme
  pas la fenêtre, et le rendu reprend à la page suivante.
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
  fenêtre.
