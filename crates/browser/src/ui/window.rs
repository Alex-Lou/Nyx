use std::sync::Arc;

use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box as GtkBox, Entry, HeaderBar, Orientation, ProgressBar,
};
use webkit2gtk::WebViewExt;

use crate::state::bookmarks::Bookmarks;
use crate::state::settings::{LastTab, Settings};
use crate::ui::tabs::TabBar;
use crate::ui::{navbar, shortcuts};
use crate::web::nyxguard::NyxGuard;

pub struct BrowserWindow {
    pub window: ApplicationWindow,
    pub tabs:   TabBar,
}

impl BrowserWindow {
    pub fn new(app: &Application, blocker: Arc<NyxGuard>, settings: Settings, bm: Bookmarks) -> Self {
        let window = ApplicationWindow::builder()
            .application(app).title("Nyx")
            .default_width(1400).default_height(860)
            .build();

        // Titlebar Nyx (CSD) : remplace la barre système « froide » par une
        // barre thémée, cohérente avec le reste de la fenêtre.
        let header = HeaderBar::new();
        header.set_show_close_button(true);
        header.set_title(Some("Nyx"));
        header.style_context().add_class("nyx-headerbar");
        window.set_titlebar(Some(&header));

        let progress = ProgressBar::new();
        progress.style_context().add_class("nyx-progress");
        progress.set_no_show_all(true);
        progress.set_visible(false);

        let url_bar = Entry::builder()
            .placeholder_text("nyx://  ou  recherche…")
            .hexpand(true).build();
        url_bar.style_context().add_class("nyx-urlbar");

        let tabs   = TabBar::new(blocker, settings.clone(), bm.clone());
        tabs.set_parent(&window);  // ancre le modal Paramètres
        let navbar = navbar::build(&url_bar, &tabs, &settings, &bm);

        let vbox = GtkBox::new(Orientation::Vertical, 0);
        vbox.pack_start(&progress,      false, false, 0);
        vbox.pack_start(&navbar,        false, false, 0);
        vbox.pack_start(&tabs.notebook, true,  true,  0);
        window.add(&vbox);

        wire_webview_hooks(&tabs, &url_bar, &progress);
        wire_tab_switch(&tabs, &url_bar, &progress);
        wire_last_tab(&window, &tabs, &settings);
        wire_double_click(&tabs);
        shortcuts::wire(&window, &tabs, &url_bar, &bm);

        Self { window, tabs }
    }

    pub fn show_all(&self) { self.window.show_all(); }
}

/// Fermeture du dernier onglet → selon réglage : fermer Nyx ou rouvrir l'accueil.
fn wire_last_tab(window: &ApplicationWindow, tabs: &TabBar, settings: &Settings) {
    let w = window.clone();
    let t = tabs.clone();
    let s = settings.clone();
    tabs.notebook.connect_page_removed(move |nb, _, _| {
        if nb.n_pages() != 0 {
            return;
        }
        match s.borrow().on_last_tab {
            LastTab::CloseWindow => w.close(),
            LastTab::Home => {
                // Différé : on ne rouvre pas un onglet en plein signal de retrait.
                let t2 = t.clone();
                gtk::glib::idle_add_local_once(move || { t2.open_home(); });
            }
        }
    });
}

/// Double-clic dans la bande d'onglets (zone vide) → nouvel onglet d'accueil.
fn wire_double_click(tabs: &TabBar) {
    let t = tabs.clone();
    tabs.notebook.connect_button_press_event(move |_nb, ev| {
        if ev.event_type() == gtk::gdk::EventType::DoubleButtonPress
            && ev.button() == 1
            && ev.position().1 < 42.0   // bande d'en-tête uniquement
        {
            t.open_home();
            return gtk::glib::Propagation::Stop;
        }
        gtk::glib::Propagation::Proceed
    });
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
