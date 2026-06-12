mod adblock;
mod content_filter;
mod downloads;
mod findbar;
mod reader;
mod sidebar;
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

const APP_ID: &str        = "io.nyx.browser";
pub const HOME_PAGE: &str = "https://duckduckgo.com";

fn main() {
    let app = Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_activate(|app| {
        // Thème Nyx — doit être chargé avant toute création de widget
        theme::load();

        // Favicons des onglets (Sprint 1.4) — None = chemin par défaut
        if let Some(ctx) = webkit2gtk::WebContext::default() {
            use webkit2gtk::{CookieManagerExt, WebContextExt};
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

        // Premier onglet
        win.tabs.open(HOME_PAGE);

        win.show_all();
    });

    app.run();
}
