# Sécurité

## Signaler une vulnérabilité

N'ouvrez pas d'issue publique. Écrivez à l'adresse indiquée dans le fichier
`MAINTAINERS.md` (ou utilisez le signalement privé GitHub si activé).
Incluez : description, fichier PDF de reproduction si possible, impact.

Nous accusons réception sous 7 jours et publions un correctif de façon
coordonnée avec vous.

## Ce que le projet garantit

- Le noyau est en Rust sans `unsafe` et fuzzé en continu : un PDF malformé
  peut échouer, jamais planter ni corrompre la mémoire.
- Un module tiers s'exécute en WebAssembly sans accès au disque, au réseau
  ni aux autres modules, sauf permission déclarée dans son manifeste et
  accordée par l'utilisateur.
- Aucune télémétrie, aucune connexion sortante sans action explicite.
- Les modules du catalogue sont signés et leur code source est public.

Voir `docs/adr/0003-securite-modules.md` pour le modèle complet.
