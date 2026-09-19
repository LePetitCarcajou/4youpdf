# ADR 0007 — Durcissement de la WebView

**Statut** : accepté — 2026-09 ; précise l'ADR 0006 (CSP) et l'ADR 0004
(menu contextuel des vignettes)

## Contexte
L'application desktop est une page web dans WebView2 : ce que la page peut
charger, ce qu'elle peut demander à Rust et ce qu'un outil extérieur peut
lui faire faire sont sa surface d'attaque. État au 19 septembre 2026 :

- **Une CSP existait**, posée avec l'ADR 0006 dans `app/tauri.conf.json`
  (`default-src 'self'; img-src 'self' data:; style-src 'self'
  'unsafe-inline'; script-src 'self'`), sans test, avec `'unsafe-inline'`
  alors qu'aucun style inline n'existe dans `app/ui/`, sans `base-uri` ni
  `form-action`, qui ne retombent pas sur `default-src`, et sans
  `connect-src` : l'IPC de Tauri, qui passe d'abord par
  `fetch("http://ipc.localhost/<commande>")`, était refusé à chaque
  lancement (« Connecting to 'http://ipc.localhost/renderer_status'
  violates … default-src 'self' », mesuré par DevTools sur le build de
  développement) et Tauri se rabattait sur `postMessage` avec un
  avertissement console (« IPC custom protocol failed, Tauri will now use
  the postMessage interface instead »).
- **Les permissions étaient celles par défaut**, `core:default` et
  `dialog:default` dans `app/capabilities/default.json`, alors que
  l'interface (`app/ui/src/api.ts`) n'appelle que ses 13 commandes,
  `event.listen` et `setTitle`, déjà refusé ; sans manifeste de
  permissions de l'application (pas de dossier `app/permissions/`), Tauri 2
  autorisait d'office toutes les commandes de `main.rs` à la page locale.
- **Les outils de développement étaient absents par construction**
  (`app/Cargo.toml` : `tauri = { version = "2", features = [] }`, aucun
  `open_devtools` dans `app/src/`), mais rien ne le vérifiait, et
  `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9451`
  ouvrait bien le port de débogage sur le build release :
  `http://127.0.0.1:9451/json/version` répondait « Edg/153.0.4234.48 ».
- **Le menu natif de WebView2 s'ouvrait** hors des vignettes (« Retour »,
  « Actualiser », « Enregistrer sous », « Imprimer », « Outils
  supplémentaires », « Inspecter » en dev) : `main.ts` n'empêche
  `contextmenu` que sur la grille, pour montrer son propre menu.

## Décision
Quatre mesures, prises le 19 septembre 2026, chacune tenue par un test de
`app/src/main.rs` (voir « Vérification »).

### A. CSP stricte
Dans `app/tauri.conf.json`, `app.security` : `freezePrototype: true`, et la
CSP écrite comme une table, une directive par ligne (`Csp::DirectiveMap`) :

| Directive | Sources | Pourquoi |
|---|---|---|
| `default-src` | `'none'` | Rien par défaut ; chaque besoin a sa directive. Couvre polices, cadres, médias, objets et manifeste : l'interface n'en charge aucun (pas de `@font-face`, pas de `url()` dans `styles.css`) ; les workers, que l'interface ne crée pas non plus, relèvent de `script-src` (repli `worker-src` → `child-src` → `script-src`). |
| `script-src` | `'self'` | `main.js` seul, servi par Tauri sur l'origine de l'application. Ni `'unsafe-eval'` ni `'unsafe-inline'` : `app/ui/` n'emploie ni `eval`, ni `new Function`, ni `innerHTML`. Tauri y ajoute à l'exécution le hash SHA-256 de `main.js`, calculé à la compilation pour chaque fichier `.js` embarqué (tauri-codegen, `CspHashes::add_if_applicable`), sans effet sur un script externe que `'self'` admet déjà ; ses propres scripts sont des scripts d'initialisation (`AddScriptToExecuteOnDocumentCreated`), hors CSP. |
| `style-src` | `'self'` | `styles.css` seul. `'unsafe-inline'` retiré : l'interface ne pose que des propriétés CSSOM (`menu.style.left = …`, `frame.style.aspectRatio = …`), que la CSP n'interdit pas ; aucun attribut `style=`, aucun `<style>`. La seule exception du dépôt était la page d'attente de `app/build.rs` (interface non compilée), passée d'un attribut `style=` à un élément `<style>`, auquel Tauri donne un nonce que la CSP admet. |
| `img-src` | `'self' data:` | Vignettes et vue d'une page : URL `data:image/png;base64,…` renvoyées par la commande `render_page` (`img.src = url`, `thumbnails.ts`, `viewer.ts`). |
| `connect-src` | `ipc: http://ipc.localhost` | L'IPC de Tauri 2 : `fetch` vers `http://ipc.localhost/<commande>` sous Windows et Android, `ipc://localhost` ailleurs, servi dans le processus, jamais par le réseau. Rien d'autre : l'interface n'a ni `fetch`, ni `XMLHttpRequest`, ni `WebSocket`. |
| `base-uri` | `'none'` | Ne retombe pas sur `default-src` ; aucun `<base>` ne peut détourner les URL relatives (`main.js`, `styles.css`). |
| `form-action` | `'none'` | Ne retombe pas sur `default-src`. Les deux formulaires, le numéro de page de la vue (`#viewer-number-form`) et le mot de passe d'un avis (`passwordForm`, `main.ts`), sont traités par `submit` + `preventDefault` ; sans cette prévention, une soumission rechargerait la page et perdrait le travail. |

Retenu avec elle : `withGlobalTauri: true`, parce que `ui/src/api.ts` lit
`window.__TAURI__` (aucun Node.js ni npm dans le projet), que l'API globale
n'est qu'une façade dont les capabilities décident, et que la CSP interdit
tout script étranger qui l'appellerait ; `freezePrototype: true`, parce que
Tauri injecte `Object.freeze(Object.prototype)` avant tout script de la
page, ce qui ferme la pollution de prototype, et que l'interface n'assigne
jamais de propriété héritée de `Object.prototype`, le bundle esbuild étant
en mode strict ; pas de `devCsp`, pour que le build d'essai tourne sous la
même politique que le build livré ; `dangerousDisableAssetCspModification`
à sa valeur par défaut, pour que Tauri puisse ajouter ses nonces.

### B. Capabilities minimales
`app/permissions/commands.toml`, nouveau, déclare une `[[permission]]` par
commande de `src/main.rs`, `allow-<commande>`, avec une `description`
d'une ligne (en anglais, comme les autres fichiers de configuration) qui
dit pourquoi l'interface en a besoin. tauri-build lit
`app/permissions/**/*.toml` comme manifeste de permissions de
l'application (`tauri_build::build()`, sans changement de `build.rs`) ;
dès qu'il existe, une commande sans permission accordée est refusée à la
page. `app/capabilities/default.json` accorde à la fenêtre `main` :

| Permission | Pourquoi |
|---|---|
| `core:event:allow-listen` | `event.listen` : la fermeture refusée (`fyp://close-requested`) et le glisser-déposer (`tauri://drag-*`), émis par Rust |
| `allow-open-document` | ouvrir le fichier choisi, déposé ou passé en argument |
| `allow-close-document` | oublier le document quand la page se décharge (`beforeunload`) |
| `allow-document-modified` | déclarer les modifications non enregistrées, pour que la fermeture demande d'abord |
| `allow-close-window` | fermer la fenêtre une fois la question réglée |
| `allow-renderer-status` | dire dans la barre d'état si les pages peuvent être dessinées |
| `allow-initial-file` | ouvrir au démarrage le fichier de la ligne de commande |
| `allow-render-page` | dessiner une page (vignette, vue) |
| `allow-rotate-pages` | tourner les pages sélectionnées, annuler et refaire |
| `allow-merge-documents` | fusionner les fichiers choisis à la suite des pages |
| `allow-save-document` | écrire les pages dans leur ordre dans le fichier choisi |
| `allow-pick-open-file` | dialogue natif « ouvrir », côté Rust |
| `allow-pick-merge-files` | dialogue natif « ouvrir plusieurs », côté Rust |
| `allow-pick-save-file` | dialogue natif « enregistrer sous », côté Rust |

`core:event:allow-listen` est la seule fonction de l'API de Tauri que
l'interface appelle et qui aboutisse. `core:default` (`core:app`,
`core:event` entier, `core:image`, `core:menu`, `core:path`,
`core:resources`, `core:tray`, `core:webview`, `core:window`) et
`dialog:default` (`allow-open`, `allow-save`, `allow-message`, jamais
appelés depuis la page) sont retirés : les dialogues sont ouverts côté
Rust (`app.dialog()` dans `main.rs`), où le plugin reste initialisé
(`tauri_plugin_dialog::init()`). La page ne peut plus émettre d'événement
(`event.emit` refusé), ce que l'interface ne faisait jamais, et
`plugin:webview|internal_toggle_devtools` n'est plus accordé, sans effet
visible puisque l'interface arrêtait déjà Ctrl+Maj+I.
`core:window:allow-set-title` n'est pas accordé, à dessein (voir
« Alternatives écartées »).

### C. DevTools absents en release
La feature `devtools` de tauri reste absente (`features = []`) et un test
le garde : wry pose `SetAreDevToolsEnabled(false)` hors `debug_assertions`
sauf avec cette feature, et tauri-runtime-wry n'appelle `with_devtools`
que sous `any(debug_assertions, feature = "devtools")` ; d'après Microsoft,
`AreDevToolsEnabled` « controls whether the user is able to use the
context menu or keyboard shortcuts to open the DevTools window ».

Cela ne fermait pas le port de débogage : WebView2 lit d'abord les
variables d'environnement, puis le registre, et ajoute les arguments
trouvés à ceux que l'application passe (documentation de
`CreateCoreWebView2EnvironmentWithOptions`). `main.rs` retire donc
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` de l'environnement du processus, en
tête de `main()`, avant que WebView2 ne démarre, en release seulement
(`keeps_webview2_browser_arguments()` vaut `cfg!(debug_assertions)`) : le
build de développement la garde, c'est ainsi que l'interface est pilotée
par script, par le port CDP. Mesuré le 19 septembre 2026 sur
`target/release/fyp-app.exe` : avant, port ouvert ; après, port fermé,
l'application démarre normalement.

### D. Menu contextuel natif neutralisé en release
`main.rs` définit la constante `NO_NATIVE_CONTEXT_MENU`
(`window.addEventListener('contextmenu', (event) => event.preventDefault(),
true);`), que `WebviewWindowBuilder::initialization_script` injecte en
release seulement (`initialization_scripts()` est vide en dev). Le script
tourne avant ceux de la page, à chaque chargement, en phase de capture :
le menu natif ne s'ouvre jamais, que le clic droit, la touche Menu ou
Maj+F10 tombent sur la grille, la barre d'état, la vue d'une page ou un
champ de texte. Le menu des vignettes (ADR 0004, point 2) continue de
fonctionner : `main.ts` écoute `contextmenu` sur la grille et ne regarde
pas `defaultPrevented`. En dev, rien ne change : le menu natif reste, avec
« Inspecter », seul chemin vers les outils de développement avec le port
CDP. C'est la deuxième différence visible sanctionnée entre build d'essai
et build livré, après le titre « — DEV » du 17 septembre 2026, demandée
dans le brief du 19 septembre 2026.

## Alternatives écartées
- **Garder `'unsafe-inline'` sur `style-src`** : aucun style inline
  n'existe dans l'interface, et la seule page qui en avait un, celle de
  `app/build.rs`, reçoit un nonce de Tauri.
- **Laisser `connect-src` absent, avec le repli `postMessage`** : l'IPC
  fonctionnait ainsi, mais au prix d'une violation de CSP et d'un
  avertissement à chaque lancement, alors que la directive nomme exactement
  ce dont Tauri a besoin, servi dans le processus, jamais par le réseau.
- **Neutraliser le menu côté natif** (`AreDefaultContextMenusEnabled`) :
  Tauri 2.11 ne transmet pas `with_default_context_menus` de wry, et
  l'atteindre par `with_webview` demanderait un appel COM `unsafe`,
  interdit (`unsafe_code = "forbid"`).
- **Neutraliser le menu dans tous les builds** : en dev, « Inspecter » est
  le seul chemin vers les outils de développement.
- **Accorder `core:window:allow-set-title` maintenant** : le `setTitle` de
  `main.ts` ferait entrer le nom du document dans le titre sans le suffixe
  « — DEV » du build de développement (`window_title`, `main.rs`). C'est
  la correction du bug consigné dans `docs/backlog-ui.md`, une session à
  part ; la description de `capabilities/default.json` le dit.
- **Retirer toutes les variables `WEBVIEW2_*`** : la surcharge par le
  registre relève des stratégies de la machine, et un utilisateur local
  qui peut l'écrire peut de toute façon tout faire de sa session ; les
  autres variables attendent une condition, voir « Limites connues ».

## Conséquences

### Pour l'utilisateur
Rien de visible en release, sauf le menu natif de WebView2 qui ne s'ouvre
plus. Les champs de texte (mot de passe dans le bandeau, numéro de page de
la vue) n'ont donc plus le menu couper/copier/coller de WebView2 ; Ctrl+X,
Ctrl+C et Ctrl+V fonctionnent toujours. Un menu d'édition propre à
l'application pour ces champs est noté dans `docs/backlog-ui.md`, si le
besoin apparaît.

### Limites connues
- **La surcharge par le registre n'est pas neutralisée** :
  `AdditionalBrowserArguments` sous
  `Software\Policies\Microsoft\Edge\WebView2` (`HKLM` puis `HKCU`, valeur
  nommée d'après l'AppId : AUMID, puis nom de l'exécutable, puis `*`)
  s'ajoute toujours aux arguments de l'application, et
  `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER` (un runtime chargé depuis un dossier
  arbitraire) comme `WEBVIEW2_USER_DATA_FOLDER` (le profil déplacé, qui
  écraserait le dossier `data` d'une copie portable) restent lues. Au
  backlog technique, jusqu'à une distribution signée ou un rapport de
  vulnérabilité qui les vise.
- **La CSP ne régit pas la navigation** de la fenêtre elle-même : l'entrée
  `on_navigation` du backlog technique, sous « Faire tenir l'ADR 0006 par
  l'outillage », reste ouverte.
- **`setTitle` reste refusé** : la correction ajoutera
  `core:window:allow-set-title` à la capability et à la liste attendue de
  `the_window_is_allowed_the_commands_of_the_interface_and_nothing_else`.
- **Le build d'essai diffère du build livré** sur le menu contextuel et la
  variable d'environnement, deuxième écart sanctionné après « — DEV » ; la
  checklist manuelle du menu se fait donc sur le build release.

### Vérification
Quatre tests dans `app/src/main.rs`, module `tests` :

- `the_configuration_keeps_a_strict_content_security_policy` : CSP en
  table, `default-src 'none'`, chaque source dans la liste blanche
  (`'none'`, `'self'`, `data:`, `ipc:`, `http://ipc.localhost`), donc
  échec sur `'unsafe-eval'`, `'unsafe-inline'` ou toute source distante ;
  `base-uri` et `form-action` à `'none'` ; pas de `devCsp` ;
  `dangerousDisableAssetCspModification` par défaut ; `freezePrototype` et
  `withGlobalTauri` vrais.
- `the_window_is_allowed_the_commands_of_the_interface_and_nothing_else` :
  la capability accorde exactement `core:event:allow-listen` et une
  permission par commande de la liste `COMMANDS`, chacune définie dans
  `permissions/commands.toml` avec une description non vide, et
  `generate_handler![…]` expose exactement `COMMANDS`.
- `the_devtools_of_tauri_stay_out_of_the_release_build` : lit
  `app/Cargo.toml` avec la crate `toml` (dev-dependency de `fyp-app`, déjà
  dans l'arbre par `fyp-plugin-api`) et vérifie qu'aucun `open_devtools(`
  n'apparaît dans `main.rs`, `render.rs`, `session.rs`.
- `a_release_build_alone_closes_the_native_menu_and_the_debugging_port` :
  la release seule injecte le script et retire la variable, selon le
  profil sous lequel `cargo test` tourne.

Vérifié par script le 19 septembre 2026, sur le build de développement
(38 vérifications) : en-tête servi conforme, complété par Tauri du hash de
`main.js` ; aucune violation vue par la page ; plus d'avertissement de
repli IPC ; `Object.prototype` gelé ; les 13 commandes répondent,
sélecteurs natifs compris, du fichier ouvert à « Fermer sans enregistrer » ;
`listen` fonctionne (une demande `WM_CLOSE` avec des modifications non
enregistrées fait apparaître la question) ; le menu des vignettes
s'affiche. Sur `target/release/fyp-app.exe` : port de débogage ouvert
avant, fermé après, application démarrée normalement. Reste à la checklist
manuelle, sur le build release, où le port CDP est fermé par le point C :
le clic droit, la touche Menu, Maj+F10, F12 et Ctrl+Maj+I n'y ouvrent
rien ; le menu des vignettes s'y montre ; un vrai dépôt de fichier PDF y
ouvre le document, seul chemin par lequel `core:event:allow-listen` sert
au glisser-déposer, non mesuré depuis la réduction des permissions.
