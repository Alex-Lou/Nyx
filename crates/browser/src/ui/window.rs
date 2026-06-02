use std::cell::Cell;
use std::rc::Rc;
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

        let revealer = Revealer::builder()
            .transition_type(RevealerTransitionType::SlideDown)
            .transition_duration(250)
            .reveal_child(true)
            .build();
        revealer.add(&nav_bar);

        let chrome_hidden = Rc::new(Cell::new(false));

        let hover_zone = EventBox::new();
        hover_zone.set_above_child(true);
        hover_zone.set_visible_window(false);
        hover_zone.set_size_request(-1, 10);

        let content_box = GtkBox::new(Orientation::Vertical, 0);
        content_box.pack_start(&tabs.notebook, true, true, 0);

        let overlay = Overlay::new();
        overlay.add(&content_box);

        let hover_box = GtkBox::new(Orientation::Vertical, 0);
        hover_box.pack_start(&hover_zone, false, false, 0);
        hover_box.set_valign(gtk::Align::Start);
        overlay.add_overlay(&hover_box);

        let vbox = GtkBox::new(Orientation::Vertical, 0);
        vbox.pack_start(&progress, false, false, 0);
        vbox.pack_start(&revealer, false, false, 0);
        vbox.pack_start(&overlay,  true,  true,  0);
        window.add(&vbox);

        wire_chrome_autohide(&revealer, &hover_zone, &nav_bar, &tabs, &chrome_hidden);
        wire_webview_hooks(&tabs, &url_bar, &progress, &revealer, &chrome_hidden);
        wire_tab_switch(&tabs, &url_bar, &progress, &revealer, &chrome_hidden);
        wire_last_tab(&window, &tabs, &settings);
        wire_double_click(&tabs);
        shortcuts::wire(&window, &tabs, &url_bar, &bm);

        // Le premier onglet est toujours un newtab → cacher le chrome au démarrage.
        {
            let r = revealer.clone();
            let t = tabs.clone();
            let h = chrome_hidden.clone();
            gtk::glib::idle_add_local_once(move || {
                sync_chrome(&r, &t, &h);
            });
        }

        Self { window, tabs }
    }

    pub fn show_all(&self) { self.window.show_all(); }
}

fn is_newtab(wv: &WebView) -> bool {
    wv.widget_name().as_str() == "nyx-newtab"
}

fn set_chrome_visible(revealer: &Revealer, tabs: &TabBar, visible: bool, hidden: &Rc<Cell<bool>>) {
    revealer.set_reveal_child(visible);
    tabs.notebook.set_show_tabs(visible);
    hidden.set(!visible);
}

fn sync_chrome(revealer: &Revealer, tabs: &TabBar, hidden: &Rc<Cell<bool>>) {
    let on_newtab = tabs.current_webview().is_some_and(|wv| is_newtab(&wv));
    set_chrome_visible(revealer, tabs, !on_newtab, hidden);
}

fn wire_chrome_autohide(
    revealer: &Revealer, hover_zone: &EventBox, nav_bar: &GtkBox,
    tabs: &TabBar, hidden: &Rc<Cell<bool>>,
) {
    {
        let r = revealer.clone();
        let t = tabs.clone();
        let h = hidden.clone();
        hover_zone.connect_enter_notify_event(move |_, _| {
            if h.get() {
                set_chrome_visible(&r, &t, true, &h);
            }
            gtk::glib::Propagation::Proceed
        });
    }
    {
        let r = revealer.clone();
        let t = tabs.clone();
        let h = hidden.clone();
        nav_bar.connect_leave_notify_event(move |widget, ev| {
            if ev.detail() == gtk::gdk::NotifyType::Inferior {
                return gtk::glib::Propagation::Proceed;
            }
            let (_, ey) = ev.position();
            let alloc = widget.allocation();
            if ey >= 0.0 && ey <= alloc.height() as f64 {
                return gtk::glib::Propagation::Proceed;
            }
            if t.current_webview().is_some_and(|wv| is_newtab(&wv)) {
                set_chrome_visible(&r, &t, false, &h);
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
        if nb.n_pages() != 0 { return; }
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
            && ev.button() == 1 && ev.position().1 < 42.0
        {
            t.open_home();
            return gtk::glib::Propagation::Stop;
        }
        gtk::glib::Propagation::Proceed
    });
}

fn wire_webview_hooks(
    tabs: &TabBar, url_bar: &Entry, progress: &ProgressBar,
    revealer: &Revealer, hidden: &Rc<Cell<bool>>,
) {
    let ub   = url_bar.clone();
    let prog = progress.clone();
    let rev  = revealer.clone();
    let t    = tabs.clone();
    let h    = hidden.clone();
    tabs.set_on_new_webview(move |wv| {
        let ub2 = ub.clone();
        let rev2 = rev.clone();
        let t2   = t.clone();
        let h2   = h.clone();
        wv.connect_uri_notify(move |w| {
            let uri = w.uri().map(|u| u.to_string()).unwrap_or_default();
            ub2.set_text(&uri);
            // Quand on navigue hors du newtab, effacer le tag.
            if w.widget_name().as_str() == "nyx-newtab"
                && !uri.is_empty()
                && !uri.starts_with("nyx://newtab")
                && !(uri.starts_with("file://") && uri.contains("newtab"))
            {
                w.set_widget_name("");
                sync_chrome(&rev2, &t2, &h2);
            }
        });
        let prog2 = prog.clone();
        wv.connect_estimated_load_progress_notify(move |w| {
            let p = w.estimated_load_progress();
            prog2.set_fraction(p);
            prog2.set_visible(p > 0.0 && p < 1.0);
        });
        let rev3 = rev.clone();
        let t3   = t.clone();
        let h3   = h.clone();
        wv.connect_load_changed(move |_, _| {
            sync_chrome(&rev3, &t3, &h3);
        });
    });
}

fn wire_tab_switch(
    tabs: &TabBar, url_bar: &Entry, progress: &ProgressBar,
    revealer: &Revealer, hidden: &Rc<Cell<bool>>,
) {
    let ub   = url_bar.clone();
    let prog = progress.clone();
    let rev  = revealer.clone();
    let t    = tabs.clone();
    let h    = hidden.clone();
    tabs.notebook.connect_switch_page(move |_nb, page, _| {
        if let Ok(wv) = page.clone().downcast::<webkit2gtk::WebView>() {
            ub.set_text(wv.uri().as_deref().unwrap_or(""));
            let p = wv.estimated_load_progress();
            prog.set_fraction(p);
            prog.set_visible(p > 0.0 && p < 1.0);
        }
        sync_chrome(&rev, &t, &h);
    });
}
