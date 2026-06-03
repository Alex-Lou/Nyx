//! Menu « ⋮ » du header — `Popover` au lieu de `Menu` pour le flip auto.
//!
//! Le `Menu` GTK3 s'ouvrait fixement à droite du bouton et débordait
//! hors écran quand la navbar collait au bord droit. Le `Popover`
//! re-positionne automatiquement (flip horizontal + vertical) selon
//! l'espace disponible — le menu glisse vers la gauche tout seul.
//!
//! Contenu :
//!   - bouton item « Choisir le dossier de destination… » → FileChooser.
//!   - CheckButton « Re-vérifier avant ouverture » → toggle settings.
//!
//! Sortir d'un GtkMenu impose aussi de réimplémenter le look ; le CSS
//! `nyx-dl-menu-pop` / `nyx-dl-menu-item` / `nyx-dl-menu-toggle` s'en
//! charge dans `assets/theme.css`.

use gtk::prelude::*;
use gtk::{
    Box as GtkBox, Button, CheckButton, FileChooserAction, FileChooserNative,
    IconSize, Image, Label, MenuButton, Orientation, Popover, PositionType,
    ResponseType, Window,
};

use crate::state::settings::Settings;

const POP_WIDTH: i32 = 260;

/// Construit le `MenuButton` ⋮ avec un Popover ancré + auto-flip.
pub fn build(parent: Option<Window>, settings: Settings) -> MenuButton {
    let btn = MenuButton::new();
    btn.set_image(Some(&Image::from_icon_name(
        Some("view-more-symbolic"), IconSize::Button,
    )));
    btn.set_relief(gtk::ReliefStyle::None);
    btn.set_tooltip_text(Some("Plus"));
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

    add_choose_dir(&content, &pop, parent, settings.clone());
    add_recheck_toggle(&content, settings);

    pop.add(&content);
    content.show_all();
    btn.set_popover(Some(&pop));
    btn
}

fn add_choose_dir(
    content: &GtkBox, pop: &Popover, parent: Option<Window>, settings: Settings,
) {
    let item = item_button("Choisir le dossier de destination…");
    content.pack_start(&item, false, false, 0);

    let p = pop.clone();
    item.connect_clicked(move |_| {
        p.popdown(); // ferme avant ouverture du chooser pour éviter overlap
        let chooser = FileChooserNative::new(
            Some("Dossier de téléchargement"),
            parent.as_ref(),
            FileChooserAction::SelectFolder,
            Some("Sélectionner"),
            Some("Annuler"),
        );
        if let Some(current) = current_dir(&settings) {
            let _ = chooser.set_current_folder(current);
        }
        if chooser.run() == ResponseType::Accept {
            if let Some(path) = chooser.filename() {
                let s = path.to_string_lossy().into_owned();
                settings.borrow_mut().downloads_dir = Some(s);
            }
        }
    });
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

fn item_button(text: &str) -> Button {
    let btn = Button::with_label(text);
    btn.set_relief(gtk::ReliefStyle::None);
    btn.style_context().add_class("nyx-dl-menu-item");
    if let Some(lbl) = btn.child().and_then(|c| c.downcast::<Label>().ok()) {
        lbl.set_xalign(0.0);
    }
    btn
}

fn current_dir(settings: &Settings) -> Option<std::path::PathBuf> {
    let user = settings.borrow().downloads_dir.clone();
    user.filter(|s| !s.is_empty()).map(std::path::PathBuf::from)
        .or_else(crate::platform::default_downloads_dir)
}
