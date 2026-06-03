use std::sync::Arc;

use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box as GtkBox, Entry, Orientation, ProgressBar,
};
use webkit2gtk::WebViewExt;

use nyx_core::domain_risk::{self, Risk};

use crate::state::bookmarks::Bookmarks;
use crate::state::downloads::DownloadsHandle;
use crate::state::settings::{LastTab, Settings};
use crate::ui::tabs::TabBar;
use crate::ui::{chrome, navbar, shortcuts, toast};
use crate::web::nyxguard::NyxGuard;

pub struct BrowserWindow {
    pub window: ApplicationWindow,
    pub tabs:   TabBar,
}

impl BrowserWindow {
    pub fn new(
        app: &Application, blocker: Arc<NyxGuard>, settings: Settings,
        bm: Bookmarks, permissions: nyx_core::permissions::PermissionStore,
        downloads: DownloadsHandle,
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

        // UX standard navigateur : clic sur l'URL bar à froid sélectionne
        // tout pour permettre un remplacement direct. focus-in ne fire
        // qu'au gain de focus (re-clic en édition ne déclenche rien).
        // idle_add défère la sélection après le placement du curseur par
        // le click, sinon le clic écraserait la sélection.
        url_bar.connect_focus_in_event(|entry, _| {
            let e = entry.clone();
            gtk::glib::idle_add_local_once(move || {
                e.select_region(0, -1);
            });
            gtk::glib::Propagation::Proceed
        });

        let toaster = toast::new();

        let tabs = TabBar::new(
            blocker, settings.clone(), bm.clone(), permissions,
            downloads.clone(), crate::state::downloads_temp::temp_root(),
            toaster.clone(),
        );
        tabs.set_parent(&window);
        let navbar = navbar::build(&url_bar, &tabs, &settings, &bm, &downloads, &toaster);

        let vbox = GtkBox::new(Orientation::Vertical, 0);
        vbox.pack_start(&progress,      false, false, 0);
        vbox.pack_start(&navbar,        false, false, 0);
        vbox.pack_start(&tabs.notebook, true,  true,  0);

        // Overlay : vbox dessous, toasts dessus (pass_through laisse les
        // clics traverser les zones vides du host).
        let overlay = gtk::Overlay::new();
        overlay.add(&vbox);
        overlay.add_overlay(toaster.widget());
        overlay.set_overlay_pass_through(toaster.widget(), true);
        window.add(&overlay);

        wire_webview_hooks(&tabs, &url_bar, &progress);
        wire_tab_switch(&tabs, &url_bar, &progress);
        wire_last_tab(&window, &tabs, &settings);
        wire_double_click(&tabs);
        shortcuts::wire(&window, &tabs, &url_bar, &bm);

        Self { window, tabs }
    }

    pub fn show_all(&self) { self.window.show_all(); }
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
            && ev.position().1 < 42.0
        {
            t.open_home();
            return gtk::glib::Propagation::Stop;
        }
        gtk::glib::Propagation::Proceed
    });
}

fn wire_webview_hooks(tabs: &TabBar, url_bar: &Entry, progress: &ProgressBar) {
    let ub   = url_bar.clone();
    let prog = progress.clone();
    tabs.set_on_new_webview(move |wv| {
        let ub2 = ub.clone();
        wv.connect_uri_notify(move |w| {
            let uri = w.uri().map(|u| u.to_string()).unwrap_or_default();
            ub2.set_text(&uri);
            apply_risk_indicator(&ub2, &uri);
        });
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
            let uri = wv.uri().map(|u| u.to_string()).unwrap_or_default();
            ub.set_text(&uri);
            apply_risk_indicator(&ub, &uri);
            let p = wv.estimated_load_progress();
            prog.set_fraction(p);
            prog.set_visible(p > 0.0 && p < 1.0);
        }
    });
}
