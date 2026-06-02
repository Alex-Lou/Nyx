use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use gtk::glib;
use gtk::glib::SourceId;
use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box as GtkBox, Entry, EventBox,
    Orientation, Overlay, ProgressBar,
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

        let tabs = TabBar::new(blocker, settings.clone(), bm.clone());
        tabs.set_parent(&window);
        let nav_bar = navbar::build(&url_bar, &tabs, &settings, &bm);

        // ── Chrome flottant : progress + navbar dans un overlay ──────
        let chrome_box = GtkBox::new(Orientation::Vertical, 0);
        chrome_box.style_context().add_class("nyx-chrome");
        chrome_box.pack_start(&progress, false, false, 0);
        chrome_box.pack_start(&nav_bar, false, false, 0);
        chrome_box.set_valign(gtk::Align::Start);

        // Zone de détection hover (couvre le chrome + une marge de 10px en dessous)
        let hover_zone = EventBox::new();
        hover_zone.set_above_child(false);
        hover_zone.add(&chrome_box);
        hover_zone.set_valign(gtk::Align::Start);

        // Layout : notebook plein écran, chrome en overlay par-dessus
        let overlay = Overlay::new();
        overlay.add(&tabs.notebook);
        overlay.add_overlay(&hover_zone);

        let vbox = GtkBox::new(Orientation::Vertical, 0);
        vbox.pack_start(&overlay, true, true, 0);
        window.add(&vbox);

        // Timer de fade-out (4s d'inactivité sur newtab)
        let fade_timer: Rc<Cell<Option<SourceId>>> = Rc::new(Cell::new(None));

        wire_chrome_fade(&hover_zone, &chrome_box, &tabs, &fade_timer);
        wire_webview_hooks(&tabs, &url_bar, &progress, &chrome_box, &fade_timer);
        wire_tab_switch(&tabs, &url_bar, &progress, &chrome_box, &fade_timer);
        wire_last_tab(&window, &tabs, &settings);
        wire_double_click(&tabs);
        shortcuts::wire(&window, &tabs, &url_bar, &bm);

        // Premier onglet = newtab → lancer le timer de fade-out
        {
            let cb = chrome_box.clone();
            let t  = tabs.clone();
            let ft = fade_timer.clone();
            glib::idle_add_local_once(move || { schedule_fade_out(&cb, &t, &ft); });
        }

        Self { window, tabs }
    }

    pub fn show_all(&self) { self.window.show_all(); }
}

fn is_newtab(wv: &WebView) -> bool {
    wv.widget_name().as_str() == "nyx-newtab"
}

fn on_newtab(tabs: &TabBar) -> bool {
    tabs.current_webview().is_some_and(|wv| is_newtab(&wv))
}

/// Cache le chrome (fade out via CSS class).
fn hide_chrome(chrome: &GtkBox, tabs: &TabBar) {
    chrome.style_context().add_class("nyx-hidden");
    tabs.notebook.set_show_tabs(false);
}

/// Montre le chrome (fade in via CSS class).
fn show_chrome(chrome: &GtkBox, tabs: &TabBar) {
    chrome.style_context().remove_class("nyx-hidden");
    tabs.notebook.set_show_tabs(true);
}

/// Annule le timer en cours s'il y en a un.
fn cancel_timer(timer: &Rc<Cell<Option<SourceId>>>) {
    if let Some(id) = timer.take() {
        id.remove();
    }
}

/// Programme un fade-out dans 4 secondes (seulement si on est sur newtab).
fn schedule_fade_out(chrome: &GtkBox, tabs: &TabBar, timer: &Rc<Cell<Option<SourceId>>>) {
    cancel_timer(timer);
    if !on_newtab(tabs) { return; }
    let cb = chrome.clone();
    let t  = tabs.clone();
    let id = glib::timeout_add_local_once(std::time::Duration::from_secs(4), move || {
        if on_newtab(&t) {
            hide_chrome(&cb, &t);
        }
    });
    timer.set(Some(id));
}

fn wire_chrome_fade(
    hover_zone: &EventBox, chrome: &GtkBox, tabs: &TabBar,
    timer: &Rc<Cell<Option<SourceId>>>,
) {
    // Souris entre dans la zone chrome → montrer immédiatement + annuler le timer
    {
        let cb = chrome.clone();
        let t  = tabs.clone();
        let ft = timer.clone();
        hover_zone.connect_enter_notify_event(move |_, _| {
            cancel_timer(&ft);
            if on_newtab(&t) {
                show_chrome(&cb, &t);
            }
            glib::Propagation::Proceed
        });
    }
    // Souris quitte la zone chrome → relancer le timer de 4s
    {
        let cb = chrome.clone();
        let t  = tabs.clone();
        let ft = timer.clone();
        hover_zone.connect_leave_notify_event(move |_, ev| {
            if ev.detail() == gtk::gdk::NotifyType::Inferior {
                return glib::Propagation::Proceed;
            }
            schedule_fade_out(&cb, &t, &ft);
            glib::Propagation::Proceed
        });
    }
}

fn wire_last_tab(window: &ApplicationWindow, tabs: &TabBar, settings: &Settings) {
    let w = window.clone();
    let t = tabs.clone();
    let s = settings.clone();
    tabs.notebook.connect_page_removed(move |nb, _, _| {
        if nb.n_pages() != 0 { return; }
        match s.borrow().on_last_tab {
            LastTab::CloseWindow => w.close(),
            LastTab::Home => {
                let t2 = t.clone();
                glib::idle_add_local_once(move || { t2.open_home(); });
            }
        }
    });
}

fn wire_double_click(tabs: &TabBar) {
    let t = tabs.clone();
    tabs.notebook.connect_button_press_event(move |_nb, ev| {
        if ev.event_type() == gtk::gdk::EventType::DoubleButtonPress
            && ev.button() == 1 && ev.position().1 < 42.0
        {
            t.open_home();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
}

fn wire_webview_hooks(
    tabs: &TabBar, url_bar: &Entry, progress: &ProgressBar,
    chrome: &GtkBox, timer: &Rc<Cell<Option<SourceId>>>,
) {
    let ub   = url_bar.clone();
    let prog = progress.clone();
    let cb   = chrome.clone();
    let t    = tabs.clone();
    let ft   = timer.clone();
    tabs.set_on_new_webview(move |wv| {
        let ub2 = ub.clone();
        let cb2 = cb.clone();
        let t2  = t.clone();
        let ft2 = ft.clone();
        wv.connect_uri_notify(move |w| {
            let uri = w.uri().map(|u| u.to_string()).unwrap_or_default();
            ub2.set_text(&uri);
            // On efface le tag newtab seulement à la navigation vers une URL externe.
            // (file:// vise nos assets, on garde le tag.)
            if w.widget_name().as_str() == "nyx-newtab"
                && (uri.starts_with("http://") || uri.starts_with("https://"))
            {
                w.set_widget_name("");
                show_chrome(&cb2, &t2);
                cancel_timer(&ft2);
            }
        });
        let prog2 = prog.clone();
        wv.connect_estimated_load_progress_notify(move |w| {
            let p = w.estimated_load_progress();
            prog2.set_fraction(p);
            prog2.set_visible(p > 0.0 && p < 1.0);
        });
        let cb3 = cb.clone();
        let t3  = t.clone();
        let ft3 = ft.clone();
        wv.connect_load_changed(move |_, _| {
            if on_newtab(&t3) {
                schedule_fade_out(&cb3, &t3, &ft3);
            } else {
                show_chrome(&cb3, &t3);
                cancel_timer(&ft3);
            }
        });
    });
}

fn wire_tab_switch(
    tabs: &TabBar, url_bar: &Entry, progress: &ProgressBar,
    chrome: &GtkBox, timer: &Rc<Cell<Option<SourceId>>>,
) {
    let ub   = url_bar.clone();
    let prog = progress.clone();
    let cb   = chrome.clone();
    let t    = tabs.clone();
    let ft   = timer.clone();
    tabs.notebook.connect_switch_page(move |_nb, page, _| {
        if let Ok(wv) = page.clone().downcast::<webkit2gtk::WebView>() {
            ub.set_text(wv.uri().as_deref().unwrap_or(""));
            let p = wv.estimated_load_progress();
            prog.set_fraction(p);
            prog.set_visible(p > 0.0 && p < 1.0);
        }
        if on_newtab(&t) {
            schedule_fade_out(&cb, &t, &ft);
        } else {
            show_chrome(&cb, &t);
            cancel_timer(&ft);
        }
    });
}
