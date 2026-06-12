//! Dialog modal thématisé Nyx — partagé entre les surfaces qui demandent
//! confirmation utilisateur.
//!
//! Remplace les `MessageDialog` GTK qui sortaient en thème système blanc/gris.
//! Ici on a un layout clair (icône large + titre + corps + boutons), centré
//! sur la fenêtre parent, classes CSS `.nyx-confirm*` pour le style.
//!
//! Deux niveaux :
//!   - `Normal`   : icône question lune, bouton accept aurora.
//!   - `Danger`   : icône warning ambre, bouton accept destructive (rose).

use std::cell::RefCell;

use gtk::prelude::*;
use gtk::{
    Box as GtkBox, Dialog, DialogFlags, IconSize, Image, Label, Orientation,
    ResponseType, Window, WindowPosition,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmLevel {
    Normal,
    Danger,
}

pub struct ConfirmParams<'a> {
    pub parent:  Option<&'a Window>,
    pub level:   ConfirmLevel,
    pub title:   String,
    pub body:    String,
    pub accept:  String,
    pub cancel:  String,
}

/// Affiche un dialog modal. `callback(true)` si l'utilisateur valide,
/// `false` sinon. Non-bloquant — le callback est invoqué via `connect_response`.
pub fn show<F: FnOnce(bool) + 'static>(params: ConfirmParams, callback: F) {
    let dlg = Dialog::with_buttons(
        Some(&params.title),
        params.parent,
        DialogFlags::MODAL | DialogFlags::DESTROY_WITH_PARENT,
        &[],
    );
    dlg.set_position(WindowPosition::CenterOnParent);
    dlg.set_default_size(460, -1);
    dlg.set_resizable(false);
    // Pas de bordure native système pour respecter le thème Nyx.
    dlg.set_decorated(true);
    dlg.style_context().add_class("nyx-confirm");
    dlg.style_context().add_class(match params.level {
        ConfirmLevel::Normal => "nyx-confirm-normal",
        ConfirmLevel::Danger => "nyx-confirm-danger",
    });

    let content = dlg.content_area();
    content.set_orientation(Orientation::Vertical);
    content.set_spacing(12);
    content.set_margin_top(24);
    content.set_margin_bottom(8);
    content.set_margin_start(32);
    content.set_margin_end(32);

    // Icône large centrée.
    let icon_name = match params.level {
        ConfirmLevel::Normal => "dialog-question-symbolic",
        ConfirmLevel::Danger => "dialog-warning-symbolic",
    };
    let icon_box = GtkBox::new(Orientation::Horizontal, 0);
    icon_box.set_halign(gtk::Align::Center);
    let icon = Image::from_icon_name(Some(icon_name), IconSize::Dialog);
    icon.set_pixel_size(48);
    icon.style_context().add_class("nyx-confirm-icon");
    icon_box.pack_start(&icon, false, false, 0);
    content.pack_start(&icon_box, false, false, 0);

    // Titre.
    let title = Label::new(Some(&params.title));
    title.style_context().add_class("nyx-confirm-title");
    title.set_halign(gtk::Align::Center);
    title.set_justify(gtk::Justification::Center);
    title.set_line_wrap(true);
    title.set_max_width_chars(42);
    content.pack_start(&title, false, false, 0);

    // Corps.
    if !params.body.is_empty() {
        let body = Label::new(Some(&params.body));
        body.style_context().add_class("nyx-confirm-body");
        body.set_halign(gtk::Align::Center);
        body.set_justify(gtk::Justification::Center);
        body.set_line_wrap(true);
        body.set_max_width_chars(50);
        content.pack_start(&body, false, false, 0);
    }

    // Boutons. ResponseType::Cancel ferme par défaut sur Esc.
    let cancel = dlg.add_button(&params.cancel, ResponseType::Cancel);
    cancel.style_context().add_class("nyx-confirm-cancel");
    let accept = dlg.add_button(&params.accept, ResponseType::Accept);
    accept.style_context().add_class("nyx-confirm-accept");
    if matches!(params.level, ConfirmLevel::Danger) {
        accept.style_context().add_class("destructive-action");
    }
    accept.grab_default();

    // L'action_area de Dialog est dépréciée en gtk-rs 0.18 (pas d'accesseur
    // sûr). On la stylise via les sélecteurs CSS GTK natifs `.dialog
    // buttonbox` / `.dialog-action-box` dans `.nyx-confirm` (voir theme.css).

    let slot: RefCell<Option<F>> = RefCell::new(Some(callback));
    dlg.connect_response(move |d, resp| {
        if let Some(cb) = slot.borrow_mut().take() {
            cb(resp == ResponseType::Accept);
        }
        d.close();
    });
    dlg.show_all();
}
