# ADR 0004 — Les outils viennent au document

**Statut** : accepté — 2026-09

## Contexte
Les suites PDF existantes (PDF24, iLovePDF, Stirling-PDF) séparent la
visualisation et les outils. PDF24 ouvre sa Toolbox dans une seconde fenêtre
par-dessus le lecteur, avec une grille d'une vingtaine de tuiles visuellement
identiques :

- on perd le contexte : quel fichier, quelle page, quelle sélection ;
- il faut lire chaque libellé pour trouver l'outil voulu ;
- rien n'indique ce qui s'applique réellement au document ouvert.

À l'inverse, la meilleure interaction de PDF24 est son menu contextuel sur
une sélection de texte : les actions pertinentes apparaissent là où
l'utilisateur regarde déjà.

## Décision
1. **Une seule fenêtre.** Aucune fenêtre secondaire ni boîte modale pour une
   opération courante. Les options d'un outil s'affichent dans un panneau
   latéral ou en ligne, sans masquer le document.
2. **Actions contextuelles** : menu sur une sélection de texte, menu sur une
   vignette de page ou une sélection de pages, actions de correction
   directement dans le panneau de conformité.
3. **Palette de commandes (Ctrl+K)**, point d'entrée principal pour les
   utilisateurs réguliers :
   - recherche floue : « mrg » trouve Fusionner, « pdfa » trouve Convertir
     en PDF/A ;
   - bilingue : le libellé français et le libellé anglais mènent au même
     outil ;
   - résultats classés selon le contexte (sélection en cours, écarts de
     conformité détectés) et selon la fréquence d'usage ;
   - chaque résultat affiche son raccourci clavier et une ligne
     d'explication.
4. **Barre latérale réduite aux actions courantes**, porte d'entrée pour les
   nouveaux utilisateurs ; le reste passe par la palette. Pas de grille de
   tuiles.
5. **Actions déclarées par les modules** : la palette et la barre latérale
   sont alimentées automatiquement par le champ `actions` des manifestes
   (`id`, `label`, `category`, voir `docs/plugin-manifest.md`). Ajouter un
   module n'exige aucune modification de l'interface.
6. **Aperçu de l'effet avant application.** Tout réglage montre ce qu'il
   produira avant d'être appliqué. Un champ numérique seul (« largeur de
   brosse : 32 », « qualité : 75 ») oblige à l'essai-erreur : la valeur
   s'accompagne d'une prévisualisation en direct, et le curseur ou la
   sélection reflète le réglage réel sur le document.
7. **Une liste unique de réglages, avec recherche.** Pas un onglet par
   composant interne du logiciel : l'utilisateur cherche « langue » ou
   « dossier de sortie », il ne sait pas quel composant les détient.
8. **Plusieurs documents ouverts dans la même fenêtre.** Les opérations
   multi-documents (fusion, comparaison) portent sur les documents déjà
   ouverts plutôt que sur des fichiers à repiquer sur le disque.

## Conséquences
- **La palette devient un composant critique.** Sa qualité de recherche
  conditionne l'accès à toute fonctionnalité absente de la barre latérale :
  une action introuvable est, pour l'utilisateur, une action qui n'existe
  pas. Elle se teste comme le noyau, avec un jeu de requêtes et le résultat
  attendu en tête : abréviations, fautes de frappe, mots sans accents
  (« securite » trouve Sécurité), termes anglais dans l'interface française.
  Elle doit répondre à chaque frappe sans latence perceptible.
- **Traduction dès la première version de l'interface.** Les libellés et
  explications, ceux de l'interface comme ceux des modules, existent au
  moins en français et en anglais, et la recherche indexe les deux langues.
  L'infrastructure de traduction (fichiers de messages, règle de repli
  quand une traduction manque) fait partie du premier jalon de l'interface,
  pas d'une version ultérieure. Celui-ci est pourtant sorti en français
  seul : depuis le 19 septembre 2026, la feuille de route place la
  traduction anglaise de l'interface au bloc E
  (`docs/feuille-de-route.md`).
- **Le manifeste doit évoluer.** Le champ `actions` ne porte aujourd'hui
  qu'un `label` dans une seule langue. Il lui faut des libellés traduits,
  la ligne d'explication, et de quoi savoir à quel contexte une action
  s'applique (sélection de texte, pages, document entier) pour le menu
  contextuel et le classement. Ce changement de `fyp-plugin-api` suit le
  versionnage séparé de l'ADR 0002.
- **Raccourcis attribués par l'hôte.** Un module ne choisit pas son
  raccourci : il pourrait détourner une combinaison existante. L'hôte les
  attribue, résout les conflits et laisse l'utilisateur les modifier.
- **Historique d'usage strictement local.** Le classement par fréquence
  suppose de stocker quelles actions sont utilisées et quand. Cet historique
  reste dans le profil de l'utilisateur, ne quitte jamais la machine (ni
  télémétrie, ni synchronisation) et n'est accessible à aucun module : aucune
  permission de l'ADR 0003 n'y donne accès. L'utilisateur doit pouvoir
  l'effacer.
- **Un module mal décrit devient introuvable.** Sans tuile ni icône pour
  compenser, le libellé, l'explication et la catégorie sont toute la
  surface d'un module. La documentation du manifeste doit insister sur les
  libellés : un verbe d'action, les mots que l'utilisateur tape réellement,
  jamais le nom interne du module ou d'un format, avec des exemples bons et
  mauvais.
- **Pas de modale pour les confirmations courantes, mais un arrêt explicite
  pour l'irréversible.** Une confirmation courante s'affiche en ligne ou
  dans le panneau latéral, à côté du document concerné. Une action
  irréversible, comme la suppression du fichier source ou l'octroi d'une
  permission réseau à un module (ADR 0003), ne doit jamais pouvoir être
  validée par inattention. Le panneau latéral convient à condition qu'il
  exige une confirmation active : la demande ne disparaît pas quand on
  clique ailleurs, et l'action n'est lancée que par le bouton qui la nomme.
  Entrée pressée par réflexe, clic en dehors du panneau ou fermeture du
  panneau ne valent jamais accord.
- **L'aperçu emprunte le chemin de l'application réelle.** Une
  prévisualisation qui approxime l'effet ment, et ramène l'essai-erreur
  qu'elle devait supprimer. Elle exécute le même code que l'application,
  sur une copie en mémoire, restreinte à ce qui est visible (la page
  affichée, une vignette) pour tenir le temps réel. Quand seule une
  estimation est possible, comme la taille finale d'un fichier compressé à
  « qualité : 75 », l'interface le dit. Chaque nouvelle valeur annule le
  calcul précédent encore en cours, et les limites de ressources de
  l'ADR 0003 s'appliquent à l'aperçu comme à l'exécution.
- **Le rendu de page devient un prérequis de l'interface.** Il n'y a pas
  d'aperçu sans page affichée. Le module de rendu, officiel et natif selon
  l'ADR 0003, doit être disponible dès le jalon 0.3, et assez rapide pour
  redessiner une page à chaque réglage modifié, pas seulement à l'ouverture.
- **Les modules décrivent leurs paramètres, l'hôte dessine les contrôles.**
  Pour qu'un réglage de module ait son aperçu et entre dans la liste
  unique, le manifeste déclare chaque paramètre : type, bornes, unité,
  valeur par défaut, libellé et explication traduits, mots-clés de
  recherche. L'hôte construit le contrôle, affiche l'aperçu et range le
  réglage ; un module ne fournit pas d'écran de réglages à lui. Le contrat
  distingue « calculer un aperçu » d'« appliquer » : calculer un aperçu ne
  vaut pas exécution et ne déclenche jamais la confirmation d'une
  permission sensible, si bien qu'un module qui a besoin du réseau pour
  calculer son effet n'a pas d'aperçu en direct tant que la permission
  n'est pas accordée. Cette évolution de `fyp-plugin-api` s'ajoute à celle
  du champ `actions` décrite plus haut et suit le même versionnage séparé.
- **Les réglages forment un registre, pas des écrans.** Chaque réglage, de
  l'application comme d'un module, est déclaré une fois avec un
  identifiant stable ; la liste, la recherche et le stockage en découlent.
  La recherche réutilise le moteur de la palette et ses exigences (fautes
  de frappe, mots sans accents, deux langues), et la palette trouve aussi
  les réglages : « dossier de sortie » tapé dans Ctrl+K y mène directement.
  Les regroupements affichés suivent le sens (document, sortie, apparence,
  sécurité), jamais le composant ; le module qui fournit un réglage
  n'apparaît qu'en information secondaire, pour savoir lequel désactiver.
- **Un module ne lit que ses propres réglages.** La liste est unique à
  l'écran, pas dans les droits. Les identifiants sont préfixés par celui
  du module (`org.fouryoupdf.merge.…`), ce qui écarte les collisions, et un
  module n'accède ni aux réglages des autres ni à ceux de l'application,
  sauf ceux que l'hôte expose explicitement à tous, comme la langue de
  l'interface. Le « dossier de sortie » est un réglage de l'hôte : c'est
  l'hôte qui écrit le document revalidé (ADR 0003), et ce réglage ne donne
  à aucun module la permission `write_dir` sur ce dossier.
- **Le noyau garde plusieurs documents vivants à la fois.** `Document`
  emprunte les octets du fichier : chaque document ouvert garde son
  fichier entier en mémoire, et dix fichiers de 200 Mio en occupent près
  de 2 Gio avant tout rendu. `unsafe` étant interdit, la projection du
  fichier en mémoire (mmap) n'est pas une issue. Il faut, avant le
  jalon 0.3, une source d'octets qui lit le fichier par blocs à la demande,
  et un budget mémoire global de l'hôte qui libère les caches (object
  streams décodés, pages rendues) des documents qui ne sont pas à l'écran.
  Le jalon 0.3, l'application, est passé sans l'un ni l'autre, et depuis le
  19 septembre 2026 la feuille de route ne leur donne pas de jalon
  (`docs/feuille-de-route.md` ; `docs/backlog-technique.md`).
- **Les opérations reçoivent des documents, pas des chemins.** C'est déjà
  la forme du noyau (`ops::merge` prend des `Document` ouverts) et ce doit
  être celle du contrat des modules : l'hôte transmet au module les
  documents choisis parmi ceux qui sont ouverts. Un module de fusion ou de
  comparaison n'a alors besoin ni de `read_dir` ni d'aucun accès au disque,
  ce qui renforce l'ADR 0003. L'outil affiche en permanence sur quels
  documents il porte et dans quel ordre, réordonnable à la souris ; la
  palette classe Fusionner et Comparer plus haut dès que le nombre de
  documents ouverts atteint le `min_inputs` de l'action. La comparaison
  suppose deux documents visibles ensemble : la mise en page prévoit une
  vue partagée dans la fenêtre unique dès la première version.
- **Chaque document ouvert a son propre état.** Historique d'annulation et
  modifications non enregistrées sont tenus par document. Fermer la
  fenêtre alors que plusieurs documents sont modifiés passe par un arrêt
  explicite qui les liste, comme toute action irréversible. Ouvrir un
  fichier déjà ouvert ramène celui-ci au premier plan au lieu d'en créer
  une seconde copie qui divergerait.
