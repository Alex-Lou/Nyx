use std::sync::Arc;

use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box as GtkBox, Entry, Orientation, ProgressBar,
};
use webkit2gtk::WebViewExt;

use crate::state::bookmarks::Bookmarks;
use crate::state::settings::Settings;
use crate::ui::tabs::TabBar;
use crate::ui::{navbar, shortcuts};
use crate::web::adblock::AdBlocker;

pub struct BrowserWindow {
    pub window: ApplicationWindow,
    pub tabs:   TabBar,
}

impl BrowserWindow {
    pub fn new(app: &Application, blocker: Arc<AdBlocker>, settings: Settings, bm: Bookmarks) -> Self {
        let window = ApplicationWindow::builder()
            .application(app).title("Nyx")
            .default_width(1400).default_height(860)
            .build();

        let progress = ProgressBar::new();
        progress.style_context().add_class("nyx-progress");
        progress.set_no_show_all(true);
        progress.set_visible(false);

        let url_bar = Entry::builder()
            .placeholder_text("nyx://  ou  recherche…")
            .hexpand(true).build();
        url_bar.style_context().add_class("nyx-urlbar");

        let tabs   = TabBar::new(blocker, settings.clone(), bm.clone());
        let navbar = navbar::build(&url_bar, &tabs, &settings, &bm);

        let vbox = GtkBox::new(Orientation::Vertical, 0);
        vbox.pack_start(&progress,      false, false, 0);
        vbox.pack_start(&navbar,        false, false, 0);
        vbox.pack_start(&tabs.notebook, true,  true,  0);
        window.add(&vbox);

        wire_webview_hooks(&tabs, &url_bar, &progress);
        wire_tab_switch(&tabs, &url_bar, &progress);
        shortcuts::wire(&window, &tabs, &url_bar, &bm);

        Self { window, tabs }
    }

    pub fn show_all(&self) { self.window.show_all(); }
}

/// URL bar + progress câblés une fois par WebView à sa création.
fn wire_webview_hooks(tabs: &TabBar, url_bar: &Entry, progress: &ProgressBar) {
    let ub   = url_bar.clone();
    let prog = progress.clone();
    tabs.set_on_new_webview(move |wv| {
        let ub2 = ub.clone();
        wv.connect_uri_notify(move |w| { ub2.set_text(w.uri().as_deref().unwrap_or("")); });
        let prog2 = prog.clone();
        wv.connect_estimated_load_progress_notify(move |w| {
            let p = w.estimated_load_progress();
            prog2.set_fraction(p);
            prog2.set_visible(p > 0.0 && p < 1.0);
        });
    });
}

/// Resync URL bar + progress au changement d'onglet.
fn wire_tab_switch(tabs: &TabBar, url_bar: &Entry, progress: &ProgressBar) {
    let ub   = url_bar.clone();
    let prog = progress.clone();
    tabs.notebook.connect_switch_page(move |_nb, page, _| {
        if let Ok(wv) = page.clone().downcast::<webkit2gtk::WebView>() {
            ub.set_text(wv.uri().as_deref().unwrap_or(""));
            let p = wv.estimated_load_progress();
            prog.set_fraction(p);
            prog.set_visible(p > 0.0 && p < 1.0);
        }
    });
}
