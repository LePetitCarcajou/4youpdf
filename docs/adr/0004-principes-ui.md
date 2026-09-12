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
  pas d'une version ultérieure.
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
