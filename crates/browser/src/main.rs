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

const APP_ID: &str    = "io.nyx.browser";
const HOME_PAGE: &str = "https://duckduckgo.com";

fn main() {
    let app = Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_activate(|app| {
        // Thème Nyx — doit être chargé avant toute création de widget
        theme::load();

        // TODO Sprint 2 : écran de déverrouillage vault ici

        let blocker = Arc::new(AdBlocker::new());
        let win = BrowserWindow::new(app, blocker.clone());

        // Premier onglet
        win.tabs.open(HOME_PAGE);

        // Ctrl+T → nouvel onglet
        {
            let tabs_nb = win.tabs.notebook.clone();
            let bl = blocker.clone();
            win.window.connect_key_press_event(move |_, event| {
                use gtk::gdk::keys::constants as key;
                if event.state().contains(gtk::gdk::ModifierType::CONTROL_MASK)
                    && event.keyval() == key::t
                {
                    let wv = webkit2gtk::WebView::new();
                    wv.set_vexpand(true);
                    wv.set_hexpand(true);
                    webview::configure(&wv, bl.clone());
                    let label = gtk::Label::new(Some("Nouveau"));
                    let idx = tabs_nb.append_page(&wv, Some(&label));
                    wv.load_uri(HOME_PAGE);
                    tabs_nb.set_current_page(Some(idx));
                    wv.show();
                    return gtk::Inhibit(true);
                }
                gtk::Inhibit(false)
            });
        }

        win.show_all();
    });

    app.run();
}
