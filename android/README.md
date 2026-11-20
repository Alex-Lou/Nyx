# Nyx pour Android

Version Android native de Nyx : application Kotlin bâtie sur la WebView
système d'Android, qui réutilise la logique Rust de `nyx-core` et le
coffre chiffré `vault` via la petite crate JNI `crates/nyx-ffi`.

**Contenu** : navigation par onglets, barre d'adresse (URL ou recherche
avec le moteur choisi), page « Nouvel onglet » Nyx avec ses ambiances,
thème sombre, splash animé, blocage publicitaire NyxGuard (EasyList +
EasyPrivacy), et le **coffre chiffré** (favoris, historique, mots de
passe, réglages) au même comportement que sur PC, et les **appareils
liés** : synchronisation chiffrée de bout en bout avec Nyx Windows sur le
même Wi-Fi (Réglages › Appareils liés ; guide : `docs/sync.md`).

**Usage quotidien** :

- **Téléchargements** : confiés au gestionnaire de téléchargements
  d'Android (dossier *Téléchargements*, notification à la fin), avec les
  cookies et l'User-Agent de la page. Le nom du fichier est assaini par
  `nyx_core::download_policy` (export JNI `sanitizeFilename`). Liens
  `blob:` / `data:` pas encore pris en charge. Android 8–9 : la permission
  de stockage est demandée au premier téléchargement.
- **Envoi de fichiers** (`<input type=file>`, sélection multiple).
- **Vidéos plein écran** : barres système masquées, paysage autorisé,
  le bouton retour quitte d'abord le plein écran.
- **Caméra, micro, position, DRM** : toujours un dialogue « *site* veut
  utiliser… » puis, si besoin, la permission Android. Seules les origines
  https sont éligibles ; l'accord DRM est mémorisé pour la session, les
  autres valent pour une seule demande.
- **Rechercher dans la page** : menu ⋮, ou appui long sur le compteur
  d'onglets.
- **Menu ⋮** : ajouter / retirer la page des favoris (étoile), nouvel
  onglet, rechercher, **Favoris**, **Historique**, **Mots de passe**,
  **Réglages**.

## Coffre chiffré

Comme sur PC, tout est dans une base **SQLCipher** (AES-256),
`filesDir/vault.db`, privée à l'application (`allowBackup="false"` ; la
clé Keystore ne quitte jamais l'appareil). Seule l'origine de la clé
change :

- **Clé d'appareil (par défaut)** : 32 octets aléatoires, enveloppés par
  une clé AES-256-GCM du **Keystore Android** (alias `nyx_vault_wrap`,
  non exportable, sans authentification) dans `filesDir/vault.key.enc`.
  Le coffre s'ouvre sans rien demander.
- **Mot de passe principal (optionnel)** : demandé à chaque lancement,
  aucun fichier de clé. Au premier lancement, une carte de bienvenue
  propose « **Plus tard** » (défaut : clé d'appareil) ou « **Définir un
  mot de passe principal** ». Ensuite : **Réglages → Mot de passe
  principal** (définir / changer / retirer).
- **Ordre sûr** (identique à `vault::keymode`) : chaque changement de clé
  est vérifié en rouvrant le coffre ; en cas d'échec la clé précédente est
  restaurée. Le fichier de clé n'est supprimé qu'*après* un rechiffrement
  vérifié, et il est écrit puis relu *avant* de rechiffrer vers une clé
  d'appareil. Un fichier de clé illisible n'efface rien : Nyx demande
  alors le mot de passe principal.
- Les octets de clé traversent JNI en `ByteArray`, effacés des deux côtés
  après usage.

Fonctions du coffre :

- **Mots de passe** : après une connexion sur un site **https**, Nyx
  propose « Enregistrer le mot de passe de … pour *site* ? » (Oui / Non).
  Le script de capture est celui du PC (`assets/password_capture.js`) ;
  il parle à Nyx par `WebViewCompat.addWebMessageListener` (cadres https
  seulement) et Rust revalide l'origine de l'émetteur contre l'hôte de la
  page principale. Désactivable dans les réglages.
- **Pré-remplissage** : https, hôte exact, cadre principal, un seul
  compte enregistré — même politique que Windows
  (`nyx_core::vault_autofill::fill_script`).
- **Écran Mots de passe** : masqués par défaut ; « Afficher » et
  « Copier » demandent le verrouillage de l'appareil (biométrie ou code,
  `androidx.biometric`) s'il est configuré.
- **Historique** : navigations principales http(s), recherche, « Tout
  effacer ». **Favoris** : liste, suppression.
- **Réglages** : moteur de recherche (DuckDuckGo, Brave, Ecosia), page
  d'accueil des nouveaux onglets, bloqueur on/off, enregistrement des
  mots de passe on/off — stockés dans le coffre au même format que le PC.
- **Appareils liés** : nom du téléphone, appareils du profil (Retirer),
  « Associer à un ordinateur » (scan du QR affiché par Nyx Windows —
  lecteur ZXing embarqué, sans Google Play Services — ou « Saisir le
  code »), comparaison du code à 6 chiffres, « Synchroniser maintenant ».
  Synchronisation automatique à l'ouverture et au retour dans l'app quand
  le téléphone est sur Wi-Fi ; aucun service en arrière-plan.

Android n'a pas encore de navigation privée : quand elle arrivera, ni
historique, ni capture, ni pré-remplissage (déjà prévu côté Rust).

## Installer l'APK sur son téléphone

1. Sur GitHub, ouvrir l'onglet **Actions** du dépôt, puis le workflow
   **Android**.
2. Choisir la dernière exécution réussie (coche verte) sur `develop`.
   Pour en lancer une à la main : bouton **Run workflow**.
3. En bas de la page, section **Artifacts**, télécharger
   **`nyx-android-debug`** (une archive `.zip`, il faut être connecté
   à GitHub).
4. Décompresser l'archive : elle contient `app-debug.apk`. Transférer ce
   fichier sur le téléphone (câble USB, Google Drive, e-mail…), ou
   télécharger l'artefact directement depuis le navigateur du téléphone.
5. Sur le téléphone, ouvrir `app-debug.apk` depuis l'application
   **Fichiers**. Android demande d'autoriser l'installation :
   **Paramètres → Applications → Accès spécial → Installer des applications
   inconnues**, puis activer l'application depuis laquelle l'APK est ouvert
   (Fichiers, Chrome, Drive…).
6. Valider l'installation. Nyx apparaît dans le tiroir d'applications.
   Play Protect peut afficher un avertissement (application non publiée sur
   le Play Store) : choisir **Installer quand même**.

Pour mettre à jour : installer le nouvel APK par-dessus l'ancien. L'APK
de debug est signé avec la clé de debug du runner CI ; si la clé change
(nouveau runner sans cache), Android refuse la mise à jour : désinstaller
Nyx puis réinstaller.

Appareils compatibles : Android 8.0 (API 26) ou plus récent, en
**arm64-v8a** (quasiment tous les téléphones actuels) ou **x86_64**
(émulateur).

## Comment le build fonctionne

Le SDK et le NDK Android ne sont pas nécessaires en local : l'APK est
produit par GitHub Actions (`.github/workflows/android.yml`), déclenché
par chaque push sur `develop` qui touche `android/`, `crates/nyx-core/`,
`crates/nyx-ffi/`, `crates/vault/` (ou le workflow lui-même), ou à la main.

1. **Rust → `libnyx_ffi.so`** : `cargo-ndk` compile la crate `nyx-ffi`
   pour `arm64-v8a` et `x86_64` (API 26) avec le NDK préinstallé du
   runner, et dépose les bibliothèques dans
   `android/app/src/main/jniLibs/` (non versionné). SQLCipher et OpenSSL
   sont compilés depuis les sources et liés dans la même bibliothèque
   (feature rusqlite `bundled-sqlcipher-vendored-openssl`, comme sous
   Windows) : Perl (présent sur le runner) pour le `Configure` d'OpenSSL,
   et la chaîne LLVM du NDK dans le `PATH`.
2. **Kotlin → APK** : `./gradlew assembleDebug` (wrapper Gradle 8.9,
   Android Gradle Plugin 8.7, Kotlin 2.0, JDK 17).
3. L'APK est publié comme artefact `nyx-android-debug` (30 jours).

### Architecture

| Couche | Rôle |
|---|---|
| `crates/nyx-core` | Logique pure partagée avec le desktop : résolution de la barre d'adresse, règles NyxGuard, moteur EasyList (`adfilter`), palette des pages (`theme`). |
| `crates/vault` | Coffre SQLCipher partagé avec le PC (schéma, clés, rechiffrement). |
| `crates/nyx-sync` | Appairage QR (Noise XXpsk3) et synchronisation LAN (Noise KK, mDNS) partagés avec le PC. |
| `crates/nyx-ffi` | Pont JNI mince (`io.nyx.browser.NyxCore`, `io.nyx.browser.NyxVault`, `io.nyx.browser.NyxSync`) : helpers purs testés sur l'hôte (`vault_api`, `sync_api`) + marshalling. État global en `OnceLock` / `Mutex`, sûr entre threads ; listes en JSON. |
| `android/app` | Interface Kotlin : `MainActivity` (onglets, barre, menu, retour, téléchargements, fichiers, plein écran, permissions, recherche), `NewTabPage` (page servie par `WebViewAssetLoader` sur `https://appassets.androidplatform.net/`), `NyxApp` (chargement des listes en arrière-plan), `VaultRepo` + `KeyStoreWrap` (coffre, clé d'appareil), `VaultDialogs` (bienvenue, déverrouillage, mot de passe principal), `PasswordCapture` (capture, pré-remplissage), `LibraryActivity` (Favoris / Historique / Mots de passe), `SettingsActivity`, `NyxSync` + `SyncRepo` + `DevicesActivity` (appareils liés). |

Les fichiers `assets/newtab.html`, `assets/password_capture.js` et
`assets/filterlists/*.txt.gz` du dépôt
sont empaquetés directement comme assets Android (pas de copie) : desktop
et mobile partagent la même source.

### Sécurité de la WebView

- JavaScript et DOM storage activés ; accès fichiers et content providers
  **désactivés** ; contenu mixte **interdit** ; Safe Browsing par défaut ;
  cookies tiers refusés ; pas de fenêtres multiples.
- Une page ne peut jamais naviguer vers `file:`, `content:` ou
  `javascript:`. Les liens `intent:`, `market:`, `tel:`, `mailto:` sont
  confiés à Android après assainissement de l'intent.
- Les sous-ressources (scripts, images, iframes…) sont filtrées par
  NyxGuard ; le document principal n'est jamais bloqué.
- Permissions de site : aucune accordée automatiquement ; refus d'office
  pour les origines non https et la page Nouvel onglet. Toute demande en
  attente (permission, sélecteur de fichiers, plein écran) est soldée à la
  fermeture de l'onglet.

### Compiler en local (optionnel)

Avec Android Studio ou un SDK + NDK installés :

```sh
cargo install cargo-ndk --version 3.5.4 --locked
rustup target add aarch64-linux-android x86_64-linux-android
cargo ndk -t arm64-v8a -t x86_64 --platform 26 \
    -o android/app/src/main/jniLibs build --release -p nyx-ffi
cd android && ./gradlew assembleDebug
# APK : android/app/build/outputs/apk/debug/app-debug.apk
```
