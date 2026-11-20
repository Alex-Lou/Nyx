//! Réglages → « Mot de passe principal » : dialogue GTK natif, ouvert par
//! l'action interne `nyx://master/set|remove` (verdict
//! `security::Verdict::MasterPassword`, page émettrice interne uniquement).
//! Les mots de passe sont saisis ICI, jamais dans une URL ni dans la page.
//! Les transitions (rekey vérifié, fichier de clé) vivent dans
//! `vault::keymode::KeyStore`.

use gtk::prelude::*;
use gtk::{ResponseType, Window};
use nyx_core::security::MasterOp;
use vault::keymode;
use vault::Vault;

use crate::ui::unlock::{key_store_for, password_entry, take_secret, Card};

/// Ouvre le dialogue adapté au mode courant. `true` si le mode a changé.
pub fn run(parent: Option<&Window>, op: MasterOp, vault: &Vault) -> bool {
    match (op, vault.has_master_password()) {
        (MasterOp::Set, false) => define(parent, vault),
        (MasterOp::Set, true) => change(parent, vault),
        (MasterOp::Remove, true) => remove(parent, vault),
        (MasterOp::Remove, false) => false,
    }
}

fn define(parent: Option<&Window>, vault: &Vault) -> bool {
    let card = Card::new(
        parent,
        "Définir un mot de passe principal",
        "Il sera demandé à chaque lancement de Nyx et protège tes données même \
         si quelqu'un accède à ta session ouverte. Irrécupérable si oublié.",
    );
    let new = password_entry("nouveau mot de passe (8 caractères min.)");
    let confirm = password_entry("confirme le mot de passe");
    card.fields.pack_start(&new, false, false, 0);
    card.fields.pack_start(&confirm, false, false, 0);
    let ks = key_store_for(vault.path().to_path_buf());
    run_card(&card, &new, "Définir", || {
        keymode::check_new_passphrase(&new.text(), &confirm.text()).map_err(str::to_string)?;
        confirm.set_text("");
        ks.set_master(vault, &take_secret(&new)).map_err(|e| e.to_string())
    })
}

fn change(parent: Option<&Window>, vault: &Vault) -> bool {
    let card = Card::new(parent, "Changer le mot de passe principal", "Saisis l'actuel, puis le nouveau.");
    let old = password_entry("mot de passe actuel");
    let new = password_entry("nouveau mot de passe (8 caractères min.)");
    let confirm = password_entry("confirme le nouveau mot de passe");
    for e in [&old, &new, &confirm] {
        card.fields.pack_start(e, false, false, 0);
    }
    let ks = key_store_for(vault.path().to_path_buf());
    run_card(&card, &old, "Changer", || {
        keymode::check_new_passphrase(&new.text(), &confirm.text()).map_err(str::to_string)?;
        confirm.set_text("");
        ks.change_master(vault, &take_secret(&old), &take_secret(&new)).map_err(|e| e.to_string())
    })
}

fn remove(parent: Option<&Window>, vault: &Vault) -> bool {
    let card = Card::new(
        parent,
        "Retirer le mot de passe principal",
        "Nyx ouvrira le coffre sans rien demander, avec une clé gardée par ta \
         session Linux. Toute personne ayant accès à ta session ouverte pourra \
         lire les mots de passe enregistrés.",
    );
    let current = password_entry("mot de passe actuel");
    card.fields.pack_start(&current, false, false, 0);
    let ks = key_store_for(vault.path().to_path_buf());
    run_card(&card, &current, "Retirer", || {
        ks.remove_master(vault, &take_secret(&current)).map_err(|e| e.to_string())
    })
}

/// Boucle du dialogue : `submit` tente l'opération, l'erreur s'affiche et
/// on recommence ; `true` si l'opération a réussi.
fn run_card(card: &Card, first: &gtk::Entry, accept: &str, submit: impl Fn() -> Result<(), String>) -> bool {
    card.secondary_button("Annuler", ResponseType::Cancel);
    card.primary_button(accept, ResponseType::Ok);
    card.dialog.set_default_response(ResponseType::Ok);
    card.dialog.show_all();
    first.grab_focus();
    loop {
        if card.dialog.run() != ResponseType::Ok {
            card.dialog.close();
            return false;
        }
        match submit() {
            Ok(()) => return card.close(()).is_some(),
            Err(msg) => card.error(&msg, Some(first)),
        }
    }
}
