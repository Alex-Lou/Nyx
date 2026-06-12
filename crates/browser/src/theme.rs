use gtk::prelude::*;
use gtk::{CssProvider, StyleContext};

const THEME_CSS: &str = include_str!("../../../assets/theme.css");

/// Charge le thème Nyx. Un thème invalide ne doit pas empêcher
/// le navigateur de démarrer : on log et on continue en thème GTK natif.
pub fn load() {
    // Nyx est un thème nuit : les widgets natifs (menus, dialogs WebKit)
    // doivent suivre la variante sombre (Sprint 4.1).
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(true);
    }

    let provider = CssProvider::new();
    if let Err(e) = provider.load_from_data(THEME_CSS.as_bytes()) {
        eprintln!("nyx: thème CSS invalide, fallback GTK natif : {e}");
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
