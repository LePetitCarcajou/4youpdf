# Sécurité

## Signaler une vulnérabilité

N'ouvrez pas d'issue publique. Le canal privé n'est pas encore en place :
`MAINTAINERS.md` n'indique pas encore d'adresse, et le signalement privé de
GitHub n'est pas activé sur le dépôt (`docs/backlog-technique.md`). Un
signalement devra inclure : description, fichier PDF de reproduction si
possible, impact.

Nous accusons réception sous 7 jours et publions un correctif de façon
coordonnée avec vous.

## Ce que le projet garantit

- Le noyau est en Rust sans `unsafe` : un PDF malformé peut échouer, jamais
  planter ni corrompre la mémoire. Le fuzzing de la CI n'en couvre encore
  qu'une partie, le parseur d'objets et la lecture rapide de l'en-tête et de
  la fin du fichier (`docs/backlog-technique.md`).
- Un module tiers s'exécute en WebAssembly sans accès au disque, au réseau
  ni aux autres modules, sauf permission déclarée dans son manifeste et
  accordée par l'utilisateur.
- Aucune télémétrie, aucune connexion sortante sans action explicite.
- Les modules du catalogue seront signés et leur code source public : ni le
  catalogue ni la signature n'existent encore (ADR 0003, « Limites connues »).

Voir `docs/adr/0003-securite-modules.md` pour le modèle complet.
