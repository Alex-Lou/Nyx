//! Nyx — navigateur web personnel, privé et léger.
//!
//! Organisation des modules :
//!   state/  — réglages & favoris (modèles, sans GTK)
//!   web/    — moteur WebKit : config, NyxGuard, dark mode, routage nyx://
//!   pages/  — pages internes HTML (newtab, settings, bookmarks)
//!   ui/     — interface GTK : fenêtre, navbar, onglets, raccourcis, thème

mod pages;
mod platform;
mod state;
mod ui;
mod web;

use std::sync::Arc;

use gtk::prelude::*;
use gtk::Application;

use ui::window::BrowserWindow;
use web::nyxguard::NyxGuard;

const APP_ID: &str = "io.nyx.browser";

fn main() {
    // Debug sécurité : `NYX_SECURITY_DEBUG=1 cargo run` → stderr structuré
    // pour chaque décision (BLOCK / DENY / WARN / ALLOW). Voir docs/security.md §4.
    nyx_core::sec_log::set_enabled(std::env::var_os("NYX_SECURITY_DEBUG").is_some());

    // Silence le bruit non pertinent au démarrage : pont d'accessibilité
    // at-spi (spam org.a11y/atspi) et messages de debug verbeux GLib.
    std::env::set_var("NO_AT_BRIDGE", "1");
    std::env::set_var("G_MESSAGES_DEBUG", "");
    std::env::set_var("GST_DEBUG", "0");

    // WSL : le renderer GPU/DMABUF de WebKit crashe (pas de vrai GPU —
    // « MESA ZINK failed », « egl: failed to create dri2 screen »). On force
    // le rendu logiciel avant tout init GTK/WebKit.
    if platform::is_wsl() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        std::env::set_var("LIBGL_ALWAYS_SOFTWARE", "1");
    }

    let app = Application::builder().application_id(APP_ID).build();

    app.connect_activate(|app| {
        ui::theme::load();
        ui::icon::set_default();
        // TODO Sprint 2 : écran de déverrouillage vault.
        let prefs   = state::settings::new();
        let bm      = state::bookmarks::new();
        let perms   = nyx_core::permissions::new();
        let blocker = Arc::new(NyxGuard::new());
        let win     = BrowserWindow::new(app, blocker, prefs, bm, perms);
        win.tabs.open_new_tab();
        win.show_all();
        ui::anim::fade_in(&win.window); // fondu d'ouverture discret
    });

    app.run();
}
