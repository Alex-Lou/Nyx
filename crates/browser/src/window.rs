use std::sync::Arc;

use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box as GtkBox, Button, Entry,
    Orientation, ProgressBar,
};
use webkit2gtk::WebViewExt;

use crate::adblock::AdBlocker;
use crate::bookmarks::Bookmarks;
use crate::settings::Settings;
use crate::tabs::TabBar;
use crate::{shortcuts, webview};

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

        let navbar = build_navbar(&url_bar);
        let tabs   = TabBar::new(blocker, settings.clone(), bm.clone());

        let vbox = GtkBox::new(Orientation::Vertical, 0);
        vbox.pack_start(&progress,      false, false, 0);
        vbox.pack_start(&navbar.bar,    false, false, 0);
        vbox.pack_start(&tabs.notebook, true,  true,  0);
        window.add(&vbox);

        wire_webview_hooks(&tabs, &url_bar, &progress);
        wire_tab_switch(&tabs, &url_bar, &progress);
        wire_buttons(&navbar, &tabs, &url_bar, &settings, &bm);
        shortcuts::wire(&window, &tabs, &url_bar, settings.clone());

        Self { window, tabs }
    }

    pub fn show_all(&self) { self.window.show_all(); }
}

// ── Navbar ───────────────────────────────────────────────────────────────

struct Navbar {
    bar:         GtkBox,
    back_btn:    Button,
    forward_btn: Button,
    reload_btn:  Button,
    home_btn:    Button,
    star_btn:    Button,
    new_tab_btn: Button,
    settings_btn:Button,
}

fn build_navbar(url_bar: &Entry) -> Navbar {
    let back_btn     = nav_button("◀",  "Précédent");
    let forward_btn  = nav_button("▶",  "Suivant");
    let reload_btn   = nav_button("↺",  "Recharger (Ctrl+R)");
    let home_btn     = nav_button("⌂",  "Accueil (Alt+Home)");
    let star_btn     = nav_button("☆",  "Ajouter aux favoris (Ctrl+D)");
    let new_tab_btn  = nav_button("+",  "Nouvel onglet (Ctrl+T)");
    let settings_btn = nav_button("⚙",  "Paramètres (Ctrl+,)");

    let bar = GtkBox::new(Orientation::Horizontal, 4);
    bar.style_context().add_class("nyx-navbar");
    bar.pack_start(&back_btn,     false, false, 0);
    bar.pack_start(&forward_btn,  false, false, 0);
    bar.pack_start(&reload_btn,   false, false, 0);
    bar.pack_start(&home_btn,     false, false, 4);
    bar.pack_start(url_bar,       true,  true,  0);
    bar.pack_end(&settings_btn,   false, false, 0);
    bar.pack_end(&star_btn,       false, false, 0);
    bar.pack_end(&new_tab_btn,    false, false, 4);

    Navbar { bar, back_btn, forward_btn, reload_btn, home_btn, star_btn, new_tab_btn, settings_btn }
}

fn nav_button(label: &str, tooltip: &str) -> Button {
    let btn = Button::with_label(label);
    btn.style_context().add_class("nyx-nav-btn");
    btn.set_relief(gtk::ReliefStyle::None);
    btn.set_tooltip_text(Some(tooltip));
    btn
}

// ── Câblage ───────────────────────────────────────────────────────────────

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

fn wire_buttons(nb: &Navbar, tabs: &TabBar, url_bar: &Entry, settings: &Settings, bm: &Bookmarks) {
    let t = tabs.clone();
    nb.back_btn.connect_clicked(move |_| t.with_current(|wv| wv.go_back()));
    let t = tabs.clone();
    nb.forward_btn.connect_clicked(move |_| t.with_current(|wv| wv.go_forward()));
    let t = tabs.clone();
    nb.reload_btn.connect_clicked(move |_| t.with_current(|wv| wv.reload()));
    let t = tabs.clone();
    nb.new_tab_btn.connect_clicked(move |_| { t.open_new_tab(); });

    // ── Home → charge l'URL configurée dans les paramètres ────────────
    let t = tabs.clone();
    let s = settings.clone();
    nb.home_btn.connect_clicked(move |_| {
        let url = s.borrow().home_url.clone();
        t.with_current(|wv| load_nyx_or_web(wv, &url));
        if t.current_webview().is_none() { t.open_new_tab(); }
    });

    // ── Settings ─────────────────────────────────────────────────────
    let t = tabs.clone();
    nb.settings_btn.connect_clicked(move |_| { t.open_settings(); });

    // ── Favoris : ajouter + toast dans l'URL bar (1.5 s) ─────────────
    let t = tabs.clone();
    let ub = url_bar.clone();
    let b = bm.clone();
    nb.star_btn.connect_clicked(move |_| {
        if let Some(wv) = t.current_webview() {
            let url   = wv.uri().map(|s| s.to_string()).unwrap_or_default();
            let title = wv.title().map(|s| s.to_string()).unwrap_or_else(|| url.clone());
            b.borrow_mut().push(crate::bookmarks::Bookmark { url: url.clone(), title });
            let ub2 = ub.clone(); let url2 = url.clone();
            ub.set_text("  ★  Favori ajouté");
            gtk::glib::timeout_add_local(
                std::time::Duration::from_millis(1500),
                move || { ub2.set_text(&url2); gtk::glib::ControlFlow::Break },
            );
        }
    });

    // ── URL bar ───────────────────────────────────────────────────────
    let t = tabs.clone();
    let s = settings.clone();
    url_bar.connect_activate(move |entry| {
        let url = webview::resolve_input(&entry.text(), &s.borrow());
        if url.is_empty() { return; }
        entry.set_text(&url);
        t.with_current(|wv| load_nyx_or_web(wv, &url));
    });
}

/// Charge une URL dans une WebView — gère les schémas `nyx://` sans
/// redéclencher la politique de navigation (évite une boucle).
fn load_nyx_or_web(wv: &webkit2gtk::WebView, url: &str) {
    if url == "nyx://newtab" {
        wv.load_html(crate::newtab::html(), Some(&crate::newtab::base_uri()));
    } else if url == "nyx://settings" {
        // Le policy filter prend le relais via wv.load_uri
        wv.load_uri("nyx://settings");
    } else {
        wv.load_uri(url);
    }
}
