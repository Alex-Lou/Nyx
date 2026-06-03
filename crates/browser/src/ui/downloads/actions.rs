//! Actions sur une ligne / globalement — la couche qui touche au disque.
//!
//! Aucune logique de policy ici : `actions::run` ré-évalue via
//! `download_policy` SEULEMENT si `settings.recheck_on_run` est vrai.
//! La sélection de la décision (ouvrir / refuser / demander) est faite
//! par cette policy, pas localement.

use std::path::{Path, PathBuf};

use gtk::prelude::*;
use gtk::{ButtonsType, DialogFlags, MessageDialog, MessageType, ResponseType, Window};

use nyx_core::download_policy::Kind;
use nyx_core::downloads::DownloadEntry;
use nyx_core::magic_bytes;

use crate::platform;
use crate::state::settings::Settings;

/// Ouvre le dossier de téléchargements (la « racine », pas un fichier).
pub fn open_downloads_folder(settings: &Settings) {
    let dir = resolve_downloads_dir(settings);
    if let Some(d) = dir {
        let _ = platform::open_path(&d);
    }
}

/// Révèle le fichier dans le file manager OS (sélectionné si possible).
pub fn reveal(entry: &DownloadEntry) {
    let _ = platform::reveal_in_file_manager(Path::new(&entry.dest_path));
}

/// Ouvre / exécute le fichier selon son type.
///
/// Si `recheck_on_run` est actif et que le contenu réel est exécutable
/// (sniff des magic bytes), affiche un dialog de confirmation. L'utilisateur
/// peut désactiver ce re-check dans les paramètres généraux.
pub fn run(entry: &DownloadEntry, settings: &Settings, parent: Option<&Window>) {
    let recheck = settings.borrow().recheck_on_run;
    let path = PathBuf::from(&entry.dest_path);

    if !recheck {
        let _ = platform::open_path(&path);
        return;
    }

    let live_kind = sniff_kind(&path).unwrap_or(entry.kind);
    match live_kind {
        Kind::Executable | Kind::MacroDoc => confirm_then_open(entry, &path, live_kind, parent),
        Kind::Archive => {
            // Pour les archives, on révèle plutôt que d'ouvrir l'archive
            // directement : ça réduit la surface (clic involontaire = run).
            let _ = platform::reveal_in_file_manager(&path);
        }
        _ => { let _ = platform::open_path(&path); }
    }
}

fn confirm_then_open(entry: &DownloadEntry, path: &Path, kind: Kind, parent: Option<&Window>) {
    let kind_label = match kind {
        Kind::Executable => "exécutable",
        Kind::MacroDoc   => "document avec macros",
        _ => "binaire",
    };
    let msg = format!(
        "Ouvrir « {} » ?\n\nType détecté : {kind_label}.\nCe fichier peut modifier votre système.",
        entry.filename,
    );
    let dlg = MessageDialog::new(
        parent,
        DialogFlags::MODAL,
        MessageType::Warning,
        ButtonsType::None,
        &msg,
    );
    dlg.add_button("Annuler", ResponseType::Cancel);
    let open_btn = dlg.add_button("Ouvrir quand même", ResponseType::Yes);
    open_btn.style_context().add_class("destructive-action");

    let resp = dlg.run();
    dlg.close();
    if resp == ResponseType::Yes {
        let _ = platform::open_path(path);
    }
}

fn sniff_kind(path: &Path) -> Option<Kind> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = vec![0u8; 8192];
    let n = f.read(&mut head).ok()?;
    head.truncate(n);
    Some(magic_bytes::category(magic_bytes::sniff(&head)))
}

fn resolve_downloads_dir(settings: &Settings) -> Option<PathBuf> {
    let user = settings.borrow().downloads_dir.clone();
    if let Some(d) = user.filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(d));
    }
    platform::default_downloads_dir()
}
