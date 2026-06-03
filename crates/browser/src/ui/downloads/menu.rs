//! Menu « ⋮ » du header — options globales de la shelf.
//!
//! Contenu actuel :
//!   - « Choisir le dossier de destination… » → FileChooser SelectFolder
//!     écrit `settings.downloads_dir`.
//!   - « Re-vérifier avant ouverture » → toggle de
//!     `settings.recheck_on_run` (case cochée par défaut).
//!
//! Étendable : ajouter une entrée = nouvelle ligne. Pas d'effet de bord
//! ailleurs (Single Responsibility).

use gtk::prelude::*;
use gtk::{
    CheckMenuItem, FileChooserAction, FileChooserNative, Menu, MenuButton, MenuItem,
    ResponseType, Window,
};

use crate::state::settings::Settings;

/// Construit le `MenuButton` ⋮ prêt à packer.
pub fn build(parent: Option<Window>, settings: Settings) -> MenuButton {
    let btn = MenuButton::new();
    btn.set_relief(gtk::ReliefStyle::None);
    btn.set_tooltip_text(Some("Plus d'options"));
    btn.style_context().add_class("nyx-nav-btn");
    btn.set_label("⋮");

    let menu = Menu::new();
    btn.set_popup(Some(&menu));

    add_choose_dir(&menu, parent, settings.clone());
    add_recheck_toggle(&menu, settings);

    menu.show_all();
    btn
}

fn add_choose_dir(menu: &Menu, parent: Option<Window>, settings: Settings) {
    let item = MenuItem::with_label("Choisir le dossier de destination…");
    menu.append(&item);
    item.connect_activate(move |_| {
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

fn add_recheck_toggle(menu: &Menu, settings: Settings) {
    let initial = settings.borrow().recheck_on_run;
    let item = CheckMenuItem::with_label("Re-vérifier avant ouverture");
    item.set_active(initial);
    item.set_tooltip_text(Some(
        "Demande confirmation si le contenu détecté est exécutable",
    ));
    menu.append(&item);

    let s = settings.clone();
    item.connect_toggled(move |it| {
        s.borrow_mut().recheck_on_run = it.is_active();
    });
}

fn current_dir(settings: &Settings) -> Option<std::path::PathBuf> {
    let user = settings.borrow().downloads_dir.clone();
    user.filter(|s| !s.is_empty()).map(std::path::PathBuf::from)
        .or_else(crate::platform::default_downloads_dir)
}
