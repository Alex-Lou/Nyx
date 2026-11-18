//! Capture des soumissions de mots de passe (Sprint 2.8) : un UserScript
//! écoute submit / clic / Entrée (SPA, login en deux temps), poste les identifiants en script-message,
//! et Nyx propose de les enregistrer dans le vault (ou de les mettre à jour).

use std::rc::Rc;

use gtk::glib::ObjectExt;

use javascriptcore::ValueExt;
use webkit2gtk::{
    UserContentInjectedFrames, UserContentManagerExt, UserScript,
    UserScriptInjectionTime, WebView, WebViewExt,
};

use vault::{Password, Vault};
use crate::state::settings::Settings;
use nyx_core::url::{host_of, scheme_of};


const CAPTURE_JS: &str = r#"
(function () {
    var KEY = '__nyx_last_user';
    var USER_RE = /user|login|email|mail|identifiant|account|compte|phone/i;
    var BTN_RE = /log ?in|sign ?in|connexion|se connecter|continuer|continue|next|suivant/i;
    var posted = {};

    function visible(el) {
        return !!(el && (el.offsetWidth || el.offsetHeight || el.getClientRects().length));
    }
    function lastUser() {
        try { return sessionStorage.getItem(KEY) || ''; } catch (_) { return ''; }
    }
    function remember(v) {
        try { if (v) sessionStorage.setItem(KEY, v); } catch (_) {}
    }
    function formOf(el) {
        return el && el.closest ? el.closest('form') : null;
    }

    // Mot de passe à envoyer : dernier champ non vide (inscription / changement).
    function findPassword(root) {
        var all = Array.prototype.filter.call(
            root.querySelectorAll('input[type="password"]'),
            function (p) { return p.value; });
        return all.length ? all[all.length - 1] : null;
    }

    // Champ identifiant : autocomplete, email, nom évocateur, sinon le champ
    // texte rempli le plus proche avant le mot de passe.
    function pickUser(root, pwd) {
        var inputs = Array.prototype.filter.call(
            root.querySelectorAll('input'),
            function (i) { return i !== pwd && i.type !== 'password' && i.value && visible(i); });
        var ac = function (i) { return (i.getAttribute('autocomplete') || '').toLowerCase(); };
        var textual = function (i) {
            return i.type === 'text' || i.type === 'tel' || i.type === 'email' || i.type === 'search';
        };
        return inputs.filter(function (i) { return /\b(username|email)\b/.test(ac(i)); })[0]
            || inputs.filter(function (i) { return i.type === 'email'; })[0]
            || inputs.filter(function (i) {
                return (i.type === 'text' || i.type === 'tel') &&
                    USER_RE.test(i.name + ' ' + i.id + ' ' + ac(i));
            })[0]
            || (pwd && inputs.filter(function (i) {
                return textual(i) &&
                    (i.compareDocumentPosition(pwd) & Node.DOCUMENT_POSITION_FOLLOWING);
            }).pop())
            || null;
    }

    function findUser(scope, pwd) {
        var form = (pwd && pwd.form) || formOf(scope);
        return (form && pickUser(form, pwd)) || pickUser(document, pwd);
    }

    // Déclencheur (submit / clic / Entrée) : poste si un mot de passe est
    // rempli, sinon mémorise l'identifiant (première étape d'un login en deux temps).
    function handle(scope) {
        var form = formOf(scope);
        var pwd = (form && findPassword(form)) || findPassword(document);
        var user = findUser(scope, pwd);
        if (user) remember(user.value);
        if (!pwd) return;
        var name = user ? user.value : lastUser();
        var key = name + '\u0000' + pwd.value;
        if (posted[key]) return;
        posted[key] = true;
        window.webkit.messageHandlers.nyx_password.postMessage(JSON.stringify({
            username: name,
            password: pwd.value,
            host: location.hostname
        }));
    }

    function submitLike(el) {
        if (!el) return false;
        var t = (el.type || '').toLowerCase();
        if (el.tagName === 'INPUT' && t === 'submit') return true;
        if (el.tagName === 'BUTTON' && t === 'submit') return true;
        if (el.tagName === 'BUTTON' && !el.getAttribute('type') && el.form) return true;
        var txt = [el.textContent, el.getAttribute('aria-label'), el.id, el.name, el.value].join(' ');
        return BTN_RE.test(txt.slice(0, 200));
    }

    document.addEventListener('submit', function (e) {
        if (e.target && e.target.querySelector) handle(e.target);
    }, true);

    document.addEventListener('click', function (e) {
        var el = e.target && e.target.closest
            ? e.target.closest('button, input[type="submit"], input[type="button"], a, [role="button"]')
            : null;
        if (submitLike(el)) handle(el);
    }, true);

    document.addEventListener('keydown', function (e) {
        var t = e.target;
        if (e.key === 'Enter' && t && t.tagName === 'INPUT' && (t.type === 'password' || !findPassword(document))) {
            handle(t);
        }
    }, true);
})();
"#;

/// Branche la capture sur une WebView. À appeler une fois par WebView.
pub fn wire(webview: &WebView, vault: Rc<Vault>, prefs: Settings) {
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
    ucm.connect_script_message_received(Some("nyx_password"), move |_, result| {
        let (Some(wv), Some(value)) = (weak.upgrade(), result.js_value()) else { return };
        // Lu à chaque message : le réglage s'applique sans redémarrage.
        if !prefs.borrow().save_passwords {
            return;
        }
        handle(&wv, &vault, &value.to_str());
    });
    // Pas de désenregistrement à la destruction : les vues liées
    // (`window.open`) partagent ce UCM, qui meurt avec la dernière d'entre
    // elles. La closure ne tient qu'une WeakRef → aucun cycle.
}

fn handle(wv: &WebView, vault: &Rc<Vault>, json: &str) {
    #[derive(serde::Deserialize)]
    struct Cred {
        username: String,
        password: String,
        #[serde(default)]
        host: String,
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
    // UCM partagé avec les vues liées : le message peut venir d'un autre
    // onglet que `wv`. Host différent → on ignore plutôt que d'enregistrer
    // l'identifiant sous le mauvais domaine.
    if !cred.host.eq_ignore_ascii_case(host) {
        return;
    }

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
