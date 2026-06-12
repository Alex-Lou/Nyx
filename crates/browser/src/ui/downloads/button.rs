//! Bouton de la shelf de téléchargements — flèche soft style Nyx.
//!
//! Symbole `⤓` (downwards arrow to bar, U+2913) : sobre, lisible à
//! 12 px comme à 24 px, cohérent avec les autres glyphes Unicode de la
//! navbar (`◀ ▶ ↺ ⌂ ☆ 🛇 ⚙`).

use gtk::prelude::*;
use gtk::Button;

/// Construit le bouton (style aligné sur `nyx-nav-btn`).
pub fn build() -> Button {
    let btn = Button::with_label("⤓");
    btn.style_context().add_class("nyx-nav-btn");
    btn.style_context().add_class("nyx-dl-btn");
    btn.set_relief(gtk::ReliefStyle::None);
    btn.set_tooltip_text(Some("Téléchargements (Ctrl+J)"));
    btn
}
