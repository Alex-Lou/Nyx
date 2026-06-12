use std::sync::Arc;

use glib::prelude::*;
use webkit2gtk::{
    NavigationPolicyDecision, NavigationPolicyDecisionExt, PermissionRequestExt,
    PolicyDecisionExt, PolicyDecisionType, URIRequestExt, WebView, WebViewExt,
};

use crate::adblock::AdBlocker;

pub fn configure(webview: &WebView, blocker: Arc<AdBlocker>) {
    apply_settings(webview);
    wire_policy_filter(webview, blocker);
}

fn apply_settings(webview: &WebView) {
    use webkit2gtk::SettingsExt;
    let settings = WebViewExt::settings(webview).expect("WebView sans settings");

    // WebRTC leak prevention
    // (hyperlink auditing : plus besoin, WebKitGTK ≥ 2.46 le désactive en dur)
    settings.set_enable_media_stream(false);

    // Géolocalisation, notifications, micro/caméra : tout refusé par défaut
    webview.connect_permission_request(|_, request| {
        request.deny();
        true
    });
}

fn wire_policy_filter(webview: &WebView, blocker: Arc<AdBlocker>) {
    webview.connect_decide_policy(move |_wv, decision, decision_type| {
        // TODO Sprint 3.3 : filtrer aussi les sous-ressources (img, script, xhr…)
        match decision_type {
            PolicyDecisionType::NavigationAction | PolicyDecisionType::NewWindowAction => {}
            _ => return false,
        }

        if let Some(nav) = decision.downcast_ref::<NavigationPolicyDecision>() {
            let url = nav
                .navigation_action()
                .and_then(|a| a.request())
                .and_then(|r| r.uri())
                .unwrap_or_default();

            if blocker.should_block(&url) {
                decision.ignore();
                return true;
            }
        }

        false // laisser webkit gérer
    });
}

/// Normalise une entrée utilisateur en URL chargeable.
/// "google.com" → "https://google.com"
/// "rust lang"  → recherche DuckDuckGo
pub fn resolve_input(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
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
