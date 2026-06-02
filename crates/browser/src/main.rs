mod adblock;
mod newtab;
mod tabs;
mod theme;
mod webview;
mod window;

use std::sync::Arc;

use gtk::prelude::*;
use gtk::Application;

use adblock::AdBlocker;
use window::BrowserWindow;

const APP_ID: &str = "io.nyx.browser";

/// Page chargée par le bouton home — DuckDuckGo, jamais Google.
pub const HOME_URL: &str = "https://duckduckgo.com";

fn main() {
    let app = Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_activate(|app| {
        // Thème Nyx — doit être chargé avant la création des widgets.
        theme::load();

        // TODO Sprint 2 : écran de déverrouillage vault ici.

        let blocker = Arc::new(AdBlocker::new());
        let win = BrowserWindow::new(app, blocker);

        // Premier onglet : page de démarrage Nyx.
        win.tabs.open_new_tab();

        win.show_all();
    });

    app.run();
}
