use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use gtk::glib;
use gtk::glib::SourceId;
use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box as GtkBox, Entry, Orientation,
    ProgressBar, Revealer, RevealerTransitionType,
};
use webkit2gtk::{WebView, WebViewExt};

use nyx_core::domain_risk::{self, Risk};

use crate::state::bookmarks::Bookmarks;
use crate::state::settings::{LastTab, Settings};
use crate::ui::tabs::TabBar;
use crate::ui::{chrome, navbar, shortcuts};
use crate::web::nyxguard::NyxGuard;

/// Combien de pixels en haut de l'écran déclenchent la réapparition du chrome.
const HOVER_ZONE_PX: f64 = 12.0;
/// Délai d'inactivité avant fade-out (newtab uniquement).
const FADE_DELAY_MS: u32 = 4000;

pub struct BrowserWindow {
    pub window: ApplicationWindow,
    pub tabs:   TabBar,
}

impl BrowserWindow {
    pub fn new(
        app: &Application, blocker: Arc<NyxGuard>, settings: Settings,
        bm: Bookmarks, permissions: nyx_core::permissions::PermissionStore,
    ) -> Self {
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

        let tabs = TabBar::new(blocker, settings.clone(), bm.clone(), permissions);
        tabs.set_parent(&window);
        let nav_bar = navbar::build(&url_bar, &tabs, &settings, &bm);

        // Chrome flottant = progress + navbar. Englobé dans un Revealer pour
        // un slide-down/up smooth ; opacité animée en plus via CSS.
        let chrome_box = GtkBox::new(Orientation::Vertical, 0);
        chrome_box.style_context().add_class("nyx-chrome");
        chrome_box.pack_start(&progress, false, false, 0);
        chrome_box.pack_start(&nav_bar, false, false, 0);

        let revealer = Revealer::builder()
            .transition_type(RevealerTransitionType::SlideDown)
            .transition_duration(800)
            .reveal_child(true)
            .build();
        revealer.add(&chrome_box);

        // Layout simple — pas d'overlay : Revealer hidden = hauteur 0,
        // le notebook prend tout l'écran. On capte les déplacements souris
        // au niveau de la fenêtre pour réafficher quand le curseur monte.
        let vbox = GtkBox::new(Orientation::Vertical, 0);
        vbox.pack_start(&revealer, false, false, 0);
        vbox.pack_start(&tabs.notebook, true, true, 0);
        window.add(&vbox);

        let fade_timer: Rc<Cell<Option<SourceId>>> = Rc::new(Cell::new(None));
        let chrome_hidden = Rc::new(Cell::new(false));

        wire_window_motion(&window, &revealer, &chrome_box, &tabs, &chrome_hidden, &fade_timer);
        wire_webview_hooks(&tabs, &url_bar, &progress, &revealer, &chrome_box, &chrome_hidden, &fade_timer);
        wire_tab_switch(&tabs, &url_bar, &progress, &revealer, &chrome_box, &chrome_hidden, &fade_timer);
        wire_last_tab(&window, &tabs, &settings);
        wire_double_click(&tabs);
        shortcuts::wire(&window, &tabs, &url_bar, &bm);

        // Au démarrage : c'est un newtab → on lance le timer de fade-out.
        {
            let cb = chrome_box.clone();
            let r  = revealer.clone();
            let t  = tabs.clone();
            let h  = chrome_hidden.clone();
            let ft = fade_timer.clone();
            glib::idle_add_local_once(move || {
                schedule_fade_out(&r, &cb, &t, &h, &ft);
            });
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

/// Cache le chrome : opacity CSS pour le fade + revealer pour libérer l'espace.
fn hide_chrome(revealer: &Revealer, chrome: &GtkBox, tabs: &TabBar, hidden: &Rc<Cell<bool>>) {
    chrome.style_context().add_class("nyx-hidden");
    revealer.set_reveal_child(false);
    tabs.notebook.set_show_tabs(false);
    hidden.set(true);
}

/// Montre le chrome : retire la classe + revealer ouvre l'espace.
fn show_chrome(revealer: &Revealer, chrome: &GtkBox, tabs: &TabBar, hidden: &Rc<Cell<bool>>) {
    chrome.style_context().remove_class("nyx-hidden");
    revealer.set_reveal_child(true);
    tabs.notebook.set_show_tabs(true);
    hidden.set(false);
}

fn cancel_timer(timer: &Rc<Cell<Option<SourceId>>>) {
    if let Some(id) = timer.take() {
        id.remove();
    }
}

/// Programme un fade-out dans `FADE_DELAY_MS` ms (newtab uniquement).
fn schedule_fade_out(
    revealer: &Revealer, chrome: &GtkBox, tabs: &TabBar,
    hidden: &Rc<Cell<bool>>, timer: &Rc<Cell<Option<SourceId>>>,
) {
    cancel_timer(timer);
    if !on_newtab(tabs) { return; }
    let r  = revealer.clone();
    let cb = chrome.clone();
    let t  = tabs.clone();
    let h  = hidden.clone();
    let ft = timer.clone();
    let id = glib::timeout_add_local_once(
        std::time::Duration::from_millis(FADE_DELAY_MS as u64),
        move || {
            ft.set(None);
            if on_newtab(&t) {
                hide_chrome(&r, &cb, &t, &h);
            }
        },
    );
    timer.set(Some(id));
}

/// Surveille la position souris sur la fenêtre : si elle monte dans la zone
/// haute, on rappelle le chrome. Sinon, on relance le timer de fade-out.
fn wire_window_motion(
    window: &ApplicationWindow, revealer: &Revealer, chrome: &GtkBox,
    tabs: &TabBar, hidden: &Rc<Cell<bool>>, timer: &Rc<Cell<Option<SourceId>>>,
) {
    window.add_events(gtk::gdk::EventMask::POINTER_MOTION_MASK);
    let r  = revealer.clone();
    let cb = chrome.clone();
    let t  = tabs.clone();
    let h  = hidden.clone();
    let ft = timer.clone();
    window.connect_motion_notify_event(move |_, ev| {
        let (_, y) = ev.position();
        if y <= HOVER_ZONE_PX {
            if h.get() {
                show_chrome(&r, &cb, &t, &h);
            }
            cancel_timer(&ft);
        } else if !h.get() && on_newtab(&t) && ft.take().is_none() {
            // Sorti de la zone haute → relance le timer (si pas déjà en cours).
            schedule_fade_out(&r, &cb, &t, &h, &ft);
        } else if let Some(id) = ft.take() {
            ft.set(Some(id)); // remet en place (take/set pour peek)
        }
        glib::Propagation::Proceed
    });
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

/// Pose une classe CSS + tooltip sur l'URL bar selon l'analyse de risque
/// du domaine courant. Pure UI — la politique vit dans nyx-core::domain_risk.
fn apply_risk_indicator(url_bar: &Entry, uri: &str) {
    let ctx = url_bar.style_context();
    ctx.remove_class("nyx-risk-suspicious");
    ctx.remove_class("nyx-risk-dangerous");
    url_bar.set_tooltip_text(None);

    let Some(a) = domain_risk::analyze_url(uri) else { return };
    match a.risk {
        Risk::Safe => {}
        Risk::Suspicious => {
            ctx.add_class("nyx-risk-suspicious");
            url_bar.set_tooltip_text(Some(&format!(
                "Domaine international\n{} (ASCII : {})",
                a.unicode_host, a.ascii_host
            )));
        }
        Risk::Dangerous => {
            ctx.add_class("nyx-risk-dangerous");
            url_bar.set_tooltip_text(Some(&format!(
                "⚠ Domaine suspect — imitation possible\n{} → {}",
                a.unicode_host, a.ascii_host
            )));
        }
    }
}

fn wire_webview_hooks(
    tabs: &TabBar, url_bar: &Entry, progress: &ProgressBar,
    revealer: &Revealer, chrome: &GtkBox,
    hidden: &Rc<Cell<bool>>, timer: &Rc<Cell<Option<SourceId>>>,
) {
    let ub   = url_bar.clone();
    let prog = progress.clone();
    let rev  = revealer.clone();
    let cb   = chrome.clone();
    let t    = tabs.clone();
    let h    = hidden.clone();
    let ft   = timer.clone();
    tabs.set_on_new_webview(move |wv| {
        let ub2 = ub.clone();
        let rev2 = rev.clone();
        let cb2  = cb.clone();
        let t2   = t.clone();
        let h2   = h.clone();
        let ft2  = ft.clone();
        wv.connect_uri_notify(move |w| {
            let uri = w.uri().map(|u| u.to_string()).unwrap_or_default();
            ub2.set_text(&uri);
            apply_risk_indicator(&ub2, &uri);
            if w.widget_name().as_str() == "nyx-newtab"
                && (uri.starts_with("http://") || uri.starts_with("https://"))
            {
                w.set_widget_name("");
                show_chrome(&rev2, &cb2, &t2, &h2);
                cancel_timer(&ft2);
            }
        });
        let prog2 = prog.clone();
        wv.connect_estimated_load_progress_notify(move |w| {
            let p = w.estimated_load_progress();
            prog2.set_fraction(p);
            prog2.set_visible(p > 0.0 && p < 1.0);
        });
        let rev3 = rev.clone();
        let cb3  = cb.clone();
        let t3   = t.clone();
        let h3   = h.clone();
        let ft3  = ft.clone();
        wv.connect_load_changed(move |_, _| {
            if on_newtab(&t3) {
                schedule_fade_out(&rev3, &cb3, &t3, &h3, &ft3);
            } else {
                show_chrome(&rev3, &cb3, &t3, &h3);
                cancel_timer(&ft3);
            }
        });
    });
}

fn wire_tab_switch(
    tabs: &TabBar, url_bar: &Entry, progress: &ProgressBar,
    revealer: &Revealer, chrome: &GtkBox,
    hidden: &Rc<Cell<bool>>, timer: &Rc<Cell<Option<SourceId>>>,
) {
    let ub   = url_bar.clone();
    let prog = progress.clone();
    let rev  = revealer.clone();
    let cb   = chrome.clone();
    let t    = tabs.clone();
    let h    = hidden.clone();
    let ft   = timer.clone();
    tabs.notebook.connect_switch_page(move |_nb, page, _| {
        if let Ok(wv) = page.clone().downcast::<webkit2gtk::WebView>() {
            let uri = wv.uri().map(|u| u.to_string()).unwrap_or_default();
            ub.set_text(&uri);
            apply_risk_indicator(&ub, &uri);
            let p = wv.estimated_load_progress();
            prog.set_fraction(p);
            prog.set_visible(p > 0.0 && p < 1.0);
        }
        if on_newtab(&t) {
            schedule_fade_out(&rev, &cb, &t, &h, &ft);
        } else {
            show_chrome(&rev, &cb, &t, &h);
            cancel_timer(&ft);
        }
    });
}
