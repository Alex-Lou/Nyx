use std::rc::Rc;

use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box as GtkBox, Button, Entry, EntryCompletion,
    ListStore, Notebook, Orientation, ProgressBar,
};
use vault::{Bookmark, Vault};
use webkit2gtk::{LoadEvent, WebView, WebViewExt};

use crate::adblock::AdBlocker;
use crate::downloads;
use crate::findbar::FindBar;
use crate::reader;
use crate::sidebar::Sidebar;
use crate::tabs::{current_webview, TabBar};
use crate::webview;
use crate::HOME_PAGE;

pub struct BrowserWindow {
    pub window: ApplicationWindow,
    pub tabs: TabBar,
}

impl BrowserWindow {
    pub fn new(app: &Application, blocker: Rc<AdBlocker>, vault: Rc<Vault>) -> Self {
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
        let sidebar_btn = nav_button("☰");
        let back_btn    = nav_button("◀");
        let forward_btn = nav_button("▶");
        let reload_btn  = nav_button("↺");
        let home_btn    = nav_button("⌂");
        let reader_btn  = nav_button("Aa");
        let shield_btn  = nav_button("⛨");
        let star_btn    = nav_button("★");
        let new_tab_btn = nav_button("+");
        star_btn.style_context().add_class("nyx-star-btn");
        shield_btn.style_context().add_class("nyx-shield-btn");
        shield_btn.set_tooltip_text(Some("Bloqueur de pubs actif — cliquer pour whitelister ce site"));
        reader_btn.set_tooltip_text(Some("Mode lecture (re-cliquer pour sortir)"));

        let url_bar = Entry::builder()
            .placeholder_text("nyx://  ou  recherche…")
            .hexpand(true)
            .build();
        url_bar.style_context().add_class("nyx-urlbar");

        let navbar = GtkBox::new(Orientation::Horizontal, 4);
        navbar.style_context().add_class("nyx-navbar");
        navbar.pack_start(&sidebar_btn, false, false, 0);
        navbar.pack_start(&back_btn,    false, false, 0);
        navbar.pack_start(&forward_btn, false, false, 0);
        navbar.pack_start(&reload_btn,  false, false, 4);
        navbar.pack_start(&home_btn,    false, false, 0);
        navbar.pack_start(&url_bar,     true,  true,  0);
        navbar.pack_end(&new_tab_btn,   false, false, 4);
        navbar.pack_end(&star_btn,      false, false, 0);
        navbar.pack_end(&shield_btn,    false, false, 0);
        navbar.pack_end(&reader_btn,    false, false, 0);

        // ── Onglets + sidebar ────────────────────────────────────────────
        // Whitelist adblock persistée dans le vault (Sprint 3.4)
        if let Ok(Some(json)) = vault.setting("adblock_whitelist") {
            let domains: Vec<String> = serde_json::from_str(&json).unwrap_or_default();
            blocker.load_whitelist(domains);
        }

        let tabs = TabBar::new(blocker.clone());
        let sidebar = Sidebar::new(vault.clone(), tabs.clone());
        let findbar = FindBar::new(&tabs.notebook);

        let content = GtkBox::new(Orientation::Horizontal, 0);
        content.pack_start(&sidebar.widget, false, false, 0);
        content.pack_start(&tabs.notebook,  true,  true,  0);

        // ── Layout ───────────────────────────────────────────────────────
        let vbox = GtkBox::new(Orientation::Vertical, 0);
        vbox.pack_start(&progress,       false, false, 0);
        vbox.pack_start(&navbar,         false, false, 0);
        vbox.pack_start(&findbar.widget, false, false, 0);
        vbox.pack_start(&content,        true,  true,  0);
        downloads::init(&vbox); // barre d'état téléchargements (Sprint 4.3)
        window.add(&vbox);

        // ── Raccourcis clavier ───────────────────────────────────────────
        wire_shortcuts(&window, &tabs, &url_bar, &findbar);

        // ── Boutons ──────────────────────────────────────────────────────
        {
            let sb = sidebar.clone();
            sidebar_btn.connect_clicked(move |_| sb.toggle());
        }
        {
            let tb = tabs.clone();
            new_tab_btn.connect_clicked(move |_| { tb.open(HOME_PAGE); });
        }
        wire_star_button(&star_btn, &tabs.notebook, &vault, &sidebar);
        wire_shield_button(&shield_btn, &tabs.notebook, &blocker, &vault);
        wire_nav_button(&back_btn,    &tabs.notebook, |wv| wv.go_back());
        wire_nav_button(&forward_btn, &tabs.notebook, |wv| wv.go_forward());
        wire_nav_button(&reload_btn,  &tabs.notebook, |wv| wv.reload());
        wire_nav_button(&home_btn,    &tabs.notebook, |wv| wv.load_uri(HOME_PAGE));
        wire_nav_button(&reader_btn,  &tabs.notebook, reader::toggle);

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

        // ── Autocomplétion depuis bookmarks + history (Sprint 2.6) ───────
        wire_completion(&url_bar, vault.clone());

        // ── Sync URL bar + progress ↔ onglet actif ───────────────────────
        // Les signaux par-WebView sont câblés une seule fois, à la création
        // de la page (`page-added`) — jamais dans `switch-page`, sinon les
        // handlers s'accumulent à chaque changement d'onglet.
        {
            let ub = url_bar.clone();
            let prog = progress.clone();
            let v = vault.clone();
            let shield = shield_btn.clone();
            let bl = blocker.clone();
            tabs.notebook.connect_page_added(move |nb, child, _| {
                let Ok(wv) = child.clone().downcast::<WebView>() else { return };

                let nb2 = nb.clone();
                let ub2 = ub.clone();
                let shield2 = shield.clone();
                let bl2 = bl.clone();
                wv.connect_uri_notify(move |w| {
                    if is_current(&nb2, w) {
                        let uri = w.uri().unwrap_or_default();
                        ub2.set_text(&uri);
                        update_shield(&shield2, &bl2, &uri);
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

                // Historique automatique (Sprint 2.2)
                let v2 = v.clone();
                wv.connect_load_changed(move |w, event| {
                    if event != LoadEvent::Finished {
                        return;
                    }
                    let Some(uri) = w.uri() else { return };
                    if !uri.starts_with("http") {
                        return;
                    }
                    // Pas de doublon consécutif (reload, redirect déjà noté)
                    let last = v2.recent_history(1).ok().and_then(|mut h| h.pop());
                    if last.is_some_and(|l| l.url == uri) {
                        return;
                    }
                    let title = w.title().unwrap_or_default();
                    let _ = v2.push_history(&uri, &title);
                });

                // Détection de mot de passe (Sprint 2.8)
                webview::wire_password_capture(&wv, v.clone());
            });
        }
        {
            // `switch-page` fournit le widget de la page cible — ne pas
            // utiliser current_page() ici, il pointe encore l'ancien onglet.
            let ub = url_bar.clone();
            let prog = progress.clone();
            let shield = shield_btn.clone();
            let bl = blocker.clone();
            tabs.notebook.connect_switch_page(move |_, page, _| {
                if let Some(wv) = page.downcast_ref::<WebView>() {
                    let uri = wv.uri().unwrap_or_default();
                    ub.set_text(&uri);
                    update_shield(&shield, &bl, &uri);
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

        Self { window, tabs }
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

/// Bouton ★ : ajoute la page courante aux bookmarks (Sprint 2.4).
fn wire_star_button(btn: &Button, nb: &Notebook, vault: &Rc<Vault>, sidebar: &Sidebar) {
    let nb = nb.clone();
    let vault = vault.clone();
    let sidebar = sidebar.clone();
    btn.connect_clicked(move |_| {
        let Some(wv) = current_webview(&nb) else { return };
        let Some(uri) = wv.uri() else { return };

        // Pas de doublon : déjà bookmarké → rien à faire
        let already = vault
            .search_bookmarks(&uri)
            .unwrap_or_default()
            .iter()
            .any(|b| b.url == uri);
        if already {
            return;
        }

        let title = wv.title().map(String::from).unwrap_or_else(|| uri.to_string());
        if vault.add_bookmark(&Bookmark::new(uri.as_str(), title)).is_ok() {
            sidebar.refresh_bookmarks();
        }
    });
}

/// Bouton ⛨ : whitelist le site courant (Sprint 3.4), persiste dans le vault.
fn wire_shield_button(btn: &Button, nb: &Notebook, blocker: &Rc<AdBlocker>, vault: &Rc<Vault>) {
    let nb = nb.clone();
    let blocker = blocker.clone();
    let vault = vault.clone();
    btn.connect_clicked(move |btn| {
        let Some(wv) = current_webview(&nb) else { return };
        let Some(uri) = wv.uri() else { return };
        let (host, _) = crate::urls::host_and_path(&uri);
        if host.is_empty() {
            return;
        }

        blocker.set_whitelisted(host, !blocker.is_whitelisted(host));
        let json = serde_json::to_string(&blocker.whitelist_snapshot())
            .unwrap_or_else(|_| "[]".into());
        let _ = vault.set_setting("adblock_whitelist", &json);

        update_shield(btn, &blocker, &uri);
        wv.reload();
    });
}

/// Reflète l'état du bloqueur pour l'URL affichée (classe CSS + tooltip).
fn update_shield(btn: &Button, blocker: &Rc<AdBlocker>, url: &str) {
    let (host, _) = crate::urls::host_and_path(url);
    let off = !host.is_empty() && blocker.is_whitelisted(host);
    let ctx = btn.style_context();
    if off {
        ctx.add_class("nyx-shield-off");
        btn.set_tooltip_text(Some("Bloqueur coupé sur ce site — cliquer pour réactiver"));
    } else {
        ctx.remove_class("nyx-shield-off");
        btn.set_tooltip_text(Some("Bloqueur de pubs actif — cliquer pour whitelister ce site"));
    }
}

/// Autocomplétion substring sur les URLs des bookmarks + historique.
/// Le modèle est reconstruit quand l'URL bar prend le focus.
fn wire_completion(url_bar: &Entry, vault: Rc<Vault>) {
    let store = ListStore::new(&[glib::Type::STRING]);
    let completion = EntryCompletion::new();
    completion.set_model(Some(&store));
    completion.set_text_column(0);
    completion.set_minimum_key_length(2);
    completion.set_match_func(|completion, key, iter| {
        completion
            .model()
            .and_then(|m| m.value(iter, 0).get::<String>().ok())
            .is_some_and(|url| url.to_lowercase().contains(key))
    });
    url_bar.set_completion(Some(&completion));

    url_bar.connect_focus_in_event(move |_, _| {
        store.clear();
        let mut seen = std::collections::HashSet::new();
        let bookmarks = vault.list_bookmarks().unwrap_or_default();
        let history = vault.recent_history(300).unwrap_or_default();
        let urls = bookmarks
            .iter()
            .map(|b| b.url.as_str())
            .chain(history.iter().map(|h| h.url.as_str()));
        for url in urls {
            if seen.insert(url) {
                store.set(&store.append(), &[(0, &url)]);
            }
        }
        glib::Propagation::Proceed
    });
}

/// true si `wv` est la page actuellement affichée.
fn is_current(nb: &Notebook, wv: &WebView) -> bool {
    nb.current_page().is_some() && nb.current_page() == nb.page_num(wv)
}

fn wire_shortcuts(window: &ApplicationWindow, tabs: &TabBar, url_bar: &Entry, findbar: &FindBar) {
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

    // Ctrl+F → recherche dans la page (Sprint 4.2)
    let fb = findbar.clone();
    add_ctrl_accel(&accel, 'f', move || fb.open());

    // Ctrl + / − / 0 → zoom (Sprint 4.5) ; '=' = '+' sans Shift
    for key in ['+', '='] {
        let nb = tabs.notebook.clone();
        add_ctrl_accel(&accel, key, move || zoom_by(&nb, 0.1));
    }
    let nb = tabs.notebook.clone();
    add_ctrl_accel(&accel, '-', move || zoom_by(&nb, -0.1));
    let nb = tabs.notebook.clone();
    add_ctrl_accel(&accel, '0', move || {
        if let Some(wv) = current_webview(&nb) { wv.set_zoom_level(1.0); }
    });
}

fn zoom_by(nb: &Notebook, delta: f64) {
    if let Some(wv) = current_webview(nb) {
        wv.set_zoom_level((wv.zoom_level() + delta).clamp(0.3, 3.0));
    }
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
