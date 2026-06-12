# ROADMAP — my-browser

Chaque sprint est indépendant et livrable. L'ordre est strict : chaque sprint
s'appuie sur le précédent. Les tickets sont des fichiers/fonctionnalités
concrètes à implémenter, pas des intentions vagues.

---

## Sprint 0 — Foundation ✅ (fait)

**Objectif** : repo compilable, architecture posée, aucune dépendance cachée.

| # | Tâche | Fichier cible |
|---|-------|---------------|
| 0.1 | Workspace Cargo avec deux crates | `Cargo.toml` |
| 0.2 | Schéma SQLite + ouverture SQLCipher | `vault/src/db.rs` |
| 0.3 | Modèles de données (Bookmark, Password, HistoryEntry) | `vault/src/models.rs` |
| 0.4 | CRUD bookmarks / passwords / history | `vault/src/{bookmarks,passwords,history}.rs` |
| 0.5 | API publique `Vault` | `vault/src/lib.rs` |
| 0.6 | Fenêtre GTK + WebView skeleton | `browser/src/{window,webview,main}.rs` |
| 0.7 | AdBlocker pipeline (domaines en dur) | `browser/src/adblock.rs` |
| 0.8 | Résolution d'URL (bare domain → https, texte → DDG) | `browser/src/webview.rs` |

**Définition of done** : `cargo build` passe sur les deux crates.

---

## Sprint 1 — Browser core navigable 🟡 (partiel)

**Objectif** : on peut réellement surfer, les onglets de base fonctionnent.

> Restent à faire : 1.4 favicon, 1.7 bouton home, 1.8 `create-web-view` → nouvel onglet.

| # | Tâche | Notes |
|---|-------|-------|
| 1.1 | Onglets (`gtk::Notebook`) | Créer / fermer / switcher |
| 1.2 | Chaque onglet a son propre `WebView` | Isolation via `WebContext` séparé |
| 1.3 | Titre de l'onglet mis à jour depuis `notify::title` | Signal WebView |
| 1.4 | Favicon dans l'onglet | `WebViewExt::favicon()` |
| 1.5 | Raccourcis clavier : Ctrl+T, Ctrl+W, Ctrl+L, Ctrl+R | `gtk::AccelGroup` |
| 1.6 | Barre de progression de chargement | `gtk::ProgressBar` + `estimated-load-progress` |
| 1.7 | Bouton home | Charge `HOME_PAGE` |
| 1.8 | Ouverture de liens dans un nouvel onglet | `create-web-view` signal |

**Définition of done** : navigation multi-onglets fluide, raccourcis fonctionnels.

---

## Sprint 2 — Vault : déverrouillage + UI basique ✅ (fait)

**Objectif** : le vault est intégré dans le browser, les données persistent.

> Implémenté : unlock dialog (`unlock.rs`), sidebar Historique/Favoris/Clés
> (`sidebar.rs`), historique auto + capture MDP câblés dans `window.rs`/`webview.rs`.
> Vault stocké dans `~/.local/share/nyx/vault.db`.

| # | Tâche | Notes |
|---|-------|-------|
| 2.1 | Écran de déverrouillage (dialog GTK) au démarrage | Passphrase → `Vault::open()` |
| 2.2 | Sauvegarde automatique de l'historique | Signal `load-changed` → `vault.push_history()` |
| 2.3 | Panel historique (sidebar ou dialog) | Liste + searchbar |
| 2.4 | Ajouter un bookmark depuis la barre d'adresse | Bouton ★ |
| 2.5 | Panel bookmarks | Liste + supprimer + tags |
| 2.6 | Autocomplétion URL-bar depuis bookmarks + history | `gtk::EntryCompletion` |
| 2.7 | Panel mots de passe | Liste par domaine, copier, supprimer |
| 2.8 | Proposition d'enregistrement de MDP détectée | Injection JS + signal `script-message-received` |

**Définition of done** : les données survivent à un redémarrage, le vault est verrouillé sans passphrase.

---

## Sprint 3 — Bloqueur de pub production-ready

**Objectif** : le bloqueur filtre vraiment les pubs via des règles standards.

| # | Tâche | Notes |
|---|-------|-------|
| 3.1 | Intégrer la crate `adblock = "0.9"` (Brave) | Remplace la liste en dur |
| 3.2 | Bundler EasyList + EasyPrivacy au compile time | `include_bytes!` + fichiers dans `assets/` |
| 3.3 | Filtrage réseau via `decide-policy` | Tous types de ressources (img, script, xhr…) |
| 3.4 | Whitelist configurable | Stockée dans le vault (table `settings`) |
| 3.5 | Compteur de pubs bloquées par onglet | Affiché dans la barre |
| 3.6 | Mise à jour des listes au démarrage (optionnel, async) | `tokio::spawn` + HTTP GET |

**Définition of done** : YouTube et un site d'actu s'ouvrent sans pubs mesurables.

---

## Sprint 4 — UX & ergonomie

**Objectif** : le navigateur est plaisant à utiliser au quotidien.

| # | Tâche | Notes |
|---|-------|-------|
| 4.1 | Mode sombre / clair (respect du thème système) | `gtk::Settings::gtk-application-prefer-dark-theme` |
| 4.2 | Recherche dans la page (Ctrl+F) | `WebViewExt::find_text()` |
| 4.3 | Téléchargements | `DownloadManager` signal WebKit |
| 4.4 | Menu contextuel custom | Remplace le menu natif WebKit |
| 4.5 | Zoom (Ctrl++ / Ctrl+- / Ctrl+0) | `set_zoom_level()` |
| 4.6 | Mode lecture (strip ads + CSS minimal) | Injection JS style Readability.js |
| 4.7 | Barre latérale rétractable (bookmarks / history) | `gtk::Paned` |

**Définition of done** : usage quotidien confortable pendant 1 semaine sans régression.

---

## Sprint 5 — Privacy avancée

**Objectif** : le navigateur protège activement la vie privée.

| # | Tâche | Notes |
|---|-------|-------|
| 5.1 | HTTPS forcé (redirect HTTP → HTTPS) | Avant `load_uri` |
| 5.2 | Gestion des cookies : bloquer tiers par défaut | `CookieManager` WebKit |
| 5.3 | Nettoyage cookies/cache à la fermeture (option) | `WebsiteDataManager` |
| 5.4 | Bloquer WebRTC leak | `set_enable_media_stream(false)` déjà fait — vérifier IP leak |
| 5.5 | User-Agent neutre configurable | `settings.set_user_agent()` |
| 5.6 | Désactiver JS par domaine | Liste noire dans le vault |
| 5.7 | Rapport de confidentialité par site | Ce qui a été bloqué sur la page courante |

**Définition of done** : coveryourtracks.eff.org montre une empreinte réduite.

---

## Sprint 6 — Packaging & distribution

**Objectif** : le browser s'installe et se lance comme une vraie app.

| # | Tâche | Notes |
|---|-------|-------|
| 6.1 | Fichier `.desktop` + icône | Standard XDG |
| 6.2 | Build AppImage (portable, sans install) | `linuxdeploy` |
| 6.3 | Paquet `.deb` | `cargo-deb` |
| 6.4 | CI GitHub Actions : build + test sur Ubuntu | `ubuntu-latest` runner |
| 6.5 | Profiling mémoire (valgrind / heaptrack) | Cible < 80 MB RAM idle |
| 6.6 | README install + usage | Prérequis système, build from source |

**Définition of done** : un utilisateur lambda peut installer et lancer sans compiler.

---

## Backlog (pas de sprint assigné)

- Sync vault chiffré via fichier (Syncthing, Nextcloud — pas de cloud proprio)
- Extensions via scripts JS injectés
- Gestionnaire de sessions (sauvegarder/restaurer un groupe d'onglets)
- Support Wayland natif
- Build macOS (WebKit natif via WKWebView)
