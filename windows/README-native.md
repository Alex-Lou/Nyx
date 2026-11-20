# Nyx natif Windows (WebView2) — phase 2 : le coffre

`crates/nyx-win` : Nyx sans WSL, sur le moteur **WebView2** (Chromium/Edge)
déjà présent sur Windows 10/11. Plus léger que la capsule (pas de VM),
décodage vidéo GPU, SmartScreen + Mark-of-the-Web sur les téléchargements,
moteur mis à jour par Microsoft. DRM : celui fourni par le runtime WebView2
(PlayReady) — Netflix & co. sont à vérifier sur ta machine.

Même philosophie qu'Android : coquille mince, **toute la politique dans
`nyx-core`** (navigation, barre d'adresse, blocage pub/traqueurs, noms de
téléchargement, pré-remplissage, palette) et **le même coffre `vault`**
(SQLCipher) que le Nyx Linux.

## Lancer depuis Git Bash

Prérequis :

- Rust **MSVC** (`rustup default stable-x86_64-pc-windows-msvc`) + Visual
  Studio Build Tools, charge « Développement Desktop en C++ » ;
- **Strawberry Perl** (`winget install StrawberryPerl.StrawberryPerl`) : le
  coffre compile SQLCipher et OpenSSL depuis les sources (aucune DLL à
  livrer). Le Perl de Git Bash ne convient pas au `Configure` d'OpenSSL ;
- le runtime WebView2 (préinstallé sur Windows 10/11).

```bash
export OPENSSL_SRC_PERL='C:\Strawberry\perl\bin\perl.exe'
export OPENSSL_RUST_USE_NASM=0   # OpenSSL sans assembleur : pas besoin de NASM
cargo run --release -p nyx-win
```

Le premier build est long (OpenSSL), les suivants sont incrémentaux.

Installation + raccourci Bureau « Nyx » (remplace celui de la capsule) :

```bash
powershell -ExecutionPolicy Bypass -File windows/nyx-native.ps1
```

Sans compiler : onglet **Actions → Windows** du dépôt, artefact
`nyx-windows` → `Nyx.exe` (la CI règle Perl / NASM toute seule).

## Le coffre

- **Emplacement** : `%APPDATA%\Nyx\vault.db` (chiffré AES-256 par
  SQLCipher). Le profil WebView2 (cookies, cache) reste dans
  `%LOCALAPPDATA%\Nyx\WebView2`.
- **Premier lancement** : écran « Crée la passphrase de ton coffre Nyx »
  (passphrase + confirmation). Irrécupérable si oubliée.
- **Lancements suivants** : écran « Coffre Nyx verrouillé ». Mauvaise
  passphrase → message, nouvel essai. **Quitter** ferme Nyx.
- La passphrase n'entre que par le chrome Nyx (origine interne
  `nyxui.localhost`, IPC validé) : jamais journalisée, champs vidés dès
  l'envoi, copie Rust effacée après usage.

### Reprendre les données de la capsule Nyx (Linux)

Si la capsule WSL existe, Nyx la détecte via
`\\wsl.localhost\NyxCapsule\home\nyx\.local\share\nyx\vault.db`
(ou `\\wsl$\…`) :

- **à la création** du coffre : case « Importer depuis la capsule Nyx
  (Linux) » + passphrase de la capsule (vide = la même) ;
- **plus tard** : *Paramètres → Importer la capsule*.

Mots de passe, favoris, historique et réglages sont recopiés **sans
doublon** (un identifiant déjà présent est conservé, les réglages Windows
ne sont jamais écrasés). Le coffre de la capsule n'est **jamais modifié** :
Nyx en lit une copie temporaire, supprimée ensuite.

Capsule « injoignable » = WSL arrêté. Démarre-la dans un terminal puis
clique **Réessayer** :

```bash
wsl -d NyxCapsule -e true
```

## Ce que fait Nyx Windows

- Onglets (Ctrl+T, Ctrl+W, Ctrl+Tab, Ctrl+L/F6), précédent/suivant,
  recharger/arrêter, accueil = page d'accueil des réglages.
- Chaque navigation passe par `nyx_core::security::decide` ; `file://` est
  refusé depuis le web ; les actions internes (`nyx://apply`, `nyx://move`,
  `nyx://history/clear`, imports) ne sont acceptées **que depuis une page
  interne** (canal natif dont WebView2 certifie l'émetteur, ou lien direct
  non issu d'une redirection).
- **Mots de passe** : à la connexion sur un site, barre « Enregistrer le mot
  de passe de … ? Oui / Non » (ou « Mettre à jour »). Même script de
  capture que le Linux (`assets/password_capture.js`) ; l'hôte annoncé doit
  être celui de la page principale de l'onglet. Désactivable dans les
  réglages.
- **Pré-remplissage** : décidé par `nyx_core::vault_autofill` — https,
  hôte exact, cadre principal, un seul compte enregistré pour le site,
  formulaire qui ne part pas vers une autre origine. Jamais en navigation
  privée.
- **Historique** : pages http(s) chargées (titre mis à jour ensuite), rien
  en navigation privée. `nyx://history`, « Tout effacer ».
- **Favoris** : étoile ☆/★ dans la barre, `nyx://bookmarks` (dossiers,
  glisser-déposer).
- **Paramètres** `nyx://settings` : moteur de recherche, page d'accueil,
  NyxGuard, connexions tierces, enregistrement des mots de passe,
  navigation privée ; **Importer depuis Chrome** (favoris + historique d'un
  profil de `%LOCALAPPDATA%\Google\Chrome\User Data`, Chrome fermé pour
  l'historique) et **Importer la capsule**. Réglages stockés dans le coffre
  (`app_settings`, même format que le Linux).
- Popups (`target=_blank`, `window.open`) → nouvel onglet, jamais de fenêtre.
- Blocage des sous-ressources : NyxGuard dès le démarrage, puis
  EasyList + EasyPrivacy (chargées en arrière-plan) ; prévention du pistage
  WebView2 en mode Strict.
- Téléchargements natifs WebView2, nom assaini par `download_policy`.
- Permissions (caméra, micro, position, notifications) : jamais accordées
  automatiquement, invite native WebView2.

## Hors périmètre (pour l'instant)

- Mots de passe Chrome (chiffrés DPAPI) : non importés ; passer par
  l'export CSV de Chrome côté Linux, puis importer la capsule.
- Gestion des mots de passe enregistrés (liste / suppression) dans l'UI.
- Modes Banking / Shadow, mode sombre des sites, refus automatique des
  bannières cookies (réglages conservés tels quels dans le coffre).
- `window.open` avec `opener` (certaines connexions OAuth en popup).
