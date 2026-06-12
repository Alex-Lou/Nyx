use std::rc::Rc;

use glib::prelude::*;
use javascriptcore::ValueExt;
use webkit2gtk::{
    NavigationPolicyDecision, NavigationPolicyDecisionExt, PermissionRequestExt,
    PolicyDecisionExt, PolicyDecisionType, URIRequestExt, UserContentInjectedFrames,
    UserContentManagerExt, UserScript, UserScriptInjectionTime, WebView, WebViewExt,
};

use vault::{Password, Vault};

use crate::adblock::AdBlocker;
use crate::content_filter;
use crate::urls;

/// Prépare une WebView : settings privacy, content filter (sous-ressources),
/// filtre de navigation. `on_block` est appelé à chaque blocage (compteur).
pub fn configure(webview: &WebView, blocker: Rc<AdBlocker>, on_block: impl Fn() + 'static) {
    apply_settings(webview);
    if let Some(ucm) = webview.user_content_manager() {
        content_filter::apply_to(&ucm);
    }
    wire_policy_filter(webview, blocker, on_block);
}

/// User-agent neutre (Sprint 5.5) : même façade qu'Epiphany — un Safari
/// générique, le pool d'utilisateurs le plus large possible.
const NEUTRAL_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
    AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15";

fn apply_settings(webview: &WebView) {
    use webkit2gtk::SettingsExt;
    let settings = WebViewExt::settings(webview).expect("WebView sans settings");

    // WebRTC leak prevention
    // (hyperlink auditing : plus besoin, WebKitGTK ≥ 2.46 le désactive en dur)
    settings.set_enable_media_stream(false);

    settings.set_user_agent(Some(NEUTRAL_UA));

    // Géolocalisation, notifications, micro/caméra : tout refusé par défaut
    webview.connect_permission_request(|_, request| {
        request.deny();
        true
    });
}

fn wire_policy_filter(webview: &WebView, blocker: Rc<AdBlocker>, on_block: impl Fn() + 'static) {
    webview.connect_decide_policy(move |wv, decision, decision_type| {
        // Ici : navigations et popups. Les sous-ressources (img, script,
        // xhr…) sont bloquées par le content filter WebKit (content_filter.rs).
        match decision_type {
            PolicyDecisionType::NavigationAction | PolicyDecisionType::NewWindowAction => {}
            _ => return false,
        }

        let Some(nav) = decision.downcast_ref::<NavigationPolicyDecision>() else {
            return false;
        };
        let url = nav
            .navigation_action()
            .and_then(|a| a.request())
            .and_then(|r| r.uri())
            .unwrap_or_default();
        if url.is_empty() {
            return false;
        }

        let source = wv.uri().unwrap_or_default();
        let main_frame = decision_type == PolicyDecisionType::NavigationAction
            && nav.navigation_action().is_some_and(|mut a| a.frame_name().is_none());

        // Whitelist (Sprint 3.4) : sur navigation principale, activer ou
        // couper le content filter selon le site visité.
        if main_frame {
            if let Some(ucm) = wv.user_content_manager() {
                let (host, _) = urls::host_and_path(&url);
                if blocker.is_whitelisted(host) {
                    content_filter::remove_from(&ucm);
                } else {
                    content_filter::apply_to(&ucm);
                }
            }
        }

        if blocker.should_block(&url, &source, main_frame) {
            decision.ignore();
            on_block();
            return true;
        }

        false // laisser webkit gérer
    });
}

/// Script injecté dans chaque page : capture les soumissions de formulaires
/// contenant un champ mot de passe (Sprint 2.8).
const PASSWORD_CAPTURE_JS: &str = r#"
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

/// Propose d'enregistrer les identifiants soumis dans les pages (Sprint 2.8).
pub fn wire_password_capture(webview: &WebView, vault: Rc<Vault>) {
    let Some(ucm) = webview.user_content_manager() else { return };
    if !ucm.register_script_message_handler("nyx_password") {
        return;
    }
    ucm.add_script(&UserScript::new(
        PASSWORD_CAPTURE_JS,
        UserContentInjectedFrames::TopFrame,
        UserScriptInjectionTime::End,
        &[],
        &[],
    ));

    let wv = webview.clone();
    ucm.connect_script_message_received(Some("nyx_password"), move |_, result| {
        if let Some(value) = result.js_value() {
            handle_captured_credentials(&wv, &vault, &value.to_str());
        }
    });
}

fn handle_captured_credentials(wv: &WebView, vault: &Rc<Vault>, json: &str) {
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
    let (host, _) = urls::host_and_path(&uri);
    if host.is_empty() {
        return;
    }

    let existing = vault.passwords_for(host).unwrap_or_default();
    let who = if cred.username.is_empty() { "ce compte" } else { &cred.username };

    match existing.iter().find(|p| p.username == cred.username) {
        Some(known) if known.password == cred.password => {} // déjà à jour
        Some(known) => {
            let msg = format!("Mettre à jour le mot de passe de {} sur {} ?", who, host);
            if let Some(id) = known.id {
                if ask(wv, &msg) {
                    let _ = vault.update_password(id, &cred.password);
                }
            }
        }
        None => {
            let msg = format!("Enregistrer le mot de passe de {} sur {} ?", who, host);
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

/// Normalise une entrée utilisateur en URL chargeable.
/// "google.com" → "https://google.com"
/// "rust lang"  → recherche DuckDuckGo
pub fn resolve_input(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("nyx://")
    {
        return trimmed.to_string();
    }
    if trimmed.contains('.') && !trimmed.contains(' ') {
        return format!("https://{}", trimmed);
    }
    // Fallback : DuckDuckGo, pas Google
    format!("https://duckduckgo.com/?q={}", percent_encode(trimmed))
}

/// Percent-encoding RFC 3986 : seuls les unreserved passent tels quels,
/// l'espace devient '+' (forme query).
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_complete_inchangee() {
        assert_eq!(resolve_input("https://rust-lang.org"), "https://rust-lang.org");
        assert_eq!(resolve_input("http://example.com"), "http://example.com");
        assert_eq!(resolve_input("nyx://start"), "nyx://start");
    }

    #[test]
    fn domaine_nu_devient_https() {
        assert_eq!(resolve_input("google.com"), "https://google.com");
        assert_eq!(resolve_input("  doc.rust-lang.org  "), "https://doc.rust-lang.org");
    }

    #[test]
    fn texte_devient_recherche_ddg() {
        assert_eq!(
            resolve_input("rust lang"),
            "https://duckduckgo.com/?q=rust+lang"
        );
    }

    #[test]
    fn encodage_caracteres_speciaux() {
        assert_eq!(percent_encode("a&b #c"), "a%26b+%23c");
        assert_eq!(percent_encode("50%"), "50%25");
        assert_eq!(percent_encode("q=x?y"), "q%3Dx%3Fy");
        assert_eq!(percent_encode("été"), "%C3%A9t%C3%A9");
    }
}
