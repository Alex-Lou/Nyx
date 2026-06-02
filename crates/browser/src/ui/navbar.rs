use gtk::prelude::*;
use gtk::{Box as GtkBox, Button, Entry, Orientation};
use webkit2gtk::{WebView, WebViewExt};

use crate::pages::{self, newtab};
use crate::state::bookmarks::{Bookmark, Bookmarks};
use crate::state::settings::Settings;
use crate::ui::tabs::TabBar;
use crate::web;

/// Construit la barre de navigation, câble ses boutons + la barre d'adresse,
/// et renvoie le widget prêt à packer dans la fenêtre.
pub fn build(url_bar: &Entry, tabs: &TabBar, settings: &Settings, bm: &Bookmarks) -> GtkBox {
    let back     = nav_button("◀", "Précédent");
    let forward  = nav_button("▶", "Suivant");
    let reload   = nav_button("↺", "Recharger (Ctrl+R)");
    let home     = nav_button("⌂", "Accueil");
    let star     = nav_button("☆", "Favori (Ctrl+D)");
    let new_tab  = nav_button("+", "Nouvel onglet (Ctrl+T)");
    let settings_b = nav_button("⚙", "Paramètres (Ctrl+,)");

    let bar = GtkBox::new(Orientation::Horizontal, 4);
    bar.style_context().add_class("nyx-navbar");
    bar.pack_start(&back,    false, false, 0);
    bar.pack_start(&forward, false, false, 0);
    bar.pack_start(&reload,  false, false, 0);
    bar.pack_start(&home,    false, false, 4);
    bar.pack_start(url_bar,  true,  true,  0);
    bar.pack_end(&settings_b, false, false, 0);
    bar.pack_end(&star,       false, false, 0);
    bar.pack_end(&new_tab,    false, false, 4);

    let t = tabs.clone();
    back.connect_clicked(move |_| t.with_current(|wv| wv.go_back()));
    let t = tabs.clone();
    forward.connect_clicked(move |_| t.with_current(|wv| wv.go_forward()));
    let t = tabs.clone();
    reload.connect_clicked(move |_| t.with_current(|wv| wv.reload()));
    let t = tabs.clone();
    new_tab.connect_clicked(move |_| { t.open_new_tab(); });
    let t = tabs.clone();
    settings_b.connect_clicked(move |_| { t.open_settings(); });

    // Home → URL configurée dans les paramètres.
    let t = tabs.clone();
    let s = settings.clone();
    home.connect_clicked(move |_| {
        let url = s.borrow().home_url.clone();
        match t.current_webview() {
            Some(wv) => load(&wv, &url),
            None     => { t.open_new_tab(); }
        }
    });

    // Favori courant.
    let t = tabs.clone();
    let b = bm.clone();
    let ub = url_bar.clone();
    star.connect_clicked(move |_| bookmark_current(&t, &b, &ub));

    // Barre d'adresse → charger via le moteur configuré.
    let t = tabs.clone();
    let s = settings.clone();
    url_bar.connect_activate(move |entry| {
        let url = web::resolve_input(&entry.text(), &s.borrow());
        if url.is_empty() { return; }
        entry.set_text(&url);
        t.with_current(|wv| load(wv, &url));
    });

    bar
}

/// Ajoute la page courante aux favoris + toast 1,5 s dans la barre d'adresse.
/// Partagé entre le bouton ☆ et le raccourci Ctrl+D.
pub fn bookmark_current(tabs: &TabBar, bm: &Bookmarks, url_bar: &Entry) {
    let Some(wv) = tabs.current_webview() else { return };
    let url = wv.uri().map(|s| s.to_string()).unwrap_or_default();
    if url.is_empty() { return; }
    let title = wv.title().map(|s| s.to_string()).unwrap_or_else(|| url.clone());
    bm.borrow_mut().push(Bookmark { url: url.clone(), title });

    let ub = url_bar.clone();
    url_bar.set_text("  ★  Favori ajouté");
    gtk::glib::timeout_add_local_once(
        std::time::Duration::from_millis(1500),
        move || ub.set_text(&url),
    );
}

fn nav_button(label: &str, tooltip: &str) -> Button {
    let btn = Button::with_label(label);
    btn.style_context().add_class("nyx-nav-btn");
    btn.set_relief(gtk::ReliefStyle::None);
    btn.set_tooltip_text(Some(tooltip));
    btn
}

/// Charge une URL ; `nyx://newtab` est rendu directement, le reste passe par
/// le policy filter (qui route les autres `nyx://`).
fn load(wv: &WebView, url: &str) {
    if url == "nyx://newtab" {
        wv.load_html(&newtab::html(), Some(&pages::assets_base_uri()));
    } else {
        wv.load_uri(url);
    }
}
