//! Nyx natif Windows — coquille mince au-dessus de WebView2 (wry + tao).
//!
//! Même philosophie que l'app Android : AUCUNE politique ici. Navigation
//! (`nyx_core::security`), saisie d'adresse (`nyx_core::url`), blocage
//! (`NyxGuard` + `AdFilter`), noms de téléchargement (`download_policy`) et
//! palette (`theme`) viennent de `nyx-core`.
//!
//! Modules purs (compilés et testés partout) : `ipc`, `content_ipc`, `tabs`,
//! `nav`, `protocol`, `pages`, `adblock`, `downloads`, `keys`, `chrome`,
//! `secret`, `paths`.
//! Coffre (phase 2, testés partout avec de vrais coffres SQLCipher) :
//! `store` (favoris/réglages), `capture`, `autofill`, `history`, `migrate`
//! (capsule WSL), `import_chrome`.
//! Modules Windows : `app` (fenêtre, boucle, onglets ; `app::vault` :
//! déverrouillage, capture, actions internes), `content` (WebView
//! d'onglet), `webview2` (seul module `unsafe` : COM WebView2).

#![cfg_attr(windows, windows_subsystem = "windows")]
// Hors Windows, seuls les tests utilisent les modules purs.
#![cfg_attr(not(windows), allow(dead_code))]

mod adblock;
mod autofill;
mod capture;
mod chrome;
mod content_ipc;
mod downloads;
mod history;
mod import_chrome;
mod ipc;
mod keys;
mod migrate;
mod nav;
mod pages;
mod paths;
mod protocol;
mod secret;
mod store;
mod tabs;

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod content;
#[cfg(windows)]
mod webview2;

#[cfg(windows)]
fn main() {
    if let Err(err) = app::run() {
        webview2::error_box(&format!(
            "Nyx n'a pas pu démarrer.\n\n{err}\n\nVérifiez que le runtime Microsoft Edge WebView2 est installé."
        ));
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("nyx-win est réservé à Windows (WebView2). Sous Linux : cargo run -p nyx");
}
