//! Dialog de confirmation Ask / AskDanger.
//!
//! L'utilisateur a deux niveaux : `Ask` (Question icon, bouton normal),
//! `AskDanger` (Warning icon, bouton `destructive-action` rouge).
//!
//! Non-bloquant : le résultat est livré via `callback(bool)`.
//! `true` = conserver le fichier (déclenche move+meta), `false` = jeter.

use std::cell::RefCell;

use gtk::prelude::*;
use gtk::{
    ButtonsType, DialogFlags, MessageDialog, MessageType, ResponseType, Window,
};

use nyx_core::download_policy::{Kind, Verdict};

pub fn show<F: FnOnce(bool) + 'static>(
    verdict: Verdict,
    filename: String,
    kind: Kind,
    callback: F,
) {
    let (mtype, header) = match verdict {
        Verdict::AskDanger => (MessageType::Warning,  format!("⚠ « {filename} »")),
        _                  => (MessageType::Question, format!("Conserver « {filename} » ?")),
    };
    let body = match kind {
        Kind::Executable => "Ce fichier est un exécutable. Ouvrir un binaire \
                            téléchargé peut compromettre votre système.",
        Kind::MacroDoc   => "Ce document contient des macros, qui peuvent \
                            exécuter du code arbitraire à l'ouverture.",
        Kind::Archive    => "Cette archive peut contenir n'importe quel type \
                            de fichier. Vérifiez son contenu avant extraction.",
        Kind::ActiveDoc  => "Ce document peut embarquer du contenu actif \
                            (script, redirection).",
        _ => "Type de fichier inconnu — origine et nom incohérents.",
    };

    let dlg = MessageDialog::new(
        None::<&Window>,
        DialogFlags::MODAL,
        mtype,
        ButtonsType::None,
        &format!("{header}\n\n{body}"),
    );
    dlg.add_button("Jeter", ResponseType::Cancel);
    let accept = dlg.add_button("Conserver", ResponseType::Yes);
    if verdict == Verdict::AskDanger {
        accept.style_context().add_class("destructive-action");
    }

    // `connect_response` peut firer plusieurs fois (clic Esc puis bouton) ;
    // RefCell<Option<F>>::take garantit un appel unique au callback.
    let slot: RefCell<Option<F>> = RefCell::new(Some(callback));
    dlg.connect_response(move |d, resp| {
        let approved = resp == ResponseType::Yes;
        if let Some(cb) = slot.borrow_mut().take() {
            cb(approved);
        }
        d.close();
    });
    dlg.show_all();
}
