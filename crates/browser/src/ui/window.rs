use std::sync::Arc;

use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box as GtkBox, Entry, EventBox,
    Orientation, Overlay, ProgressBar, Revealer, RevealerTransitionType,
};
use webkit2gtk::{WebView, WebViewExt};

use crate::state::bookmarks::Bookmarks;
use crate::state::settings::{LastTab, Settings};
use crate::ui::tabs::TabBar;
use crate::ui::{chrome, navbar, shortcuts};
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

        chrome::apply_titlebar(&window, "Nyx");

        let progress = ProgressBar::new();
        progress.style_context().add_class("nyx-progress");
        progress.set_no_show_all(true);
        progress.set_visible(false);

        let url_bar = Entry::builder()
            .placeholder_text("nyx://  ou  recherche…")
            .hexpand(true).build();
        url_bar.style_context().add_class("nyx-urlbar");

        let tabs   = TabBar::new(blocker, settings.clone(), bm.clone());
        tabs.set_parent(&window);
        let nav_bar = navbar::build(&url_bar, &tabs, &settings, &bm);

        // ── Revealer : la navbar glisse depuis le haut ──────────────
        let revealer = Revealer::builder()
            .transition_type(RevealerTransitionType::SlideDown)
            .transition_duration(220)
            .reveal_child(true)
            .build();
        revealer.add(&nav_bar);

        // ── Zone de hover invisible en haut (8px) ───────────────────
        let hover_zone = EventBox::new();
        hover_zone.set_above_child(true);
        hover_zone.set_visible_window(false);
        hover_zone.set_size_request(-1, 8);
        hover_zone.style_context().add_class("nyx-hover-zone");

        // Conteneur principal : overlay pour poser la hover zone par-dessus
        let content_box = GtkBox::new(Orientation::Vertical, 0);
        content_box.pack_start(&tabs.notebook, true, true, 0);

        let overlay = Overlay::new();
        overlay.add(&content_box);

        // La hover zone est dans un GtkBox en haut de l'overlay
        let hover_box = GtkBox::new(Orientation::Vertical, 0);
        hover_box.pack_start(&hover_zone, false, false, 0);
        hover_box.set_valign(gtk::Align::Start);
        overlay.add_overlay(&hover_box);

        let vbox = GtkBox::new(Orientation::Vertical, 0);
        vbox.pack_start(&progress, false, false, 0);
        vbox.pack_start(&revealer, false, false, 0);
        vbox.pack_start(&overlay,  true,  true,  0);
        window.add(&vbox);

        // ── Logique hover : montre/cache selon page + curseur ───────
        wire_navbar_autohide(&revealer, &hover_zone, &nav_bar, &tabs);
        wire_webview_hooks(&tabs, &url_bar, &progress, &revealer);
        wire_tab_switch(&tabs, &url_bar, &progress, &revealer);
        wire_last_tab(&window, &tabs, &settings);
        wire_double_click(&tabs);
        shortcuts::wire(&window, &tabs, &url_bar, &bm);

        Self { window, tabs }
    }

    pub fn show_all(&self) { self.window.show_all(); }
}

/// Retourne `true` si la WebView affiche la page d'accueil (newtab).
fn is_newtab(wv: &WebView) -> bool {
    match wv.uri() {
        Some(u) => {
            let u = u.as_str();
            u.is_empty()
                || u.starts_with("nyx://newtab")
                || (u.starts_with("file://") && u.contains("/assets/") && u.contains("newtab"))
        }
        None => true,
    }
}

/// Synchronise le revealer : caché sur newtab, visible sur les autres pages.
fn sync_navbar(revealer: &Revealer, tabs: &TabBar) {
    let hide = tabs.current_webview().is_some_and(|wv| is_newtab(&wv));
    if hide {
        revealer.set_reveal_child(false);
    } else {
        revealer.set_reveal_child(true);
    }
}

/// Câble le hover sur la zone invisible pour révéler la navbar sur newtab.
fn wire_navbar_autohide(revealer: &Revealer, hover_zone: &EventBox, nav_bar: &GtkBox, tabs: &TabBar) {
    // Souris entre dans la zone de hover → montrer la navbar
    {
        let r = revealer.clone();
        hover_zone.connect_enter_notify_event(move |_, _| {
            r.set_reveal_child(true);
            gtk::glib::Propagation::Proceed
        });
    }
    // Souris quitte la navbar → cacher si on est sur newtab
    {
        let r = revealer.clone();
        let t = tabs.clone();
        nav_bar.connect_leave_notify_event(move |widget, ev| {
            // Ignorer les leave causés par les enfants (boutons, entry)
            if ev.detail() == gtk::gdk::NotifyType::Inferior {
                return gtk::glib::Propagation::Proceed;
            }
            // Vérifier qu'on est vraiment sorti de la zone du widget
            let (_, ey) = ev.position();
            let alloc = widget.allocation();
            if ey >= 0.0 && ey <= alloc.height() as f64 {
                return gtk::glib::Propagation::Proceed;
            }
            if t.current_webview().is_some_and(|wv| is_newtab(&wv)) {
                r.set_reveal_child(false);
            }
            gtk::glib::Propagation::Proceed
        });
    }
}

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
                let t2 = t.clone();
                gtk::glib::idle_add_local_once(move || { t2.open_home(); });
            }
        }
    });
}

fn wire_double_click(tabs: &TabBar) {
    let t = tabs.clone();
    tabs.notebook.connect_button_press_event(move |_nb, ev| {
        if ev.event_type() == gtk::gdk::EventType::DoubleButtonPress
            && ev.button() == 1
            && ev.position().1 < 42.0
        {
            t.open_home();
            return gtk::glib::Propagation::Stop;
        }
        gtk::glib::Propagation::Proceed
    });
}

fn wire_webview_hooks(tabs: &TabBar, url_bar: &Entry, progress: &ProgressBar, revealer: &Revealer) {
    let ub   = url_bar.clone();
    let prog = progress.clone();
    let rev  = revealer.clone();
    let t    = tabs.clone();
    tabs.set_on_new_webview(move |wv| {
        let ub2 = ub.clone();
        wv.connect_uri_notify(move |w| { ub2.set_text(w.uri().as_deref().unwrap_or("")); });
        let prog2 = prog.clone();
        wv.connect_estimated_load_progress_notify(move |w| {
            let p = w.estimated_load_progress();
            prog2.set_fraction(p);
            prog2.set_visible(p > 0.0 && p < 1.0);
        });
        // Quand une page finit de charger → sync navbar visibility
        let rev2 = rev.clone();
        let t2   = t.clone();
        wv.connect_load_changed(move |_, _| {
            sync_navbar(&rev2, &t2);
        });
    });
}

fn wire_tab_switch(tabs: &TabBar, url_bar: &Entry, progress: &ProgressBar, revealer: &Revealer) {
    let ub   = url_bar.clone();
    let prog = progress.clone();
    let rev  = revealer.clone();
    let t    = tabs.clone();
    tabs.notebook.connect_switch_page(move |_nb, page, _| {
        if let Ok(wv) = page.clone().downcast::<webkit2gtk::WebView>() {
            ub.set_text(wv.uri().as_deref().unwrap_or(""));
            let p = wv.estimated_load_progress();
            prog.set_fraction(p);
            prog.set_visible(p > 0.0 && p < 1.0);
        }
        sync_navbar(&rev, &t);
    });
}
