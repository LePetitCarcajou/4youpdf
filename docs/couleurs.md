# Couleurs de l'interface

Les couleurs de l'application sont en tête de `app/ui/styles.css`, en deux
blocs `:root` : changer une couleur, c'est changer une ligne du premier ;
changer l'emploi d'une couleur, une ligne du second.

## Pourquoi l'ambre, et pas le rouge

Le PDF est une catégorie rouge : Acrobat, iLovePDF, Smallpdf, Foxit. En
rouge, 4YouPDF se perdrait parmi eux, jusque dans la barre des tâches. Il
prend donc un accent ambre sur des surfaces aux neutres chauds, avec un brun
très sombre autour de la page affichée : la livrée du carcajou, l'animal du
pseudo du dépôt, qui ouvre ce que les autres ne peuvent pas ouvrir. C'est la
promesse du noyau.

Le rouge ne sert qu'au danger : une ouverture qui échoue, un numéro de page
qui ne mène nulle part, le × qui supprimerait la vignette survolée, une page
que le moteur n'a pas rendue. Un fichier réparé, chiffré ou protégé par un
mot de passe, même refusé, n'est pas un danger : son bandeau est ocre, celui
de l'avertissement.

L'icône a suivi le 15 septembre 2026, après la palette :
`app/icons/icon.svg` ne porte plus le « 4Y » sur le bleu d'avant, mais une
feuille de papier ambre déchirée par trois coups de griffe, le carcajou
(`app/README.md`, « Version, signature, icônes »). Ses couleurs sont les
siennes et ne sont pas des rôles : `#bb5213` pour le papier, `#26211e` pour
le fond, à ΔE00 2,2 et 2,5 des couleurs brutes les plus proches
(`--ambre-550` `#b6540b`, `--brun-850` `#2a2723`). La règle du bloc unique,
plus bas, ne porte que sur `app/ui/styles.css`.

## Deux niveaux

1. **Les couleurs brutes**, premier bloc : `--ambre-550: #b6540b`. Chacune
   est nommée par sa teinte et son niveau, qui vaut 1000 − 10 × L* (clarté
   CIELAB) arrondi à la cinquantaine : `050` est presque blanc, `900`
   presque noir. `--blanc` et `--noir` n'ont pas de niveau.
2. **Les rôles**, second bloc : `--accent: var(--ambre-550)`. Un rôle ne
   fait que nommer une couleur brute.
3. **Les règles de mise en forme** n'emploient que les rôles. Un emploi
   translucide mélange un rôle avec `transparent` :
   `color-mix(in srgb, var(--shadow) 25%, transparent)`, que comprennent
   Chromium depuis la version 111 et Safari depuis la 16.2.

**La règle** : aucun code hexadécimal, `rgb()` ou `rgba()` hors du premier
bloc, ni nom de couleur comme `white`. Hors des deux blocs ne restent que
les mots-clés `transparent` et `currentColor`. La commande

    grep -niE "#[0-9a-f]{3,8}\b|rgba?\(" app/ui/styles.css

ne doit trouver que des lignes du premier bloc. Aucun outil ne le vérifie
encore (`docs/backlog-technique.md`).

## Rôles

| Rôle | Couleur brute | Valeur | Emploi |
|---|---|---|---|
| `--accent` | `--ambre-550` | `#b6540b` | fond du bouton principal ; lien ; bordure d'un bouton survolé ou enfoncé, de la vignette sélectionnée, du champ au focus ; vignette affichée du panneau ; marqueur d'insertion d'une vignette glissée ; cadre du dépôt de fichier |
| `--accent-strong` | `--ambre-600` | `#92400e` | texte d'accent sur `--accent-soft` : bandeau d'information, bouton enfoncé, « Déposer pour ouvrir » |
| `--accent-soft` | `--ambre-050` | `#fdf1e3` | fond du bandeau d'information, du bouton enfoncé et de l'élément survolé du menu ; anneau de la vignette sélectionnée et du champ au focus ; fond du dépôt de fichier, à 85 % |
| `--accent-ink` | `--blanc` | `#ffffff` | texte sur un fond plein : l'accent, ou le danger du × survolé d'une vignette |
| `--surface` | `--gris-050` | `#fafaf8` | fond de la fenêtre |
| `--surface-raised` | `--blanc` | `#ffffff` | barres d'outils et d'état, boutons, vignettes, champs, menu contextuel ; fond d'une page, dans sa vignette et dans la vue |
| `--surface-sunken` | `--brun-850` | `#2a2723` | fond de la vue d'une page, autour de la page |
| `--sunken-ink` | `--blanc` | `#ffffff` | texte et flèches de la vue ; à 65 % l'indication des touches, à 8 et 20 % le fond des flèches |
| `--sunken-danger` | `--rouge-150` | `#ffc9c9` | numéro de page refusé : l'explication et l'anneau du champ |
| `--text` | `--gris-900` | `#1d1d1f` | texte courant |
| `--text-muted` | `--gris-600` | `#60646c` | texte discret : nom du document, numéros des vignettes, zone vide, état du rendu, texte d'attente de la page affichée, texte indicatif du champ de mot de passe |
| `--border` | `--gris-100` | `#dcdcd7` | bordures des boutons et des vignettes, séparateurs ; texte d'attente des vignettes, discret exprès |
| `--field-border` | `--gris-450` | `#82827e` | bord des champs de texte : mot de passe, numéro de page |
| `--danger` | `--rouge-600` | `#b3261e` | texte du bandeau d'erreur, fond du × survolé d'une vignette, bord du champ refusé, page non rendue |
| `--danger-soft` | `--rouge-100` | `#f9dedc` | fond du bandeau d'erreur |
| `--success` | `--vert-550` | `#1e7b34` | aucun emploi pour l'instant |
| `--warning` | `--ocre-600` | `#8a5a00` | texte du bandeau d'avertissement : fichier réparé ou chiffré, mot de passe demandé ou refusé |
| `--warning-soft` | `--ocre-050` | `#faebb5` | fond du bandeau d'avertissement |
| `--shadow` | `--noir` | `#000000` | ombres, à 15, 25 et 50 % |

Les treize rôles de départ ne couvraient ni le texte posé sur le fond sombre
de la vue, ni les fonds des bandeaux d'avertissement et d'erreur, ni les
ombres : `--sunken-ink`, `--sunken-danger`, `--warning-soft`,
`--danger-soft` et `--shadow` s'y sont ajoutés, puis `--field-border` pour
les contrastes (plus bas).

L'accent proposé au départ, `#b45309`, est devenu `#b6540b` pour atteindre
3 : 1 sur le fond de la vue, où il borde le bouton « Vignettes » enfoncé ;
l'écart entre les deux, ΔE00 0,5, est sous le seuil de perception.
`--warning` est resté `#8a5a00` bien qu'il soit voisin de l'accent (ΔE00
13,8 ; 2,1 en protanopie simulée) : à l'écran, l'ocre est un texte sur fond
jaune pâle et l'accent un fond plein, et un ocre plus éloigné en vision
normale ne s'écarterait pas plus de l'accent en protanopie tout en se
rapprochant du texte du bandeau d'information.

## Thème sombre

Il n'est pas écrit, mais la structure le permet. Ses couleurs brutes
s'ajouteraient au premier bloc, et un seul bloc, placé après le second,
réassignerait les rôles sans toucher à une règle de mise en forme :

    @media (prefers-color-scheme: dark) {
      :root {
        color-scheme: dark;
        --surface: var(--…);
        …
      }
    }

Vérifié le 15 septembre 2026 dans l'application, build de développement,
WebView2 152.0.4191.66, par DevTools. Dans 22 états de la fenêtre (grille,
sélection, menu contextuel, vue d'une page avec et sans panneau, numéro
refusé, bandeaux, mot de passe demandé puis refusé, dépôt de fichier), un
tel bloc donnant à chaque rôle une couleur témoin, sous une préférence
sombre simulée, n'a déplacé aucune boîte ni changé une propriété calculée
autre qu'une couleur, et chaque couleur que la feuille dessine l'a suivi.

Restent au moteur web, hors des rôles :

- l'anneau de focus par défaut, mesuré sur le champ de mot de passe, et que
  Chromium donne aussi à un bouton atteint au clavier, la feuille n'en
  dessinant aucun : sa couleur calculée, `#101010`, ne suit ni les rôles ni
  `color-scheme: dark`. Un thème sombre demandera d'abord un anneau dessiné
  par la feuille (`docs/backlog-ui.md`) ;
- la sélection de texte dans les champs et les barres de défilement, que
  les styles calculés ne montrent pas.

À trancher ce jour-là : `--surface-raised` est aussi le fond d'une page tant
que son image n'est pas arrivée, et `--sunken-ink` doit rester clair tant que
le fond de la vue reste sombre.

## Contrastes

Calculés le 15 septembre 2026 selon WCAG 2.1 : 4,5 : 1 pour du texte, 3 : 1
pour un contour ou une icône porteuse de sens. Le texte courant est à
16,82 : 1 sur `--surface-raised`, le texte discret à 5,68 : 1 au plus bas, le
texte sur l'accent à 4,92 : 1, le texte des bandeaux à 4,97 : 1 au plus bas
(ocre sur jaune). Le bord d'un champ de texte est à 3,85 : 1 sur blanc et à
3,23 : 1 dans le bandeau du mot de passe ; l'accent, à 3,01 : 1 sur le fond
de la vue. Les paires encore sous le seuil, qui l'étaient toutes déjà avec
l'ancienne palette, sont au backlog (`docs/backlog-ui.md`, « Contrastes sous
les seuils de WCAG 2.1 »).
