use std::path::PathBuf;

use gtk::prelude::*;
use gtk::{Dialog, Entry, InputPurpose, Label, ResponseType};
use vault::Vault;

/// Emplacement XDG du vault : ~/.local/share/nyx/vault.db
fn vault_path() -> PathBuf {
    let dir = glib::user_data_dir().join("nyx");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("vault.db")
}

/// Dialogue de déverrouillage au démarrage (Sprint 2.1).
/// Boucle tant que la passphrase est mauvaise ; None si l'utilisateur quitte.
/// Au premier lancement, le vault est créé (passphrase + confirmation).
pub fn unlock_vault() -> Option<Vault> {
    let path = vault_path();
    let creating = !path.exists();

    let dialog = Dialog::builder()
        .title("Nyx — Vault")
        .modal(true)
        .resizable(false)
        .build();
    dialog.style_context().add_class("nyx-unlock");
    dialog.add_button("Quitter", ResponseType::Cancel);
    dialog.add_button(
        if creating { "Créer le vault" } else { "Déverrouiller" },
        ResponseType::Ok,
    );
    dialog.set_default_response(ResponseType::Ok);

    let title = Label::new(Some(if creating {
        "Choisis la passphrase de ton nouveau vault"
    } else {
        "Passphrase du vault"
    }));
    title.style_context().add_class("nyx-unlock-title");

    let passphrase = password_entry("passphrase");
    let confirm = password_entry("confirme la passphrase");

    let error = Label::new(None);
    error.style_context().add_class("nyx-error");
    error.set_no_show_all(true);

    let content = dialog.content_area();
    content.set_spacing(10);
    content.set_margin(18);
    content.pack_start(&title, false, false, 0);
    content.pack_start(&passphrase, false, false, 0);
    if creating {
        content.pack_start(&confirm, false, false, 0);
    }
    content.pack_start(&error, false, false, 0);

    dialog.show_all();
    passphrase.grab_focus();

    let show_error = |msg: &str| {
        error.set_text(msg);
        error.show();
        passphrase.select_region(0, -1);
        passphrase.grab_focus();
    };

    loop {
        if dialog.run() != ResponseType::Ok {
            dialog.close();
            return None;
        }

        let pass = passphrase.text();
        if pass.is_empty() {
            show_error("La passphrase est vide");
            continue;
        }
        if creating && pass != confirm.text() {
            show_error("Les passphrases ne correspondent pas");
            continue;
        }

        match Vault::open(&path, &pass) {
            Ok(vault) => {
                dialog.close();
                return Some(vault);
            }
            Err(_) => show_error("Passphrase invalide"),
        }
    }
}

fn password_entry(placeholder: &str) -> Entry {
    let entry = Entry::builder()
        .visibility(false)
        .input_purpose(InputPurpose::Password)
        .activates_default(true)
        .placeholder_text(placeholder)
        .build();
    entry.style_context().add_class("nyx-urlbar");
    entry
}
