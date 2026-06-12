use std::rc::Rc;

use gtk::prelude::*;
use gtk::{
    Box as GtkBox, Button, Label, ListBox, Orientation, Popover, PositionType,
    ScrolledWindow,
};
use vault::Vault;

/// Gestionnaire de mots de passe ancré sur le bouton ⚿ (Sprint 2.7) :
/// liste par domaine, copie dans le presse-papier, suppression.
/// Les secrets ne quittent jamais le vault sauf via le bouton copier.
pub fn build(anchor: &Button, vault: &Rc<Vault>) -> Popover {
    let pop = Popover::new(Some(anchor));
    pop.set_position(PositionType::Bottom);
    pop.style_context().add_class("nyx-bm-pop");

    let root = GtkBox::new(Orientation::Vertical, 8);
    root.set_size_request(320, -1);
    root.set_margin_top(10);
    root.set_margin_bottom(10);
    root.set_margin_start(10);
    root.set_margin_end(10);

    let title = Label::new(Some("Mots de passe"));
    title.set_xalign(0.0);
    title.style_context().add_class("nyx-bm-title");

    let list = ListBox::new();
    list.style_context().add_class("nyx-bm-list");
    let scroll = ScrolledWindow::builder().min_content_height(240).build();
    scroll.add(&list);

    root.pack_start(&title, false, false, 0);
    root.pack_start(&scroll, true, true, 0);
    pop.add(&root);
    root.show_all();

    // Reconstruit la liste à chaque ouverture.
    let (list, vault) = (list.clone(), vault.clone());
    pop.connect_show(move |_| repopulate(&list, &vault));

    pop
}

fn repopulate(list: &ListBox, vault: &Rc<Vault>) {
    for child in list.children() {
        list.remove(&child);
    }

    let domains = vault.list_password_domains().unwrap_or_default();
    if domains.is_empty() {
        let empty = Label::new(Some("Aucun mot de passe enregistré"));
        empty.style_context().add_class("nyx-bm-empty");
        empty.set_margin_top(18);
        empty.set_margin_bottom(18);
        list.add(&empty);
        list.show_all();
        return;
    }

    for domain in domains {
        for p in vault.passwords_for(&domain).unwrap_or_default() {
            let row = GtkBox::new(Orientation::Horizontal, 6);

            let who = if p.username.is_empty() { "(sans identifiant)" } else { &p.username };
            let text = GtkBox::new(Orientation::Vertical, 2);
            let user_lbl = Label::new(Some(who));
            user_lbl.set_xalign(0.0);
            user_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
            let dom_lbl = Label::new(Some(&domain));
            dom_lbl.set_xalign(0.0);
            dom_lbl.style_context().add_class("nyx-bm-empty");
            text.pack_start(&user_lbl, false, false, 0);
            text.pack_start(&dom_lbl, false, false, 0);

            let copy = flat_button("⧉", "Copier le mot de passe");
            let del  = flat_button("×", "Supprimer");

            row.pack_start(&text, true, true, 0);
            row.pack_end(&del, false, false, 0);
            row.pack_end(&copy, false, false, 0);
            list.add(&row);

            {
                let secret = p.password.clone();
                copy.connect_clicked(move |_| {
                    gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD).set_text(&secret);
                });
            }
            {
                let (list, vault) = (list.clone(), vault.clone());
                let id = p.id.unwrap_or_default();
                del.connect_clicked(move |_| {
                    if vault.delete_password(id).is_ok() {
                        repopulate(&list, &vault);
                    }
                });
            }
        }
    }
    list.show_all();
}

fn flat_button(label: &str, tooltip: &str) -> Button {
    let btn = Button::with_label(label);
    btn.set_relief(gtk::ReliefStyle::None);
    btn.set_tooltip_text(Some(tooltip));
    btn.style_context().add_class("nyx-nav-btn");
    btn
}
