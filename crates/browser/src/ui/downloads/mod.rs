//! Shelf de téléchargements GTK — bouton + popover + lignes + actions.
//!
//! Surface publique : [`install`] — crée le bouton, attache le popover,
//! retourne le bouton prêt à packer dans la navbar.
//!
//! Architecture (Single Responsibility) :
//!   - `button`   : factory du bouton ⤓ (style aligné `nyx-nav-btn`)
//!   - `popover`  : structure générale + fade in/out + refresh
//!   - `header`   : titre + boutons « tout effacer » / « ouvrir le dossier »
//!   - `menu`     : menu ⋮ (choisir destination + toggle re-check)
//!   - `row`      : une ligne de la liste (filename + status + actions)
//!   - `actions`  : I/O OS (open / reveal / run avec re-check policy)
//!
//! Aucun de ces sous-modules n'expose de pub vers l'extérieur du crate :
//! la frontière publique est `install`.

mod actions;
mod button;
mod header;
mod menu;
mod popover;
mod row;
mod widgets;

use gtk::prelude::*;
use gtk::Button;

use crate::state::downloads::DownloadsHandle;
use crate::state::settings::Settings;
use crate::ui::toast::ToastHandle;

/// Crée le bouton et lui attache le popover. À packer dans la navbar.
pub fn install(
    handle: &DownloadsHandle, settings: &Settings, toaster: &ToastHandle,
) -> Button {
    let btn = button::build();
    let pop = popover::build(&btn, handle.clone(), settings.clone(), toaster.clone());
    btn.connect_clicked(move |_| pop.popup());
    btn
}
