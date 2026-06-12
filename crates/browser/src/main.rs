mod adblock;
mod content_filter;
mod downloads;
mod findbar;
mod reader;
mod sidebar;
mod startpage;
mod tabs;
mod theme;
mod unlock;
mod urls;
mod webview;
mod window;

use std::rc::Rc;

use gtk::prelude::*;
use gtk::Application;

use adblock::AdBlocker;
use window::BrowserWindow;

const APP_ID: &str = "io.nyx.browser";

fn main() {
    let app = Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_activate(|app| {
        // Thème Nyx — doit être chargé avant toute création de widget
        theme::load();

        if let Some(ctx) = webkit2gtk::WebContext::default() {
            use webkit2gtk::{CookieManagerExt, WebContextExt};

            // Page de démarrage nyx://start (bundlée, animée)
            startpage::register(&ctx);

            // Favicons des onglets (Sprint 1.4) — None = chemin par défaut
            ctx.set_favicon_database_directory(None);

            // Cookies tiers bloqués par défaut (Sprint 5.2)
            if let Some(cookies) = ctx.cookie_manager() {
                cookies.set_accept_policy(webkit2gtk::CookieAcceptPolicy::NoThirdParty);
            }
        }

        // Content filter pubs/trackers (compilé au 1er lancement, asynchrone)
        content_filter::init();

        // Déverrouillage du vault (Sprint 2.1) — abandon = pas de fenêtre,
        // l'application se termine d'elle-même.
        let Some(vault) = unlock::unlock_vault() else { return };
        let vault = Rc::new(vault);

        let blocker = Rc::new(AdBlocker::new());
        let win = BrowserWindow::new(app, blocker, vault);

        // Premier onglet : page d'accueil (nyx://start par défaut)
        win.tabs.open(&win.homepage);

        win.show_all();
    });

    app.run();
}
