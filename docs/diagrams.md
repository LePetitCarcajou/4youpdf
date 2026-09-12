# Diagrammes

> **Règle de maintenance.** Ces diagrammes ne montrent que ce qui change
> lentement : couches, étapes, rôles, relations. Jamais de nom de méthode,
> de signature ni de champ précis. Un diagramme qui citerait `foo()` serait
> faux au premier renommage ; un diagramme qui dit « le noyau vérifie la
> table déclarée avant de la croire » reste vrai tant que le principe tient.
> Quand un principe change, on met le diagramme à jour ; quand un nom
> change, on ne touche à rien. Les détails vivent dans `architecture.md` et
> dans la documentation du code.

## 1. Couches et dépendances autorisées

Ce diagramme répond à la question « qui a le droit de dépendre de qui ? ».
C'est la vue d'ensemble de `architecture.md`, avec les flèches de dépendance
telles que les manifestes Cargo les déclarent. Toutes vont vers le bas : le
noyau ne connaît ni l'hôte, ni les modules, ni l'interface ; le contrat des
modules ne connaît pas le noyau ; un module tiers ne voit que le contrat.
Avant d'ajouter une dépendance entre deux crates, vérifier qu'une flèche
existe ici dans ce sens.

```mermaid
flowchart TB
    subgraph interfaces["Interfaces"]
        app["Application desktop (Tauri)"]
        cli["Ligne de commande"]
    end
    subgraph hote["Hôte"]
        host["fyp-host<br/>découverte, sandbox WebAssembly, permissions"]
    end
    subgraph contrat["Contrat, versionné à part"]
        api["fyp-plugin-api<br/>manifeste, traits, types d'échange"]
    end
    subgraph services["Services"]
        conformance["fyp-conformance"]
    end
    subgraph noyau["Noyau"]
        core["fyp-core<br/>lexique, objets, xref, filtres, récupération, déchiffrement, écriture"]
    end
    subgraph primitives["Primitives"]
        crypto["fyp-crypto<br/>handler de sécurité standard : RC4, AES, dérivation de clé"]
    end
    plugins["Modules tiers<br/>WebAssembly, jamais natifs"]

    app --> host
    cli --> host
    app --> core
    cli --> core
    host --> api
    host --> core
    conformance --> core
    core --> crypto
    plugins --> api
```

Le contrat (`fyp-plugin-api`) n'a aucune flèche sortante vers le reste du
projet : c'est ce qui permet de le publier et de le versionner seul.

## 2. Parcours d'un fichier à l'ouverture

Ce diagramme répond à la question « que se passe-t-il entre un tableau
d'octets et un nombre de pages, et où le noyau peut-il échouer ou se
rattraper ? ». Le principe : on essaie d'abord le chemin déclaré par le
fichier, on le vérifie, et l'on ne passe à la reconstruction par scan qu'en
recours. Un fichier réparé reste marqué comme tel jusqu'au bout. Les
losanges sont les points de décision, les nœuds rouges les échecs
définitifs, le nœud orange la récupération.

```mermaid
flowchart TD
    input["Octets du fichier"] --> quick{"En-tête %PDF<br/>trouvé ?"}
    quick -- non --> badheader["Échec : pas un PDF"]
    quick -- oui --> startxref{"startxref<br/>présent ?"}
    startxref -- non --> scan
    startxref -- oui --> section["Lecture de la section xref<br/>table classique, flux xref ou hybride"]
    section --> prev{"Section lisible,<br/>chaîne des mises à jour<br/>sans boucle ?"}
    prev -- non --> scan
    prev -- oui --> verify["Vérification : chaque entrée mène<br/>bien à l'objet annoncé,<br/>le trailer a une racine"]
    verify --> ok{"Table<br/>conforme au fichier ?"}
    ok -- non --> scan
    ok -- oui --> doc["Document ouvert,<br/>table déclarée"]
    scan["Reconstruction par scan :<br/>recherche des en-têtes d'objets,<br/>la dernière définition gagne,<br/>object streams ouverts,<br/>trailer reconstitué"]
    scan --> found{"Au moins un<br/>objet trouvé ?"}
    found -- non --> unrecoverable["Échec : irrécupérable,<br/>avec la cause du rejet de la table"]
    found -- oui --> repaired["Document ouvert,<br/>marqué « reconstruit »<br/>avec la cause"]

    doc --> get
    repaired --> get
    get["Lecture d'un objet"] --> kind{"Où la table<br/>le place-t-elle ?"}
    kind -- "à un offset" --> direct["Parsing à l'offset,<br/>numéro et génération vérifiés"]
    kind -- "dans un object stream" --> objstm["Décodage du conteneur (mis en cache),<br/>parsing de l'objet dedans"]
    kind -- "absent ou génération différente" --> nullobj["Objet nul"]
    direct --> catalog
    objstm --> catalog
    catalog["Trailer → racine → catalogue"] --> pages["Arbre de pages → nombre de pages"]
    catalog -- "racine absente ou<br/>pas un dictionnaire" --> badstructure["Échec : structure invalide"]

    classDef fail fill:#f8d7da,stroke:#b02a37,color:#000
    classDef recover fill:#ffe5b4,stroke:#b36b00,color:#000
    class badheader,unrecoverable,badstructure fail
    class scan,repaired recover
```

Deux garde-fous ne figurent pas comme étapes mais s'appliquent partout :
tout décodage de flux est plafonné (protection contre les bombes de
décompression) et le scan a un budget de travail linéaire en la taille du
fichier.

## 3. Le modèle objet et la structure logique d'un document

Ce diagramme répond à la question « comment les briques syntaxiques
(ISO 32000-2, 7.3) forment-elles un document (7.7) ? ». À gauche, les dix
sortes d'objets directs et leurs relations de contenance : un tableau et un
dictionnaire contiennent des objets, un flux est un dictionnaire plus des
octets encodés, une référence désigne un objet indirect par numéro et
génération. À droite, le chemin logique que le noyau suit pour passer du
trailer aux pages ; chaque flèche « désigne » est, dans le fichier, une
référence. Le même modèle sert à la lecture et à l'écriture : le writer ne
fait que sérialiser ces briques.

```mermaid
flowchart LR
    subgraph syntaxe["Objets directs (syntaxe)"]
        obj(["Objet"])
        scalars["Nul · booléen · entier · réel<br/>chaîne · nom"]
        array["Tableau"]
        dict["Dictionnaire<br/>clés = noms"]
        stream["Flux = dictionnaire<br/>+ octets encodés"]
        reference["Référence<br/>numéro + génération"]
        obj --- scalars
        obj --- array
        obj --- dict
        obj --- stream
        obj --- reference
        array -- "contient" --> obj
        dict -- "contient" --> obj
        stream -- "est décrit par" --> dict
        reference -- "désigne un objet indirect<br/>via la table xref" --> obj
    end

    subgraph logique["Structure logique (document)"]
        trailer["Trailer<br/>dictionnaire"]
        root["Catalogue<br/>dictionnaire, /Type /Catalog"]
        pagetree["Racine de l'arbre de pages<br/>/Type /Pages, compte total"]
        node["Nœud intermédiaire<br/>/Type /Pages"]
        page["Page<br/>/Type /Page, boîte, ressources"]
        content["Contenu de page<br/>flux d'opérateurs graphiques"]
        resources["Ressources<br/>polices, images, états graphiques"]
        info["Informations<br/>titre, producteur, dates"]
        trailer -- "désigne (/Root)" --> root
        trailer -- "désigne (/Info)" --> info
        root -- "désigne" --> pagetree
        pagetree -- "enfants" --> node
        pagetree -- "enfants" --> page
        node -- "enfants" --> page
        page -- "désigne" --> content
        page -- "désigne" --> resources
    end

    dict -.-> trailer
    dict -.-> root
    stream -.-> content
```

Les objets indirects peuvent être stockés à un offset du fichier ou
compressés dans un object stream ; cette différence de stockage n'apparaît
pas dans le modèle : c'est la table xref qui la connaît, et le writer la
gomme en réécrivant tout au premier niveau.

## 4. Cycle de vie d'un module

Ce diagramme répond à la question « par quelles portes passe un module
tiers avant que son résultat soit accepté ? ». Il met en ordre les six
étapes de l'ADR 0003 : découverte, parsing, validation statique,
présentation des permissions, exécution sous sandbox, re-validation. Un
module hostile est l'hypothèse par défaut : chaque étape peut le rejeter, et
le document source n'est jamais modifié en place. La découverte et la
validation existent dans l'hôte ; l'exécution sandboxée et la re-validation
sont le programme du jalon 0.2.

```mermaid
flowchart TD
    disk["Dossier de modules sur disque<br/>un sous-dossier par module"] --> find["Découverte :<br/>chaque sous-dossier avec un manifeste"]
    find --> parse{"Manifeste<br/>lisible ?"}
    parse -- non --> rejected["Refusé, avec la raison<br/>montrée à l'utilisateur"]
    parse -- oui --> validate{"Validation statique :<br/>version d'API compatible,<br/>runtime autorisé ici,<br/>permissions bien formées,<br/>signature si exigée"}
    validate -- non --> rejected
    validate -- oui --> listed["Module proposé dans l'interface<br/>(palette, barre latérale)"]
    listed --> ask["Première exécution :<br/>permissions présentées,<br/>les sensibles en évidence,<br/>confirmation active"]
    ask -- refus --> stop["Non exécuté"]
    ask -- accord --> run["Exécution dans la sandbox WebAssembly<br/>sur une copie du document,<br/>capacités accordées seulement,<br/>limites de temps, mémoire, taille"]
    run -- "limite dépassée<br/>ou erreur" --> abort["Arrêt propre,<br/>document source intact"]
    run -- "document renvoyé" --> revalidate{"Re-validation par le noyau :<br/>le résultat est-il un PDF<br/>que l'on sait relire ?"}
    revalidate -- non --> abort
    revalidate -- oui --> replace["Remplacement atomique<br/>du document"]

    classDef fail fill:#f8d7da,stroke:#b02a37,color:#000
    class rejected,stop,abort fail
```

Un module natif suit le même chemin sauf la sandbox : il n'est accepté que
depuis le dépôt, revu, et l'hôte refuse tout manifeste `native` venu
d'ailleurs.
