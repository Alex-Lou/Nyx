//! Nyx — navigateur web personnel, privé et léger.
//!
//! Organisation des modules :
//!   state/  — réglages & favoris (modèles, sans GTK)
//!   web/    — moteur WebKit : config, NyxWatch, dark mode, routage nyx://
//!   pages/  — pages internes HTML (newtab, settings, bookmarks)
//!   ui/     — interface GTK : fenêtre, navbar, onglets, raccourcis, thème

mod pages;
mod state;
mod ui;
mod web;

use std::sync::Arc;

use gtk::prelude::*;
use gtk::Application;

use ui::window::BrowserWindow;
use web::nyxwatch::NyxWatch;

const APP_ID: &str = "io.nyx.browser";

fn main() {
    let app = Application::builder().application_id(APP_ID).build();

    app.connect_activate(|app| {
        ui::theme::load();
        // TODO Sprint 2 : écran de déverrouillage vault.
        let prefs   = state::settings::new();
        let bm      = state::bookmarks::new();
        let blocker = Arc::new(NyxWatch::new());
        let win     = BrowserWindow::new(app, blocker, prefs, bm);
        win.tabs.open_new_tab();
        win.show_all();
    });

    app.run();
}
