mod adblock;
mod sidebar;
mod tabs;
mod theme;
mod unlock;
mod urls;
mod webview;
mod window;

use std::rc::Rc;
use std::sync::Arc;

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

        // Déverrouillage du vault (Sprint 2.1) — abandon = pas de fenêtre,
        // l'application se termine d'elle-même.
        let Some(vault) = unlock::unlock_vault() else { return };
        let vault = Rc::new(vault);

        let blocker = Arc::new(AdBlocker::new());
        let win = BrowserWindow::new(app, blocker, vault);

        // Premier onglet
        win.tabs.open(HOME_PAGE);

        win.show_all();
    });

    app.run();
}
