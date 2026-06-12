use std::sync::Arc;

use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box as GtkBox, Button, Entry, Notebook,
    Orientation, ProgressBar,
};
use webkit2gtk::{WebView, WebViewExt};

use crate::adblock::AdBlocker;
use crate::tabs::{current_webview, TabBar};
use crate::webview;
use crate::HOME_PAGE;

pub struct BrowserWindow {
    pub window: ApplicationWindow,
    pub tabs: TabBar,
    /// Utilisé par le Sprint 2 (autocomplétion, bouton bookmark ★).
    #[allow(dead_code)]
    pub url_bar: Entry,
}

impl BrowserWindow {
    pub fn new(app: &Application, blocker: Arc<AdBlocker>) -> Self {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("Nyx")
            .default_width(1400)
            .default_height(860)
            .build();

        // ── Progress bar ────────────────────────────────────────────────
        let progress = ProgressBar::new();
        progress.style_context().add_class("nyx-progress");
        progress.set_fraction(0.0);
        progress.set_visible(false);

        // ── Barre de navigation ──────────────────────────────────────────
        let back_btn    = nav_button("◀");
        let forward_btn = nav_button("▶");
        let reload_btn  = nav_button("↺");
        let new_tab_btn = nav_button("+");

        let url_bar = Entry::builder()
            .placeholder_text("nyx://  ou  recherche…")
            .hexpand(true)
            .build();
        url_bar.style_context().add_class("nyx-urlbar");

        let navbar = GtkBox::new(Orientation::Horizontal, 4);
        navbar.style_context().add_class("nyx-navbar");
        navbar.pack_start(&back_btn,    false, false, 0);
        navbar.pack_start(&forward_btn, false, false, 0);
        navbar.pack_start(&reload_btn,  false, false, 4);
        navbar.pack_start(&url_bar,     true,  true,  0);
        navbar.pack_end(&new_tab_btn,   false, false, 4);

        // ── Onglets ──────────────────────────────────────────────────────
        let tabs = TabBar::new(blocker);

        // ── Layout ───────────────────────────────────────────────────────
        let vbox = GtkBox::new(Orientation::Vertical, 0);
        vbox.pack_start(&progress,      false, false, 0);
        vbox.pack_start(&navbar,        false, false, 0);
        vbox.pack_start(&tabs.notebook, true,  true,  0);
        window.add(&vbox);

        // ── Raccourcis clavier ───────────────────────────────────────────
        wire_shortcuts(&window, &tabs, &url_bar);

        // ── Bouton nouvel onglet ─────────────────────────────────────────
        {
            let tb = tabs.clone();
            new_tab_btn.connect_clicked(move |_| { tb.open(HOME_PAGE); });
        }

        // ── URL bar → charger ────────────────────────────────────────────
        {
            let nb = tabs.notebook.clone();
            url_bar.connect_activate(move |entry| {
                let url = webview::resolve_input(&entry.text());
                entry.set_text(&url);
                if let Some(wv) = current_webview(&nb) {
                    wv.load_uri(&url);
                }
            });
        }

        // ── Boutons nav ──────────────────────────────────────────────────
        wire_nav_button(&back_btn,    &tabs.notebook, |wv| wv.go_back());
        wire_nav_button(&forward_btn, &tabs.notebook, |wv| wv.go_forward());
        wire_nav_button(&reload_btn,  &tabs.notebook, |wv| wv.reload());

        // ── Sync URL bar + progress ↔ onglet actif ───────────────────────
        // Les signaux par-WebView sont câblés une seule fois, à la création
        // de la page (`page-added`) — jamais dans `switch-page`, sinon les
        // handlers s'accumulent à chaque changement d'onglet.
        {
            let ub = url_bar.clone();
            let prog = progress.clone();
            tabs.notebook.connect_page_added(move |nb, child, _| {
                let Ok(wv) = child.clone().downcast::<WebView>() else { return };

                let nb2 = nb.clone();
                let ub2 = ub.clone();
                wv.connect_uri_notify(move |w| {
                    if is_current(&nb2, w) {
                        ub2.set_text(&w.uri().unwrap_or_default());
                    }
                });

                let nb3 = nb.clone();
                let prog2 = prog.clone();
                wv.connect_estimated_load_progress_notify(move |w| {
                    if is_current(&nb3, w) {
                        let p = w.estimated_load_progress();
                        prog2.set_fraction(p);
                        prog2.set_visible(p > 0.0 && p < 1.0);
                    }
                });
            });
        }
        {
            // `switch-page` fournit le widget de la page cible — ne pas
            // utiliser current_page() ici, il pointe encore l'ancien onglet.
            let ub = url_bar.clone();
            let prog = progress.clone();
            tabs.notebook.connect_switch_page(move |_, page, _| {
                if let Some(wv) = page.downcast_ref::<WebView>() {
                    ub.set_text(&wv.uri().unwrap_or_default());
                    let p = wv.estimated_load_progress();
                    prog.set_fraction(p);
                    prog.set_visible(p > 0.0 && p < 1.0);
                }
            });
        }

        // ── Dernier onglet fermé → fermer la fenêtre ─────────────────────
        {
            let win = window.clone();
            tabs.notebook.connect_page_removed(move |nb, _, _| {
                if nb.n_pages() == 0 {
                    win.close();
                }
            });
        }

        Self { window, tabs, url_bar }
    }

    pub fn show_all(&self) {
        self.window.show_all();
    }
}

fn nav_button(label: &str) -> Button {
    let btn = Button::with_label(label);
    btn.style_context().add_class("nyx-nav-btn");
    btn.set_relief(gtk::ReliefStyle::None);
    btn
}

fn wire_nav_button(btn: &Button, nb: &Notebook, action: impl Fn(&WebView) + 'static) {
    let nb = nb.clone();
    btn.connect_clicked(move |_| {
        if let Some(wv) = current_webview(&nb) {
            action(&wv);
        }
    });
}

/// true si `wv` est la page actuellement affichée.
fn is_current(nb: &Notebook, wv: &WebView) -> bool {
    nb.current_page().is_some() && nb.current_page() == nb.page_num(wv)
}

fn wire_shortcuts(window: &ApplicationWindow, tabs: &TabBar, url_bar: &Entry) {
    let accel = gtk::AccelGroup::new();
    window.add_accel_group(&accel);

    // Ctrl+L → focus URL bar
    let ub = url_bar.clone();
    add_ctrl_accel(&accel, 'l', move || {
        ub.grab_focus();
        ub.select_region(0, -1);
    });

    // Ctrl+R → reload
    let nb = tabs.notebook.clone();
    add_ctrl_accel(&accel, 'r', move || {
        if let Some(wv) = current_webview(&nb) { wv.reload(); }
    });

    // Ctrl+T → nouvel onglet
    let tb = tabs.clone();
    add_ctrl_accel(&accel, 't', move || { tb.open(HOME_PAGE); });

    // Ctrl+W → fermer onglet
    let tb = tabs.clone();
    add_ctrl_accel(&accel, 'w', move || tb.close_current());
}

fn add_ctrl_accel(accel: &gtk::AccelGroup, key: char, action: impl Fn() + 'static) {
    // Les keysyms GDK des lettres ASCII sont leur code ASCII
    accel.connect_accel_group(
        key as u32,
        gtk::gdk::ModifierType::CONTROL_MASK,
        gtk::AccelFlags::VISIBLE,
        move |_, _, _, _| { action(); true },
    );
}
