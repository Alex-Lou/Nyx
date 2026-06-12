mod adblock;
mod tabs;
mod theme;
mod webview;
mod window;

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

        // TODO Sprint 2 : écran de déverrouillage vault ici

        let blocker = Arc::new(AdBlocker::new());
        let win = BrowserWindow::new(app, blocker);

        // Premier onglet
        win.tabs.open(HOME_PAGE);

        win.show_all();
    });

    app.run();
}
