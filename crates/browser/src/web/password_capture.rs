//! Capture des soumissions de mots de passe (Sprint 2.8) : un UserScript
//! écoute les submit de formulaires, poste les identifiants en script-message,
//! et Nyx propose de les enregistrer dans le vault (ou de les mettre à jour).

use std::rc::Rc;

use gtk::glib::ObjectExt;
use gtk::prelude::WidgetExt;

use javascriptcore::ValueExt;
use webkit2gtk::{
    UserContentInjectedFrames, UserContentManagerExt, UserScript,
    UserScriptInjectionTime, WebView, WebViewExt,
};

use vault::{Password, Vault};
use nyx_core::url::{host_of, scheme_of};


const CAPTURE_JS: &str = r#"
(function () {
    document.addEventListener('submit', function (e) {
        var form = e.target;
        if (!form || !form.querySelector) return;
        var pwd = form.querySelector('input[type="password"]');
        if (!pwd || !pwd.value) return;
        var user = form.querySelector('input[type="email"], input[type="text"], input[type="tel"]');
        window.webkit.messageHandlers.nyx_password.postMessage(JSON.stringify({
            username: user ? user.value : '',
            password: pwd.value
        }));
    }, true);
})();
"#;

/// Branche la capture sur une WebView. À appeler une fois par WebView.
pub fn wire(webview: &WebView, vault: Rc<Vault>) {
    let Some(ucm) = webview.user_content_manager() else { return };
    if !ucm.register_script_message_handler("nyx_password") {
        return;
    }
    ucm.add_script(&UserScript::new(
        CAPTURE_JS,
        UserContentInjectedFrames::TopFrame,
        UserScriptInjectionTime::End,
        &[],
        &[],
    ));

    // WeakRef : le UCM (détenu par la vue) ne doit pas retenir la vue.
    let weak = webview.downgrade();
    let id = ucm.connect_script_message_received(Some("nyx_password"), move |_, result| {
        let (Some(wv), Some(value)) = (weak.upgrade(), result.js_value()) else { return };
        handle(&wv, &vault, &value.to_str());
    });
    // À la destruction de la vue : on débranche le handler et on le désenregistre.
    let ucm_cell = std::cell::RefCell::new(Some((ucm, id)));
    webview.connect_destroy(move |_| {
        if let Some((ucm, id)) = ucm_cell.borrow_mut().take() {
            ucm.disconnect(id);
            ucm.unregister_script_message_handler("nyx_password");
        }
    });
}

fn handle(wv: &WebView, vault: &Rc<Vault>, json: &str) {
    #[derive(serde::Deserialize)]
    struct Cred {
        username: String,
        password: String,
    }

    let Ok(cred) = serde_json::from_str::<Cred>(json) else { return };
    if cred.password.is_empty() {
        return;
    }
    let Some(uri) = wv.uri() else { return };
    if !matches!(scheme_of(&uri).as_deref(), Some("http" | "https")) {
        return;
    }
    let Some(host) = host_of(&uri) else { return };
    let host = host.as_ref();

    let existing = vault.passwords_for(host).unwrap_or_default();
    let who = if cred.username.is_empty() { "ce compte" } else { &cred.username };

    match existing.iter().find(|p| p.username == cred.username) {
        Some(known) if known.password == cred.password => {} // déjà à jour
        Some(known) => {
            let msg = format!("Mettre à jour le mot de passe de {who} sur {host} ?");
            if let Some(id) = known.id {
                let (vault, pwd) = (vault.clone(), cred.password);
                ask(wv, &msg, move || {
                    let _ = vault.update_password(id, &pwd);
                });
            }
        }
        None => {
            let msg = format!("Enregistrer le mot de passe de {who} sur {host} ?");
            let (vault, host) = (vault.clone(), host.to_string());
            ask(wv, &msg, move || {
                let _ = vault.add_password(&Password::new(&host, cred.username, cred.password));
            });
        }
    }
}

/// Question non bloquante : `on_yes` est appelé si l'utilisateur répond Oui.
fn ask(wv: &WebView, message: &str, on_yes: impl FnOnce() + 'static) {
    use gtk::prelude::*;
    use gtk::{ButtonsType, MessageDialog, MessageType, ResponseType};

    let dialog = MessageDialog::builder()
        .message_type(MessageType::Question)
        .buttons(ButtonsType::YesNo)
        .text(message)
        .modal(true)
        .build();
    if let Some(parent) = wv.toplevel().and_then(|w| w.downcast::<gtk::Window>().ok()) {
        dialog.set_transient_for(Some(&parent));
    }
    let on_yes = std::cell::RefCell::new(Some(on_yes));
    dialog.connect_response(move |d, response| {
        d.close();
        if let (ResponseType::Yes, Some(f)) = (response, on_yes.borrow_mut().take()) {
            f();
        }
    });
    dialog.show();
}
