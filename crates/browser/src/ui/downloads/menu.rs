//! Menu « ⋮ » du header — `Popover` au lieu de `Menu` pour le flip auto.
//!
//! Contenu réduit aux **toggles** : le choisir-dossier vit maintenant
//! comme bouton direct dans le header (le nested-popover cassait son
//! FileChooser sur certains compositeurs).
//!
//! Item actuel :
//!   - CheckButton « Re-vérifier avant ouverture » → toggle
//!     `settings.recheck_on_run`.

use std::rc::Rc;

use gtk::prelude::*;
use gtk::{
    Box as GtkBox, CheckButton, IconSize, Image, MenuButton, Orientation,
    Popover, PositionType, Window,
};

use crate::state::settings::Settings;
use crate::ui::toast::ToastHandle;

const POP_WIDTH: i32 = 240;

/// Construit le `MenuButton` ⋮ avec un Popover ancré + auto-flip.
/// `close_shelf` et `toaster` sont réservés pour de futures actions ;
/// signature stable pour ne pas casser l'appelant si on ajoute des items.
pub fn build(
    parent: Option<Window>,
    settings: Settings,
    close_shelf: Rc<dyn Fn()>,
    toaster: ToastHandle,
) -> MenuButton {
    let _ = (parent, close_shelf, toaster); // réservés pour usage futur

    let btn = MenuButton::new();
    btn.set_image(Some(&Image::from_icon_name(
        Some("view-more-symbolic"), IconSize::Button,
    )));
    btn.set_relief(gtk::ReliefStyle::None);
    btn.style_context().add_class("nyx-nav-btn");
    btn.style_context().add_class("nyx-dl-action");

    let pop = Popover::new(Some(&btn));
    pop.set_position(PositionType::Bottom);
    pop.style_context().add_class("nyx-dl-menu-pop");

    let content = GtkBox::new(Orientation::Vertical, 2);
    content.set_size_request(POP_WIDTH, -1);
    content.set_margin_top(6);
    content.set_margin_bottom(6);
    content.set_margin_start(6);
    content.set_margin_end(6);

    add_recheck_toggle(&content, settings);

    pop.add(&content);
    content.show_all();
    btn.set_popover(Some(&pop));
    btn
}

fn add_recheck_toggle(content: &GtkBox, settings: Settings) {
    let initial = settings.borrow().recheck_on_run;
    let check = CheckButton::with_label("Re-vérifier avant ouverture");
    check.set_active(initial);
    check.style_context().add_class("nyx-dl-menu-toggle");
    check.set_tooltip_text(Some(
        "Demande confirmation si le contenu détecté est exécutable",
    ));
    content.pack_start(&check, false, false, 0);

    check.connect_toggled(move |it| {
        settings.borrow_mut().recheck_on_run = it.is_active();
    });
}
