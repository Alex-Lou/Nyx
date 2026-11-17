use gtk::prelude::*;
use gtk::{CssProvider, StyleContext};

const THEME_CSS: &str = include_str!("../../../../assets/theme.css");

pub fn load() {
    let provider = CssProvider::new();
    if let Err(e) = provider.load_from_data(THEME_CSS.as_bytes()) {
        eprintln!("nyx: échec du chargement du thème CSS : {e}");
        return;
    }
    let Some(screen) = gtk::gdk::Screen::default() else {
        eprintln!("nyx: aucun écran GDK, thème non appliqué");
        return;
    };

    StyleContext::add_provider_for_screen(
        &screen,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
