use gtk::prelude::*;
use gtk::{HeaderBar, Window};

/// Pose une titlebar Nyx (CSD) thémée sur n'importe quelle fenêtre.
///
/// Toutes les fenêtres Nyx — principale, modal Paramètres, et tout futur
/// modal — passent par ici : un seul endroit, un seul look. Remplace la
/// barre système « froide » par une barre cohérente avec le thème.
pub fn apply_titlebar(window: &impl IsA<Window>, title: &str) {
    let header = HeaderBar::new();
    header.set_show_close_button(true);
    header.set_title(Some(title));
    header.style_context().add_class("nyx-headerbar");
    window.set_titlebar(Some(&header));
}
