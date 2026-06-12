//! Capture des soumissions de mots de passe (Sprint 2.8) : un UserScript
//! écoute les submit de formulaires, poste les identifiants en script-message,
//! et Nyx propose de les enregistrer dans le vault (ou de les mettre à jour).

use std::rc::Rc;

use javascriptcore::ValueExt;
use webkit2gtk::{
    UserContentInjectedFrames, UserContentManagerExt, UserScript,
    UserScriptInjectionTime, WebView, WebViewExt,
};

use vault::{Password, Vault};

use crate::web::nyxguard::extract_host;

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

    let wv = webview.clone();
    ucm.connect_script_message_received(Some("nyx_password"), move |_, result| {
        if let Some(value) = result.js_value() {
            handle(&wv, &vault, &value.to_str());
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
    let host = extract_host(&uri);
    if host.is_empty() || uri.starts_with("nyx://") || uri.starts_with("file://") {
        return;
    }

    let existing = vault.passwords_for(host).unwrap_or_default();
    let who = if cred.username.is_empty() { "ce compte" } else { &cred.username };

    match existing.iter().find(|p| p.username == cred.username) {
        Some(known) if known.password == cred.password => {} // déjà à jour
        Some(known) => {
            let msg = format!("Mettre à jour le mot de passe de {who} sur {host} ?");
            if let Some(id) = known.id {
                if ask(wv, &msg) {
                    let _ = vault.update_password(id, &cred.password);
                }
            }
        }
        None => {
            let msg = format!("Enregistrer le mot de passe de {who} sur {host} ?");
            if ask(wv, &msg) {
                let _ = vault.add_password(&Password::new(host, cred.username, cred.password));
            }
        }
    }
}

fn ask(wv: &WebView, message: &str) -> bool {
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
    let response = dialog.run();
    dialog.close();
    response == ResponseType::Yes
}
