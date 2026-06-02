use std::sync::Arc;

use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box as GtkBox, Button, Entry,
    Orientation, ProgressBar,
};
use webkit2gtk::WebViewExt;

use crate::adblock::AdBlocker;
use crate::tabs::TabBar;
use crate::{shortcuts, webview};

pub struct BrowserWindow {
    pub window: ApplicationWindow,
    pub tabs:   TabBar,
}

impl BrowserWindow {
    pub fn new(app: &Application, blocker: Arc<AdBlocker>) -> Self {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("Nyx")
            .default_width(1400)
            .default_height(860)
            .build();

        let progress = ProgressBar::new();
        progress.style_context().add_class("nyx-progress");
        progress.set_no_show_all(true);
        progress.set_visible(false);

        let url_bar = Entry::builder()
            .placeholder_text("nyx://  ou  recherche…")
            .hexpand(true)
            .build();
        url_bar.style_context().add_class("nyx-urlbar");

        let navbar = build_navbar(&url_bar);
        let tabs   = TabBar::new(blocker);

        let vbox = GtkBox::new(Orientation::Vertical, 0);
        vbox.pack_start(&progress,      false, false, 0);
        vbox.pack_start(&navbar.bar,    false, false, 0);
        vbox.pack_start(&tabs.notebook, true,  true,  0);
        window.add(&vbox);

        wire_webview_hooks(&tabs, &url_bar, &progress);
        wire_tab_switch(&tabs, &url_bar, &progress);
        wire_navbar_buttons(&navbar, &tabs, &url_bar);
        shortcuts::wire(&window, &tabs, &url_bar);

        Self { window, tabs }
    }

    pub fn show_all(&self) { self.window.show_all(); }
}

// ── Construction de la navbar ─────────────────────────────────────────────

struct Navbar {
    bar:         GtkBox,
    back_btn:    Button,
    forward_btn: Button,
    reload_btn:  Button,
    home_btn:    Button,
    new_tab_btn: Button,
}

fn build_navbar(url_bar: &Entry) -> Navbar {
    let back_btn    = nav_button("◀", "Précédent (Alt+←)");
    let forward_btn = nav_button("▶", "Suivant (Alt+→)");
    let reload_btn  = nav_button("↺", "Recharger (Ctrl+R)");
    let home_btn    = nav_button("⌂", "Accueil — DuckDuckGo (Ctrl+H)");
    let new_tab_btn = nav_button("+", "Nouvel onglet (Ctrl+T)");

    let bar = GtkBox::new(Orientation::Horizontal, 4);
    bar.style_context().add_class("nyx-navbar");
    bar.pack_start(&back_btn,    false, false, 0);
    bar.pack_start(&forward_btn, false, false, 0);
    bar.pack_start(&reload_btn,  false, false, 0);
    bar.pack_start(&home_btn,    false, false, 4);
    bar.pack_start(url_bar,      true,  true,  0);
    bar.pack_end(&new_tab_btn,   false, false, 4);

    Navbar { bar, back_btn, forward_btn, reload_btn, home_btn, new_tab_btn }
}

fn nav_button(label: &str, tooltip: &str) -> Button {
    let btn = Button::with_label(label);
    btn.style_context().add_class("nyx-nav-btn");
    btn.set_relief(gtk::ReliefStyle::None);
    btn.set_tooltip_text(Some(tooltip));
    btn
}

// ── Câblage des signaux ───────────────────────────────────────────────────

/// URL bar + progress câblés une fois par WebView à sa création.
fn wire_webview_hooks(tabs: &TabBar, url_bar: &Entry, progress: &ProgressBar) {
    let ub   = url_bar.clone();
    let prog = progress.clone();
    tabs.set_on_new_webview(move |wv| {
        let ub2 = ub.clone();
        wv.connect_uri_notify(move |w| {
            ub2.set_text(w.uri().as_deref().unwrap_or(""));
        });
        let prog2 = prog.clone();
        wv.connect_estimated_load_progress_notify(move |w| {
            let p = w.estimated_load_progress();
            prog2.set_fraction(p);
            prog2.set_visible(p > 0.0 && p < 1.0);
        });
    });
}

/// Resync URL bar + progress quand l'utilisateur change d'onglet.
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

fn wire_navbar_buttons(navbar: &Navbar, tabs: &TabBar, url_bar: &Entry) {
    let t = tabs.clone();
    navbar.back_btn.connect_clicked(move |_| t.with_current(|wv| wv.go_back()));

    let t = tabs.clone();
    navbar.forward_btn.connect_clicked(move |_| t.with_current(|wv| wv.go_forward()));

    let t = tabs.clone();
    navbar.reload_btn.connect_clicked(move |_| t.with_current(|wv| wv.reload()));

    let t = tabs.clone();
    navbar.home_btn.connect_clicked(move |_| {
        t.with_current(|wv| wv.load_uri(crate::HOME_URL));
    });

    let t = tabs.clone();
    navbar.new_tab_btn.connect_clicked(move |_| { t.open_new_tab(); });

    let t = tabs.clone();
    url_bar.connect_activate(move |entry| {
        let url = webview::resolve_input(&entry.text());
        if url.is_empty() { return; }
        entry.set_text(&url);
        t.with_current(|wv| wv.load_uri(&url));
    });
}
