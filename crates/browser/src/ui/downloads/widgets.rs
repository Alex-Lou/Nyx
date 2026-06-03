//! Constructeurs de boutons réutilisés par `header` et `row`.
//!
//! Centralise les deux variantes utilisées dans la shelf :
//!   - **icon-text** : icône symbolique GTK + label texte court (header).
//!   - **icon-only** : icône symbolique seule, taille bouton (rows).
//!
//! Pourquoi des `Image::from_icon_name(...)` symboliques plutôt que des
//! emojis (🗑 📁 …) ? Les emojis dépendent de la police installée et
//! rendent souvent à zéro pixel sous GTK3 minimal — le user voyait
//! « aucun bouton réel ». Les icônes Adwaita symboliques sont garanties
//! présentes et s'adaptent automatiquement au thème (couleur héritée).

use gtk::prelude::*;
use gtk::{Box as GtkBox, Button, IconSize, Image, Label, Orientation};

const BTN_CLASS:    &str = "nyx-nav-btn";
const ACTION_CLASS: &str = "nyx-dl-action";

/// Bouton « icône + texte court ». Pour le header.
pub fn icon_text(icon_name: &str, text: &str, tooltip: &str) -> Button {
    let btn = Button::new();
    style(&btn, tooltip);

    let row = GtkBox::new(Orientation::Horizontal, 4);
    row.pack_start(&icon(icon_name), false, false, 0);
    let lbl = Label::new(Some(text));
    lbl.style_context().add_class("nyx-dl-action-label");
    row.pack_start(&lbl, false, false, 0);
    btn.add(&row);
    btn
}

/// Bouton « icône seule » carré — pour les rows (effacer, ouvrir, dossier).
pub fn icon_only(icon_name: &str, tooltip: &str) -> Button {
    let btn = Button::new();
    btn.set_image(Some(&icon(icon_name)));
    btn.set_always_show_image(true);
    style(&btn, tooltip);
    btn
}

fn icon(name: &str) -> Image {
    let img = Image::from_icon_name(Some(name), IconSize::Button);
    img.style_context().add_class("nyx-dl-icon");
    img
}

fn style(btn: &Button, tooltip: &str) {
    btn.set_relief(gtk::ReliefStyle::None);
    btn.set_tooltip_text(Some(tooltip));
    btn.style_context().add_class(BTN_CLASS);
    btn.style_context().add_class(ACTION_CLASS);
}
