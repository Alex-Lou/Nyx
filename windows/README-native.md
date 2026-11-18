# Nyx natif Windows (WebView2) — phase 1

`crates/nyx-win` : Nyx sans WSL, sur le moteur **WebView2** (Chromium/Edge)
déjà présent sur Windows 10/11. Plus léger que la capsule (pas de VM),
décodage vidéo GPU, SmartScreen + Mark-of-the-Web sur les téléchargements,
moteur mis à jour par Microsoft. DRM : celui fourni par le runtime WebView2
(PlayReady) — Netflix & co. sont à vérifier sur ta machine.

Même philosophie qu'Android : coquille mince, **toute la politique dans
`nyx-core`** (navigation, barre d'adresse, blocage pub/traqueurs, noms de
téléchargement, palette).

## Lancer depuis Git Bash

Prérequis : Rust **MSVC** (`rustup default stable-x86_64-pc-windows-msvc`) +
Visual Studio Build Tools, charge « Développement Desktop en C++ ». Le
runtime WebView2 est préinstallé sur Windows 10/11.

```bash
cargo run --release -p nyx-win
```

Installation + raccourci Bureau « Nyx » (remplace celui de la capsule) :

```bash
powershell -ExecutionPolicy Bypass -File windows/nyx-native.ps1
```

Sans compiler : onglet **Actions → Windows** du dépôt, artefact
`nyx-windows` → `Nyx.exe`.

## Ce que fait la phase 1

- Onglets (Ctrl+T, Ctrl+W, Ctrl+Tab, Ctrl+L/F6), précédent/suivant,
  recharger/arrêter, accueil = Nouvel onglet (`nyx://newtab`).
- Chaque navigation passe par `nyx_core::security::decide` ; `file://` est
  refusé depuis le web ; les pages/actions internes autres que Nouvel
  onglet ne sont pas encore implémentées (redirigées ou refusées).
- Popups (`target=_blank`, `window.open`) → nouvel onglet, jamais de fenêtre.
- Blocage des sous-ressources : NyxGuard dès le démarrage, puis
  EasyList + EasyPrivacy (chargées en arrière-plan) ; prévention du pistage
  WebView2 en mode Strict.
- Téléchargements natifs WebView2, nom assaini par `download_policy`.
- Permissions (caméra, micro, position, notifications) : jamais accordées
  automatiquement, invite native WebView2.
- Données : `%LOCALAPPDATA%\Nyx\WebView2`.

## Hors périmètre (phase 2)

- **Coffre** (`vault` : mots de passe, favoris, historique) — SQLCipher +
  OpenSSL sous MSVC est un chantier à part. La coquille est prête : un
  module `vault` se branchera sur `app.rs` (événements `PageLoad`/`Title`).
- Pages internes Réglages / Favoris / Historique, mode Banking/Shadow.
- `window.open` avec `opener` (certaines connexions OAuth en popup).
