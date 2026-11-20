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
- Le coffre est **toujours chiffré**. Seule l'origine de la clé change :
  - **Clé d'appareil (défaut)** : 32 octets aléatoires (CSPRNG de l'OS),
    clé brute SQLCipher, rangés dans `%APPDATA%\Nyx\vault.key` scellés
    par **DPAPI** (`CryptProtectData`, utilisateur courant). Nyx s'ouvre
    sans rien demander, comme Chrome/Firefox.
  - **Mot de passe principal (optionnel)** : demandé à chaque lancement,
    aucun `vault.key`. Irrécupérable si oublié.
- **Premier lancement** : carte « Bienvenue dans Nyx ». **Plus tard**
  (bouton principal) crée le coffre avec une clé d'appareil ; **Définir un
  mot de passe principal** révèle mot de passe + confirmation (8 caractères
  minimum). L'import de la capsule reste proposé sur cet écran.
- **Lancements suivants** : `vault.key` présent → ouverture silencieuse.
  S'il est illisible (autre compte Windows, fichier corrompu, profil copié
  sur une autre machine) → écran « Coffre Nyx verrouillé » avec
  explication ; rien n'est supprimé. Sans `vault.key` (coffres créés
  avant cette version) → mot de passe principal, comme avant.
- **Paramètres → Mot de passe principal** : *Définir…* (le coffre est
  rechiffré, `vault.key` n'est supprimé qu'après réouverture vérifiée),
  *Changer…* (ancien + nouveau), *Retirer* (mot de passe actuel ; la
  nouvelle clé d'appareil est écrite et relue AVANT le rechiffrement ;
  tout échec restaure l'état précédent). Sans mot de passe principal, la
  section affiche « Protection : session Windows ».
- Les mots de passe n'entrent que par le chrome Nyx (origine interne
  `nyxui.localhost`, IPC validé, commandes acceptées seulement quand le
  panneau correspondant est ouvert, lui-même ouvert par l'action interne
  `nyx://master/…` d'une page interne) : jamais dans une URL, jamais
  journalisés, champs vidés dès l'envoi, copie Rust effacée après usage.

#### Compromis de sécurité

Sans mot de passe principal, toute personne (ou tout programme) qui agit
**dans ta session Windows ouverte** peut desceller `vault.key` et lire les
mots de passe enregistrés — exactement comme Chrome. Le coffre reste
protégé contre le vol du disque ou d'un fichier isolé, et contre les autres
comptes Windows. Le mot de passe principal ajoute une protection contre un
accès à la session ouverte, au prix d'un oubli irrécupérable.

### Reprendre les données de la capsule Nyx (Linux)

Si la capsule WSL existe, Nyx la détecte via
`\\wsl.localhost\NyxCapsule\home\nyx\.local\share\nyx\vault.db`
(ou `\\wsl$\…`) :

- **à la création** du coffre : case « Importer depuis la capsule Nyx
  (Linux) » + passphrase de la capsule, indépendante du mode choisi pour
  Windows (vide = clé d'appareil de la capsule, `vault.key` à côté de son
  coffre, si elle n'a pas de mot de passe principal) ;
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

## Appareils liés (synchronisation PC ↔ téléphone, réseau local)

Paramètres › **Appareils liés** : nom de ce PC, liste des appareils
(« Retirer », avec confirmation), **Ajouter un téléphone**, **Synchroniser
maintenant** et l'état de la dernière synchronisation. Mots de passe,
favoris, historique et réglages sont chiffrés de bout en bout (Noise), sans
compte ni serveur ; détails et modèle de menace : `docs/sync.md`.

- **Ajouter un téléphone** : un QR s'affiche (valable 5 minutes, à usage
  unique). Sur Android : Paramètres › Appareils liés › « Associer à un
  ordinateur » → scanner (ou « Saisir le code » : coller la chaîne
  `nyxpair:v1:…` affichée sous le QR). Les deux écrans montrent alors un
  code à 6 chiffres : s'ils sont identiques, « Les codes correspondent ».
- Dès le déverrouillage du coffre, un service tourne en arrière-plan (thread
  dédié, sa propre connexion au coffre, même clé — jamais écrite) : écoute
  TCP sur le port **47420** (ou le suivant libre jusqu'à 47429), annonce
  mDNS `_nyx-sync._tcp`, synchronisation avec les autres appareils trouvés
  toutes les 5 minutes et à la demande. Le téléphone se synchronise vers le
  PC à son ouverture quand il est sur le même Wi-Fi.

### Pare-feu Windows

La première fois que Nyx ouvre ce port, **Windows affiche « Le pare-feu
Windows Defender a bloqué certaines fonctionnalités de cette
application »**. Coche **Réseaux privés** (domicile, travail) et clique
« Autoriser l'accès ». Ne coche pas « Réseaux publics » (Wi-Fi d'hôtel, de
gare…) : la synchronisation n'y a rien à faire.

Si la fenêtre a été refusée ou fermée, le téléphone ne trouve pas le PC
(« Aucun appareil lié trouvé sur le réseau local ») : Paramètres Windows ›
Confidentialité et sécurité › Sécurité Windows › Pare-feu et protection du
réseau › **Autoriser une application via le pare-feu** › Modifier les
paramètres → cocher `nyx-win` (ou `Nyx`) dans la colonne **Privé**. Vérifie
aussi que le réseau Wi-Fi du PC est en profil **Privé** (Paramètres › Réseau
et Internet › Wi-Fi › propriétés du réseau). Les routeurs avec « isolation
des clients » (Wi-Fi invité) empêchent les appareils de se voir.

## Ce que fait Nyx Windows

- Onglets (Ctrl+T, Ctrl+W, Ctrl+Tab, Ctrl+L/F6), précédent/suivant,
  recharger/arrêter, accueil = page d'accueil des réglages.
- Chaque navigation passe par `nyx_core::security::decide` ; `file://` est
  refusé depuis le web ; les actions internes (`nyx://apply`, `nyx://move`,
  `nyx://history/clear`, imports, `nyx://devices/…`) ne sont acceptées **que depuis une page
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
