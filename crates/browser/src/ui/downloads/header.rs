//! Header du popover : titre + actions globales.
//!
//! Layout (gauche → droite) :
//!   `[ Téléchargements ]    [ 🗑 Effacer ] [ 📁 Dossier ] [ ⋮ ]`
//!
//! Boutons icon+text courts → visibles, hover net, tooltip court qui ne
//! déborde jamais hors écran. Tout passe par `widgets::icon_text` /
//! `widgets::icon_only` pour ne pas dupliquer le style.

use gtk::prelude::*;
use gtk::{Box as GtkBox, Label, Orientation, Window};

use crate::state::downloads::DownloadsHandle;
use crate::state::settings::Settings;

use super::popover::Refresh;
use super::{actions, menu, widgets};

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

    let clear_btn  = widgets::icon_text("user-trash-symbolic",  "Effacer", "Tout effacer");
    let folder_btn = widgets::icon_text("folder-open-symbolic", "Dossier", "Ouvrir le dossier");
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
        let s = settings;
        folder_btn.connect_clicked(move |_| actions::open_downloads_folder(&s));
    }

    bar
}
