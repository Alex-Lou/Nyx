mod adblock;
mod favicon;
mod newtab;
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

const APP_ID:   &str = "io.nyx.browser";
pub const HOME_URL: &str = "https://duckduckgo.com";

fn main() {
    let app = Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_activate(|app| {
        theme::load();
        // TODO Sprint 2 : écran de déverrouillage vault.
        let win = BrowserWindow::new(app, Arc::new(AdBlocker::new()));
        win.tabs.open_new_tab();
        win.show_all();
    });

    app.run();
}
