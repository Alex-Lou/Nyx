use std::sync::Arc;

use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box as GtkBox, Button, Entry,
    Orientation, ProgressBar,
};
use webkit2gtk::WebViewExt;

use crate::adblock::AdBlocker;
use crate::tabs::TabBar;
use crate::webview;

pub struct BrowserWindow {
    pub window: ApplicationWindow,
    pub tabs: TabBar,
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
        // L'accès à TabBar ici est indirect via le notebook clone
        // (ownership complet dans main.rs via la struct BrowserWindow)
        {
            let nb = tabs.notebook.clone();
            let ub = url_bar.clone();
            let blocker2 = Arc::new(crate::adblock::AdBlocker::new());
            new_tab_btn.connect_clicked(move |_| {
                // Nouvel onglet : DuckDuckGo par défaut
                // TODO Sprint 1.3 : page "new tab" Nyx custom
                // Pour l'instant on passe par un signal global (voir main.rs)
                let _ = &nb; // placeholder — câblage complet dans main.rs
            });
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
        {
            let nb = tabs.notebook.clone();
            back_btn.connect_clicked(move |_| {
                if let Some(wv) = current_webview(&nb) { wv.go_back(); }
            });
        }
        {
            let nb = tabs.notebook.clone();
            forward_btn.connect_clicked(move |_| {
                if let Some(wv) = current_webview(&nb) { wv.go_forward(); }
            });
        }
        {
            let nb = tabs.notebook.clone();
            reload_btn.connect_clicked(move |_| {
                if let Some(wv) = current_webview(&nb) { wv.reload(); }
            });
        }

        // ── Sync URL bar ↔ onglet actif ──────────────────────────────────
        {
            let ub = url_bar.clone();
            let prog = progress.clone();
            tabs.notebook.connect_switch_page(move |nb, _, _| {
                if let Some(wv) = current_webview(nb) {
                    ub.set_text(&wv.uri().unwrap_or_default());

                    let ub2 = ub.clone();
                    wv.connect_uri_notify(move |w| {
                        ub2.set_text(&w.uri().unwrap_or_default());
                    });

                    let prog2 = prog.clone();
                    wv.connect_estimated_load_progress_notify(move |w| {
                        let p = w.estimated_load_progress();
                        prog2.set_fraction(p);
                        prog2.set_visible(p > 0.0 && p < 1.0);
                    });
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

pub fn current_webview(nb: &gtk::Notebook) -> Option<webkit2gtk::WebView> {
    let page = nb.current_page()?;
    nb.nth_page(Some(page))?.downcast::<webkit2gtk::WebView>().ok()
}

fn wire_shortcuts(window: &ApplicationWindow, tabs: &TabBar, url_bar: &Entry) {
    use gtk::gdk::keys::constants as key;

    let accel = gtk::AccelGroup::new();
    window.add_accel_group(&accel);

    // Ctrl+L → focus URL bar
    let ub = url_bar.clone();
    accel.connect_accel_group(
        key::l.into(),
        gtk::gdk::ModifierType::CONTROL_MASK,
        gtk::AccelFlags::VISIBLE,
        move |_, _, _, _| { ub.grab_focus(); ub.select_region(0, -1); true },
    );

    // Ctrl+R → reload
    let nb = tabs.notebook.clone();
    accel.connect_accel_group(
        key::r.into(),
        gtk::gdk::ModifierType::CONTROL_MASK,
        gtk::AccelFlags::VISIBLE,
        move |_, _, _, _| {
            if let Some(wv) = current_webview(&nb) { wv.reload(); }
            true
        },
    );

    // Ctrl+W → fermer onglet
    let nb = tabs.notebook.clone();
    accel.connect_accel_group(
        key::w.into(),
        gtk::gdk::ModifierType::CONTROL_MASK,
        gtk::AccelFlags::VISIBLE,
        move |_, _, _, _| {
            if let Some(page) = nb.current_page() {
                nb.remove_page(Some(page));
            }
            true
        },
    );
}
