use std::sync::Arc;

use gtk::glib::translate::IntoGlib;
use gtk::prelude::*;
use gtk::{
    AccelFlags, AccelGroup, Application, ApplicationWindow, Box as GtkBox,
    Button, Entry, Orientation, ProgressBar,
};
use webkit2gtk::WebViewExt;

use crate::adblock::AdBlocker;
use crate::tabs::TabBar;
use crate::webview;

pub struct BrowserWindow {
    pub window:   ApplicationWindow,
    pub tabs:     TabBar,
    pub url_bar:  Entry,
    pub progress: ProgressBar,
}

impl BrowserWindow {
    pub fn new(app: &Application, blocker: Arc<AdBlocker>) -> Self {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("Nyx")
            .default_width(1400)
            .default_height(860)
            .build();

        // ── Progress bar (masquée hors chargement) ──────────────────────
        let progress = ProgressBar::new();
        progress.style_context().add_class("nyx-progress");
        progress.set_fraction(0.0);
        progress.set_no_show_all(true);
        progress.set_visible(false);

        // ── Navbar ──────────────────────────────────────────────────────
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

        // ── Onglets ─────────────────────────────────────────────────────
        let tabs = TabBar::new(blocker);

        // ── Layout ──────────────────────────────────────────────────────
        let vbox = GtkBox::new(Orientation::Vertical, 0);
        vbox.pack_start(&progress,      false, false, 0);
        vbox.pack_start(&navbar,        false, false, 0);
        vbox.pack_start(&tabs.notebook, true,  true,  0);
        window.add(&vbox);

        // ── Chaque WebView neuve câble URL bar + progress (1.5) ─────────
        {
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

        // ── Resync URL bar + progress quand on change d'onglet ──────────
        {
            let ub   = url_bar.clone();
            let prog = progress.clone();
            tabs.notebook.connect_switch_page(move |_nb, page, _idx| {
                if let Ok(wv) = page.clone().downcast::<webkit2gtk::WebView>() {
                    ub.set_text(wv.uri().as_deref().unwrap_or(""));
                    let p = wv.estimated_load_progress();
                    prog.set_fraction(p);
                    prog.set_visible(p > 0.0 && p < 1.0);
                }
            });
        }

        // ── Bouton "+" → nouvel onglet Nyx ──────────────────────────────
        {
            let t = tabs.clone();
            new_tab_btn.connect_clicked(move |_| {
                t.open_new_tab();
            });
        }

        // ── URL bar → charger ───────────────────────────────────────────
        {
            let t = tabs.clone();
            url_bar.connect_activate(move |entry| {
                let url = webview::resolve_input(&entry.text());
                if url.is_empty() {
                    return;
                }
                entry.set_text(&url);
                if let Some(wv) = t.current_webview() {
                    wv.load_uri(&url);
                }
            });
        }

        // ── Boutons navigation ──────────────────────────────────────────
        {
            let t = tabs.clone();
            back_btn.connect_clicked(move |_| {
                if let Some(wv) = t.current_webview() {
                    wv.go_back();
                }
            });
        }
        {
            let t = tabs.clone();
            forward_btn.connect_clicked(move |_| {
                if let Some(wv) = t.current_webview() {
                    wv.go_forward();
                }
            });
        }
        {
            let t = tabs.clone();
            reload_btn.connect_clicked(move |_| {
                if let Some(wv) = t.current_webview() {
                    wv.reload();
                }
            });
        }

        // ── Raccourcis clavier ──────────────────────────────────────────
        // Ctrl+L, Ctrl+R, Ctrl+W ici ; Ctrl+T & Ctrl+Tab dans ticket 1.4.
        wire_shortcuts(&window, &tabs, &url_bar);

        Self { window, tabs, url_bar, progress }
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

fn wire_shortcuts(window: &ApplicationWindow, tabs: &TabBar, url_bar: &Entry) {
    use gtk::gdk::keys::constants as key;
    use gtk::gdk::ModifierType;

    let accel = AccelGroup::new();
    window.add_accel_group(&accel);

    let ctrl  = ModifierType::CONTROL_MASK;
    let flags = AccelFlags::VISIBLE;

    // Ctrl+L → focus URL bar
    {
        let ub = url_bar.clone();
        accel.connect_accel_group(key::l.into_glib(), ctrl, flags, move |_, _, _, _| {
            ub.grab_focus();
            ub.select_region(0, -1);
            true
        });
    }

    // Ctrl+R → reload
    {
        let t = tabs.clone();
        accel.connect_accel_group(key::r.into_glib(), ctrl, flags, move |_, _, _, _| {
            if let Some(wv) = t.current_webview() {
                wv.reload();
            }
            true
        });
    }

    // Ctrl+W → fermer onglet
    {
        let t = tabs.clone();
        accel.connect_accel_group(key::w.into_glib(), ctrl, flags, move |_, _, _, _| {
            t.close_current();
            true
        });
    }
}
