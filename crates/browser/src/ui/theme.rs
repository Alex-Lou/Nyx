use gtk::prelude::*;
use gtk::{CssProvider, StyleContext};

const THEME_CSS: &str = include_str!("../../../../assets/theme.css");

pub fn load() {
    let provider = CssProvider::new();
    provider
        .load_from_data(THEME_CSS.as_bytes())
        .expect("Failed to load Nyx theme CSS");

    StyleContext::add_provider_for_screen(
        &gtk::gdk::Screen::default().expect("no screen"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
