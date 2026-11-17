use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib::WeakRef;
use gtk::pango::EllipsizeMode;
use gtk::prelude::*;
use gtk::{
    Box as GtkBox, Button, EventBox, Image, Label, Menu, MenuItem, Notebook,
    Orientation, SeparatorMenuItem,
};
use webkit2gtk::{WebView, WebViewExt};

use super::{favicon, TabBar};

/// Construit l'onglet : favicon + titre + ×, enveloppé dans un EventBox qui
/// porte les interactions souris :
///   • clic milieu → ferme l'onglet
///   • clic droit  → menu (épingler, dupliquer, recharger, fermer, …)
pub fn build(tabs: &TabBar, wv: &WebView, nb: &Notebook, initial: &str) -> EventBox {
    let icon = Image::new();
    icon.set_pixel_size(favicon::FAVICON_PX);
    icon.style_context().add_class("nyx-tab-favicon");
    favicon::set_fallback(&icon);
    favicon::bind(&icon, wv);

    let label = Label::new(Some(initial));
    label.set_max_width_chars(20);
    label.set_ellipsize(EllipsizeMode::End);

    let close = Button::with_label("×");
    close.style_context().add_class("nyx-tab-close");
    close.set_relief(gtk::ReliefStyle::None);

    let row = GtkBox::new(Orientation::Horizontal, 6);
    row.pack_start(&icon,  false, false, 0);
    row.pack_start(&label, true,  true,  0);
    row.pack_start(&close, false, false, 0);

    let host = EventBox::new();
    host.set_visible_window(false);
    host.add(&row);
    host.show_all();

    let pinned = Rc::new(Cell::new(false));

    // Titre dynamique
    let lbl = label.downgrade();
    wv.connect_title_notify(move |wv| {
        let Some(lbl) = lbl.upgrade() else { return };
        let t = wv.title().map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Nouvel onglet".into());
        lbl.set_text(&t);
        lbl.set_tooltip_text(Some(&t));
    });

    // Bouton ×
    {
        let (nb, wv) = (nb.downgrade(), wv.downgrade());
        close.connect_clicked(move |_| close_weak(&nb, &wv));
    }

    // Souris : clic milieu = fermer · clic droit = menu contextuel.
    {
        let (tabs, nb, wv) = (tabs.clone(), nb.downgrade(), wv.downgrade());
        let (label, close, pinned) = (label.clone(), close.clone(), pinned.clone());
        let menu_slot: Rc<RefCell<Option<(Menu, MenuItem)>>> = Rc::new(RefCell::new(None));
        host.connect_button_press_event(move |_, ev| {
            match ev.button() {
                2 => {
                    close_weak(&nb, &wv);
                    gtk::glib::Propagation::Stop
                }
                3 => {
                    let (Some(nb), Some(wv)) = (nb.upgrade(), wv.upgrade()) else {
                        return gtk::glib::Propagation::Stop;
                    };
                    // Un seul menu par onglet, construit à la demande puis réutilisé
                    // (aucune fuite ; libéré avec l'onglet).
                    let mut slot = menu_slot.borrow_mut();
                    let (menu, pin) = slot.get_or_insert_with(|| {
                        let (m, pin) = context_menu(&tabs, &nb, &wv, &label, &close, &pinned);
                        (m, pin)
                    });
                    pin.set_label(if pinned.get() { "Détacher" } else { "Épingler" });
                    menu.popup_at_pointer(Some(ev));
                    gtk::glib::Propagation::Stop
                }
                _ => gtk::glib::Propagation::Proceed, // gauche → bascule d'onglet
            }
        });
    }

    host
}

/// Menu contextuel d'onglet (reconstruit à chaque clic droit).
fn context_menu(
    tabs: &TabBar, nb: &Notebook, wv: &WebView,
    label: &Label, close: &Button, pinned: &Rc<Cell<bool>>,
) -> (Menu, MenuItem) {
    let menu = Menu::new();

    let pin = MenuItem::with_label(if pinned.get() { "Détacher" } else { "Épingler" });
    {
        let (nb, wv) = (nb.downgrade(), wv.downgrade());
        let (label, close, pinned) = (label.clone(), close.clone(), pinned.clone());
        pin.connect_activate(move |_| {
            let (Some(nb), Some(wv)) = (nb.upgrade(), wv.upgrade()) else { return };
            let now = !pinned.get();
            pinned.set(now);
            set_pinned(&nb, &wv, &label, &close, now);
        });
    }

    let dup = MenuItem::with_label("Dupliquer");
    {
        let (tabs, wv) = (tabs.clone(), wv.downgrade());
        dup.connect_activate(move |_| {
            let Some(wv) = wv.upgrade() else { return };
            if let Some(uri) = wv.uri() {
                if !uri.is_empty() { tabs.open_url(&uri); }
            }
        });
    }

    let reload = MenuItem::with_label("Recharger");
    {
        let wv = wv.downgrade();
        reload.connect_activate(move |_| { if let Some(wv) = wv.upgrade() { wv.reload(); } });
    }

    let close_it = MenuItem::with_label("Fermer");
    {
        let (nb, wv) = (nb.downgrade(), wv.downgrade());
        close_it.connect_activate(move |_| close_weak(&nb, &wv));
    }

    let close_others = MenuItem::with_label("Fermer les autres");
    {
        let (nb, wv) = (nb.downgrade(), wv.downgrade());
        close_others.connect_activate(move |_| {
            let (Some(nb), Some(wv)) = (nb.upgrade(), wv.upgrade()) else { return };
            for i in (0..nb.n_pages()).rev() {
                let Some(page) = nb.nth_page(Some(i)) else { continue };
                let Some(p) = page.downcast_ref::<WebView>() else { continue };
                if p != &wv && !is_pinned(&nb, p) { nb.remove_page(Some(i)); }
            }
        });
    }

    menu.append(&pin);
    menu.append(&dup);
    menu.append(&reload);
    menu.append(&SeparatorMenuItem::new());
    menu.append(&close_it);
    menu.append(&close_others);
    menu.show_all();
    (menu, pin)
}

/// Ferme l'onglet portant `wv` (no-op s'il n'est plus dans le notebook).
pub(super) fn close_tab(nb: &Notebook, wv: &WebView) {
    if let Some(i) = nb.page_num(wv) { nb.remove_page(Some(i)); }
}

fn close_weak(nb: &WeakRef<Notebook>, wv: &WeakRef<WebView>) {
    if let (Some(nb), Some(wv)) = (nb.upgrade(), wv.upgrade()) { close_tab(&nb, &wv); }
}

/// L'onglet de `wv` est-il épinglé ? (classe CSS posée par `set_pinned`)
pub(super) fn is_pinned(nb: &Notebook, wv: &WebView) -> bool {
    nb.tab_label(wv)
        .map(|t| t.style_context().has_class("nyx-tab-pinned"))
        .unwrap_or(false)
}

/// Épingle/détache : onglet compact (favicon seul) déplacé en tête, non
/// réordonnable tant qu'épinglé.
fn set_pinned(nb: &Notebook, wv: &WebView, label: &Label, close: &Button, pinned: bool) {
    label.set_visible(!pinned);
    close.set_visible(!pinned);
    if let Some(tab) = nb.tab_label(wv) {
        let ctx = tab.style_context();
        if pinned { ctx.add_class("nyx-tab-pinned"); } else { ctx.remove_class("nyx-tab-pinned"); }
    }
    if pinned {
        nb.reorder_child(wv, Some(0));
    }
    nb.set_tab_reorderable(wv, !pinned);
}
