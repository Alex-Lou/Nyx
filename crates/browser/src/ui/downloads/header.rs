//! Header du popover : titre + actions globales.
//!
//! Layout (gauche → droite) :
//!   `[ Téléchargements ]  [ 🗑 Effacer ] [ 📁 Dossier ] [ 📂 Destination ] [ ⋮ ]`
//!
//! Choisir destination est UN BOUTON DIRECT — pas dans le ⋮ — parce que
//! l'imbrication MenuButton-popover dans un Popover modal cassait le
//! FileChooser (le shelf dismissait au clic sur l'item, annulant le clic).
//! En direct dans le header, on est au même niveau que bookmarks_popover
//! qui marche déjà.

use std::rc::Rc;

use gtk::prelude::*;
use gtk::{
    Box as GtkBox, FileChooserAction, FileChooserDialog, Label, Orientation,
    ResponseType, Window, WindowPosition,
};

use crate::state::downloads::DownloadsHandle;
use crate::state::settings::Settings;
use crate::ui::toast::{ToastHandle, ToastLevel};

use super::popover::Refresh;
use super::{actions, menu, widgets};

/// Construit le header complet. `on_changed` est appelé quand le store
/// est muté localement (clear all) pour que le popover ré-affiche.
/// `close_shelf` est propagé au menu ⋮ pour fermer le shelf avant un
/// FileChooser (sinon le popover modal bloque le dialog).
pub fn build(
    parent: Option<Window>,
    handle: DownloadsHandle,
    settings: Settings,
    on_changed: Refresh,
    close_shelf: Rc<dyn Fn()>,
    toaster: ToastHandle,
) -> GtkBox {
    let bar = GtkBox::new(Orientation::Horizontal, 6);
    bar.style_context().add_class("nyx-dl-header");

    let title = Label::new(Some("Téléchargements"));
    title.set_xalign(0.0);
    title.style_context().add_class("nyx-dl-title");

    let clear_btn  = widgets::icon_text("user-trash-symbolic",   "Effacer",    "Tout effacer");
    let folder_btn = widgets::icon_text("folder-open-symbolic",  "Dossier",    "Ouvrir le dossier");
    let dest_btn   = widgets::icon_text("folder-new-symbolic",   "Destination","Choisir le dossier de destination");
    let menu_btn   = menu::build(parent.clone(), settings.clone(), close_shelf.clone(), toaster.clone());

    bar.pack_start(&title, true, true, 0);
    bar.pack_end(&menu_btn,   false, false, 0);
    bar.pack_end(&dest_btn,   false, false, 0);
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
    {
        // FileChooser DIRECT depuis le header (un seul niveau de popover,
        // même pattern que bookmarks_popover qui marche). On ferme le shelf
        // AVANT d'ouvrir le chooser pour que le popover modal ne capte pas
        // les clics du dialog (cas WSL/WSLg observé).
        let s = settings;
        let t = toaster;
        let parent = parent;
        dest_btn.connect_clicked(move |_| {
            close_shelf();
            // Défère au prochain idle : laisse popdown du shelf finir
            // avant l'ouverture modale du chooser.
            let s_cl = s.clone();
            let t_cl = t.clone();
            let parent_cl = parent.clone();
            gtk::glib::idle_add_local_once(move || {
                open_destination_chooser(parent_cl, s_cl, t_cl);
            });
        });
    }

    bar
}

fn open_destination_chooser(parent: Option<Window>, settings: Settings, toaster: ToastHandle) {
    // Sous WSL : on tente l'explorateur Windows (FolderBrowserDialog
    // WinForms via powershell.exe) → rassurant pour l'utilisateur Windows
    // qui ne veut pas d'UI Linux à l'intérieur de Windows.
    // Hors WSL OU si powershell ne répond pas : fallback FileChooserDialog
    // GTK in-process (themable via .nyx-filechooser).
    if let Some(picked) = crate::platform::wsl_pick_windows_folder(
        "Dossier de téléchargement Nyx"
    ) {
        let s = picked.to_string_lossy().into_owned();
        settings.borrow_mut().downloads_dir = Some(s.clone());
        toaster.push(ToastLevel::Info, &format!(
            "Dossier de téléchargement : {}",
            short_path(&s),
        ));
        return;
    }

    // FileChooserDialog (pas FileChooserNative) : in-process, donc notre
    // CSS .nyx-filechooser s'applique. FileChooserNative passe par le
    // portail XDG → autre process → notre theme ne le touche pas.
    let dlg = FileChooserDialog::new(
        Some("Dossier de téléchargement"),
        parent.as_ref(),
        FileChooserAction::SelectFolder,
    );
    dlg.add_button("Annuler", ResponseType::Cancel);
    let select = dlg.add_button("Sélectionner", ResponseType::Accept);
    select.style_context().add_class("suggested-action");
    dlg.set_modal(true);
    dlg.set_position(WindowPosition::CenterOnParent);
    dlg.set_default_size(720, 460);
    dlg.style_context().add_class("nyx-filechooser");

    if let Some(current) = current_dir(&settings) {
        let _ = dlg.set_current_folder(current);
    }

    let response = dlg.run();
    if response == ResponseType::Accept {
        if let Some(path) = dlg.filename() {
            let s = path.to_string_lossy().into_owned();
            settings.borrow_mut().downloads_dir = Some(s.clone());
            toaster.push(ToastLevel::Info, &format!(
                "Dossier de téléchargement : {}",
                short_path(&s),
            ));
        }
    }
    dlg.close();
}

fn current_dir(settings: &Settings) -> Option<std::path::PathBuf> {
    let user = settings.borrow().downloads_dir.clone();
    user.filter(|s| !s.is_empty()).map(std::path::PathBuf::from)
        .or_else(crate::platform::default_downloads_dir)
}

fn short_path(path: &str) -> String {
    const MAX: usize = 48;
    if path.chars().count() <= MAX { path.to_string() }
    else {
        let n = path.chars().count();
        let tail: String = path.chars().skip(n - MAX + 1).collect();
        format!("…{tail}")
    }
}
