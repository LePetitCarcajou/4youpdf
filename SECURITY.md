# Sécurité

## Signaler une vulnérabilité

N'ouvrez pas d'issue publique. Utilisez le signalement privé de GitHub :
« Report a vulnerability » dans l'onglet Security du dépôt
(<https://github.com/LePetitCarcajou/4youpdf/security/advisories/new>). Le
rapport n'est lisible que des mainteneurs ; le correctif est préparé puis
publié avec vous, dans un avis de sécurité qui vous crédite. Indiquez :
description, fichier PDF de reproduction si possible, impact.

Nous accusons réception sous 7 jours et publions un correctif de façon
coordonnée avec vous.

## Ce que le projet garantit

- Le noyau est en Rust sans `unsafe` : un PDF malformé peut échouer, jamais
  planter ni corrompre la mémoire. Chaque nuit, la CI fuzze l'ouverture
  complète d'un document, les filtres, le parseur d'objets, la lecture
  rapide de l'en-tête, le contrat des modules et les fonctions WASI de
  l'hôte (`fuzz/README.md`) ; ce qui reste hors de portée est dans
  `docs/backlog-technique.md`.
- Un module tiers s'exécute en WebAssembly sans accès au disque, au réseau
  ni aux autres modules, sauf permission déclarée dans son manifeste et
  accordée par l'utilisateur.
- Aucune télémétrie, aucune connexion sortante sans action explicite.
- Les modules du catalogue seront signés et leur code source public : ni le
  catalogue ni la signature n'existent encore (ADR 0003, « Limites connues »).

Voir `docs/adr/0003-securite-modules.md` pour le modèle complet.
