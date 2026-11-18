//! Nyx natif Windows — coquille mince au-dessus de WebView2 (wry + tao).
//!
//! Même philosophie que l'app Android : AUCUNE politique ici. Navigation
//! (`nyx_core::security`), saisie d'adresse (`nyx_core::url`), blocage
//! (`NyxGuard` + `AdFilter`), noms de téléchargement (`download_policy`) et
//! palette (`theme`) viennent de `nyx-core`.
//!
//! Modules purs (compilés et testés partout) : `ipc`, `tabs`, `nav`,
//! `protocol`, `adblock`, `downloads`, `keys`, `chrome`.
//! Modules Windows : `app` (fenêtre, boucle, onglets), `content` (WebView
//! d'onglet), `webview2` (seul module `unsafe` : COM WebView2).
//!
//! Phase 2 : intégration du coffre (`vault` : mots de passe, favoris,
//! historique) via un module `vault` branché sur `app`, comme `nyx-ffi`.

#![cfg_attr(windows, windows_subsystem = "windows")]
// Hors Windows, seuls les tests utilisent les modules purs.
#![cfg_attr(not(windows), allow(dead_code))]

mod adblock;
mod chrome;
mod downloads;
mod ipc;
mod keys;
mod nav;
mod protocol;
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
