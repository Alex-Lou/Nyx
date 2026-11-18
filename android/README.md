# Nyx pour Android

Version Android native de Nyx (phase 1) : application Kotlin bâtie sur
la WebView système d'Android, qui réutilise la logique Rust de `nyx-core`
via la petite crate JNI `crates/nyx-ffi`.

**Contenu de la phase 1** : navigation par onglets, barre d'adresse
(URL ou recherche DuckDuckGo), page « Nouvel onglet » Nyx avec ses
ambiances, thème sombre, splash animé, blocage publicitaire NyxGuard
(EasyList + EasyPrivacy). Le coffre-fort de mots de passe et la
synchronisation avec le PC arriveront dans les phases suivantes.

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
`crates/nyx-ffi/` (ou le workflow lui-même), ou à la main.

1. **Rust → `libnyx_ffi.so`** : `cargo-ndk` compile la crate `nyx-ffi`
   pour `arm64-v8a` et `x86_64` (API 26) avec le NDK préinstallé du
   runner, et dépose les bibliothèques dans
   `android/app/src/main/jniLibs/` (non versionné).
2. **Kotlin → APK** : `./gradlew assembleDebug` (wrapper Gradle 8.9,
   Android Gradle Plugin 8.7, Kotlin 2.0, JDK 17).
3. L'APK est publié comme artefact `nyx-android-debug` (30 jours).

### Architecture

| Couche | Rôle |
|---|---|
| `crates/nyx-core` | Logique pure partagée avec le desktop : résolution de la barre d'adresse, règles NyxGuard, moteur EasyList (`adfilter`), palette des pages (`theme`). |
| `crates/nyx-ffi` | Pont JNI mince (`io.nyx.browser.NyxCore`) : aucune logique, que du marshalling. État global en `OnceLock`, sûr entre threads. |
| `android/app` | Interface Kotlin : `MainActivity` (onglets, barre, retour), `NewTabPage` (page servie par `WebViewAssetLoader` sur `https://appassets.androidplatform.net/`), `NyxApp` (chargement des listes en arrière-plan). |

Les fichiers `assets/newtab.html` et `assets/filterlists/*.txt.gz` du dépôt
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
