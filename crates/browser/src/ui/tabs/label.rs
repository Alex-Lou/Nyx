use gtk::pango::EllipsizeMode;
use gtk::prelude::*;
use gtk::{Box as GtkBox, Button, Image, Label, Notebook, Orientation};
use webkit2gtk::{WebView, WebViewExt};

use super::favicon;

/// Construit le contenu d'un onglet : favicon + titre (dynamique) + bouton ×.
pub fn build(wv: &WebView, nb: &Notebook, initial: &str) -> GtkBox {
    let icon = Image::new();
    icon.set_pixel_size(favicon::FAVICON_PX);
    icon.style_context().add_class("nyx-tab-favicon");
    favicon::set_fallback(&icon);
    favicon::bind(&icon, wv);

    let label = Label::new(Some(initial));
    label.set_max_width_chars(20);
    label.set_ellipsize(EllipsizeMode::End);

    let close = Button::with_label("×");
    close.style_context().add_class("nyx-tab-close");
    close.set_relief(gtk::ReliefStyle::None);

    let row = GtkBox::new(Orientation::Horizontal, 6);
    row.pack_start(&icon,  false, false, 0);
    row.pack_start(&label, true,  true,  0);
    row.pack_start(&close, false, false, 0);
    row.show_all();

    // Titre dynamique
    let lbl = label.clone();
    wv.connect_title_notify(move |wv| {
        let t = wv.title().map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Nouvel onglet".into());
        lbl.set_text(&t);
        lbl.set_tooltip_text(Some(&t));
    });

    // Fermeture
    let nb = nb.clone();
    let wv = wv.clone();
    close.connect_clicked(move |_| {
        if let Some(i) = nb.page_num(&wv) { nb.remove_page(Some(i)); }
    });

    row
}
