use std::cell::Cell;
use std::rc::Rc;

use gtk::pango::EllipsizeMode;
use gtk::prelude::*;
use gtk::{
    Box as GtkBox, Button, DrawingArea, EventBox, Image, Label, Menu, MenuItem,
    Notebook, Orientation, SeparatorMenuItem,
};
use webkit2gtk::{WebView, WebViewExt};

use super::{favicon, TabBar};
use crate::ui::icons::{self, Icon};

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

    let close = Button::new();
    close.style_context().add_class("nyx-tab-close");
    close.set_relief(gtk::ReliefStyle::None);
    close.set_tooltip_text(Some("Fermer"));
    set_icon(&close, Icon::CloseTab, 14);

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
    let lbl = label.clone();
    wv.connect_title_notify(move |wv| {
        let t = wv.title().map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Nouvel onglet".into());
        lbl.set_text(&t);
        lbl.set_tooltip_text(Some(&t));
    });

    // Bouton ×
    {
        let (nb, wv) = (nb.clone(), wv.clone());
        close.connect_clicked(move |_| {
            if let Some(i) = nb.page_num(&wv) { nb.remove_page(Some(i)); }
        });
    }

    // Souris : clic milieu = fermer · clic droit = menu contextuel.
    {
        let (tabs, nb, wv) = (tabs.clone(), nb.clone(), wv.clone());
        let (label, close, pinned) = (label.clone(), close.clone(), pinned.clone());
        host.connect_button_press_event(move |_, ev| {
            match ev.button() {
                2 => {
                    if let Some(i) = nb.page_num(&wv) { nb.remove_page(Some(i)); }
                    gtk::glib::Propagation::Stop
                }
                3 => {
                    context_menu(&tabs, &nb, &wv, &label, &close, &pinned)
                        .popup_at_pointer(Some(ev));
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
    tabs: &TabBar,
    nb: &Notebook,
    wv: &WebView,
    label: &Label,
    close: &Button,
    pinned: &Rc<Cell<bool>>,
) -> Menu {
    let menu = Menu::new();

    let pin = MenuItem::with_label(if pinned.get() { "Détacher" } else { "Épingler" });
    {
        let (nb, wv, label, close, pinned) =
            (nb.clone(), wv.clone(), label.clone(), close.clone(), pinned.clone());
        pin.connect_activate(move |_| {
            let now = !pinned.get();
            pinned.set(now);
            set_pinned(&nb, &wv, &label, &close, now);
        });
    }

    let dup = MenuItem::with_label("Dupliquer");
    {
        let (tabs, wv) = (tabs.clone(), wv.clone());
        dup.connect_activate(move |_| {
            if let Some(uri) = wv.uri() {
                if !uri.is_empty() { tabs.open_url(&uri); }
            }
        });
    }

    let reload = MenuItem::with_label("Recharger");
    {
        let wv = wv.clone();
        reload.connect_activate(move |_| wv.reload());
    }

    let close_it = MenuItem::with_label("Fermer");
    {
        let (nb, wv) = (nb.clone(), wv.clone());
        close_it.connect_activate(move |_| {
            if let Some(i) = nb.page_num(&wv) { nb.remove_page(Some(i)); }
        });
    }

    let close_others = MenuItem::with_label("Fermer les autres");
    {
        let (nb, wv) = (nb.clone(), wv.clone());
        close_others.connect_activate(move |_| {
            // Retrait à rebours : les indices restants ne bougent pas.
            for i in (0..nb.n_pages()).rev() {
                if let Some(page) = nb.nth_page(Some(i)) {
                    let keep = page.downcast_ref::<WebView>().map(|p| p == &wv).unwrap_or(false);
                    if !keep { nb.remove_page(Some(i)); }
                }
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
    menu
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

/// Pose une icône vectorielle (recolorée selon l'état) comme image d'un bouton.
fn set_icon(btn: &Button, icon: Icon, px: i32) {
    let area = DrawingArea::new();
    area.set_size_request(px, px);
    let b = btn.clone();
    area.connect_draw(move |a, cr| {
        let (w, h) = (a.allocated_width() as f64, a.allocated_height() as f64);
        let c = b.style_context().color(b.state_flags());
        icons::draw(cr, icon, w, h, (c.red(), c.green(), c.blue(), c.alpha()));
        gtk::glib::Propagation::Proceed
    });
    let a = area.clone();
    btn.connect_state_flags_changed(move |_, _| a.queue_draw());
    btn.set_image(Some(&area));
    btn.set_always_show_image(true);
}
