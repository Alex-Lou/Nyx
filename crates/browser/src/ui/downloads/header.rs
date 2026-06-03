//! Header du popover : titre + actions globales.
//!
//! Layout (gauche → droite) :
//!   `[ Téléchargements ]              [🗑] [📁] [⋮]`
//!
//! - 🗑 « Tout effacer »  → `clear_all()` côté store (n'efface PAS le disque).
//! - 📁 « Ouvrir le dossier » → file manager OS sur le dossier de destination.
//! - ⋮  → menu (`menu::build`).
//!
//! Le `refresh` est passé en callback : quand on clear, le header redemande
//! au popover de re-render la liste.

use gtk::prelude::*;
use gtk::{Box as GtkBox, Button, Label, Orientation, Window};

use crate::state::downloads::DownloadsHandle;
use crate::state::settings::Settings;

use super::popover::Refresh;
use super::{actions, menu};

/// Construit le header complet. `on_changed` est appelé quand le store
/// est muté localement (clear all) pour que le popover ré-affiche.
pub fn build(
    parent: Option<Window>,
    handle: DownloadsHandle,
    settings: Settings,
    on_changed: Refresh,
) -> GtkBox {
    let bar = GtkBox::new(Orientation::Horizontal, 6);
    bar.style_context().add_class("nyx-dl-header");

    let title = Label::new(Some("Téléchargements"));
    title.set_xalign(0.0);
    title.style_context().add_class("nyx-dl-title");

    let clear_btn  = flat("🗑", "Tout effacer");
    let folder_btn = flat("📁", "Ouvrir le dossier des téléchargements");
    let menu_btn   = menu::build(parent, settings.clone());

    bar.pack_start(&title, true, true, 0);
    bar.pack_end(&menu_btn,   false, false, 0);
    bar.pack_end(&folder_btn, false, false, 0);
    bar.pack_end(&clear_btn,  false, false, 0);

    {
        let (h, cb) = (handle, on_changed);
        clear_btn.connect_clicked(move |_| {
            h.borrow_mut().clear_all();
            cb();
        });
    }
    {
        let s = settings.clone();
        folder_btn.connect_clicked(move |_| actions::open_downloads_folder(&s));
    }

    bar
}

fn flat(label: &str, tooltip: &str) -> Button {
    let btn = Button::with_label(label);
    btn.set_relief(gtk::ReliefStyle::None);
    btn.set_tooltip_text(Some(tooltip));
    btn.style_context().add_class("nyx-nav-btn");
    btn
}
