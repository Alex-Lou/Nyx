mod adblock;
mod bookmarks;
mod darkmode;
mod favicon;
mod newtab;
mod settings;
mod settings_page;
mod shortcuts;
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

fn main() {
    let app = Application::builder().application_id(APP_ID).build();

    app.connect_activate(|app| {
        theme::load();
        // TODO Sprint 2 : écran de déverrouillage vault.
        let prefs   = settings::new();
        let bm      = bookmarks::new();
        let blocker = Arc::new(AdBlocker::new());
        let win     = BrowserWindow::new(app, blocker, prefs, bm);
        win.tabs.open_new_tab();
        win.show_all();
    });

    app.run();
}
