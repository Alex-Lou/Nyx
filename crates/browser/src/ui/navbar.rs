use gtk::prelude::*;
use gtk::{Box as GtkBox, Button, Entry, EntryIconPosition, Orientation};
use webkit2gtk::{WebView, WebViewExt};

use nyx_core::site_data_policy::{self, Scope};

use crate::pages::{self, newtab};
use crate::state::bookmarks::{self, Bookmarks};
use crate::state::downloads::DownloadsHandle;
use crate::state::settings::Settings;
use crate::ui::tabs::TabBar;
use crate::ui::{bookmarks_popover, downloads as downloads_ui};
use crate::web::{self, site_data};

/// Construit la barre de navigation, câble ses boutons + la barre d'adresse,
/// et renvoie le widget prêt à packer dans la fenêtre.
pub fn build(
    url_bar: &Entry, tabs: &TabBar, settings: &Settings,
    bm: &Bookmarks, downloads: &DownloadsHandle,
) -> GtkBox {
    let back     = nav_button("◀", "Précédent");
    let forward  = nav_button("▶", "Suivant");
    let reload   = nav_button("↺", "Recharger (Ctrl+R)");
    let home     = nav_button("⌂", "Accueil");
    let star     = nav_button("☆", "Favoris");
    let forget   = nav_button("🛇", "Oublier ce site");
    let new_tab  = nav_button("+", "Nouvel onglet (Ctrl+T)");
    let settings_b = nav_button("⚙", "Paramètres (Ctrl+,)");
    let dl_btn   = downloads_ui::install(downloads, settings);

    // ⭐ dans la barre d'adresse → ajout direct du favori courant.
    url_bar.set_icon_from_icon_name(EntryIconPosition::Secondary, Some("starred-symbolic"));
    url_bar.set_icon_tooltip_text(EntryIconPosition::Secondary, Some("Ajouter aux favoris"));
    {
        let (t, b) = (tabs.clone(), bm.clone());
        url_bar.connect_icon_press(move |entry, pos, _| {
            if pos == EntryIconPosition::Secondary {
                bookmark_current(&t, &b, entry);
            }
        });
    }

    let bar = GtkBox::new(Orientation::Horizontal, 4);
    bar.style_context().add_class("nyx-navbar");
    bar.pack_start(&back,    false, false, 0);
    bar.pack_start(&forward, false, false, 0);
    bar.pack_start(&reload,  false, false, 0);
    bar.pack_start(&home,    false, false, 4);
    bar.pack_start(url_bar,  true,  true,  0);
    bar.pack_end(&settings_b, false, false, 0);
    bar.pack_end(&star,       false, false, 0);
    bar.pack_end(&dl_btn,     false, false, 0);
    bar.pack_end(&forget,     false, false, 0);
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

    // ☆ → petit gestionnaire de favoris (popover).
    {
        let pop = bookmarks_popover::build(&star, tabs, bm);
        star.connect_clicked(move |_| pop.popup());
    }

    // 🛇 → "Oublier ce site" : popover avec 2 niveaux (origine / domaine).
    {
        let pop = build_forget_popover(&forget, tabs, url_bar);
        forget.connect_clicked(move |_| pop.popup());
    }

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

/// Ajoute la page courante aux favoris + court toast dans la barre d'adresse.
/// Partagé entre la ⭐ de la barre et le raccourci Ctrl+D.
pub fn bookmark_current(tabs: &TabBar, bm: &Bookmarks, url_bar: &Entry) {
    let Some(wv) = tabs.current_webview() else { return };
    let url = wv.uri().map(|s| s.to_string()).unwrap_or_default();
    if url.is_empty() {
        return;
    }
    let title = wv.title().map(|s| s.to_string()).unwrap_or_else(|| url.clone());
    let msg = if bookmarks::add(bm, url.clone(), title) {
        "  ★  Favori ajouté"
    } else {
        "  ★  Déjà en favori"
    };

    let ub = url_bar.clone();
    url_bar.set_text(msg);
    gtk::glib::timeout_add_local_once(
        std::time::Duration::from_millis(1400),
        move || ub.set_text(&url),
    );
}

/// Popover "Oublier ce site" — 2 niveaux : origine exacte ou domaine entier.
/// L'exécution passe par `web::site_data::execute` ; la politique est dans
/// `nyx-core::site_data_policy`.
fn build_forget_popover(anchor: &Button, tabs: &TabBar, url_bar: &Entry) -> gtk::Popover {
    use gtk::{Label, Orientation, Popover, PositionType};
    let pop = Popover::new(Some(anchor));
    pop.set_position(PositionType::Bottom);
    pop.style_context().add_class("nyx-bm-pop");

    let root = GtkBox::new(Orientation::Vertical, 8);
    root.set_margin_top(10); root.set_margin_bottom(10);
    root.set_margin_start(12); root.set_margin_end(12);

    let title = Label::new(Some("Oublier ce site"));
    title.set_xalign(0.0);
    title.style_context().add_class("nyx-bm-title");

    let origin_btn = Button::with_label("Cette origine seulement");
    let domain_btn = Button::with_label("Tout le domaine");
    origin_btn.style_context().add_class("nyx-nav-btn");
    domain_btn.style_context().add_class("nyx-nav-btn");
    origin_btn.set_relief(gtk::ReliefStyle::None);
    domain_btn.set_relief(gtk::ReliefStyle::None);

    root.pack_start(&title, false, false, 0);
    root.pack_start(&origin_btn, false, false, 0);
    root.pack_start(&domain_btn, false, false, 0);
    pop.add(&root);
    root.show_all();

    let trigger = |scope: Scope, tabs: TabBar, url_bar: Entry, pop: Popover| -> Box<dyn Fn(&Button)> {
        Box::new(move |_| {
            let Some(wv) = tabs.current_webview() else { return };
            let uri = wv.uri().map(|u| u.to_string()).unwrap_or_default();
            let Some(plan) = site_data_policy::plan_for(&uri, scope) else {
                flash(&url_bar, "  Aucun site web à oublier");
                return;
            };
            if let Some(ctx) = wv.context() {
                site_data::execute(&ctx, &plan);
            }
            flash(&url_bar, &format!("  🛇  Oublié : {}", plan.target));
            wv.reload();
            pop.popdown();
        })
    };

    origin_btn.connect_clicked(trigger(Scope::Origin, tabs.clone(), url_bar.clone(), pop.clone()));
    domain_btn.connect_clicked(trigger(Scope::Domain, tabs.clone(), url_bar.clone(), pop.clone()));
    pop
}

/// Toast court dans la barre d'adresse (la valeur est restaurée après 1.4s).
fn flash(url_bar: &Entry, msg: &str) {
    let prev = url_bar.text().to_string();
    url_bar.set_text(msg);
    let ub = url_bar.clone();
    gtk::glib::timeout_add_local_once(
        std::time::Duration::from_millis(1400),
        move || ub.set_text(&prev),
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
