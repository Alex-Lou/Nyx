use std::sync::Arc;

use gtk::prelude::*;
use webkit2gtk::{
    NavigationPolicyDecision, NavigationPolicyDecisionExt, PolicyDecisionExt,
    PolicyDecisionType, URIRequestExt, WebView, WebViewExt,
};

use crate::adblock::AdBlocker;

pub fn configure(webview: &WebView, blocker: Arc<AdBlocker>) {
    apply_settings(webview);
    wire_policy_filter(webview, blocker);
}

fn apply_settings(webview: &WebView) {
    use webkit2gtk::SettingsExt;
    let s = WebViewExt::settings(webview).expect("WebView sans Settings");

    // Privacy & anti-tracking
    s.set_enable_hyperlink_auditing(false);
    s.set_enable_media_stream(false);           // WebRTC leak prevention

    // Sécurité
    s.set_javascript_can_open_windows_automatically(false); // bloque les popups
    s.set_enable_developer_extras(false);       // pas de "Inspecter l'élément"

    // UX
    s.set_enable_smooth_scrolling(true);

    // Java / NPAPI : dépréciés depuis WebKit 2.32–2.38, désactivés par défaut
    // dans WebKit 4.1. Pas appelés pour éviter les warnings de compilation.
    // Géolocalisation : permission-request depuis WebKit 4.1 (Sprint 5).
    // Cookies tiers : WebsiteDataManager (Sprint 5).
}

fn wire_policy_filter(webview: &WebView, blocker: Arc<AdBlocker>) {
    webview.connect_decide_policy(move |_wv, decision, dtype| {
        if dtype != PolicyDecisionType::NavigationAction {
            return false;
        }
        if let Ok(nav) = decision.clone().downcast::<NavigationPolicyDecision>() {
            let url = nav
                .navigation_action()
                .and_then(|a| a.request())
                .and_then(|r| r.uri())
                .map(|s| s.to_string())
                .unwrap_or_default();
            if blocker.should_block(&url) {
                decision.ignore();
                return true;
            }
        }
        false
    });
}

/// Normalise une entrée utilisateur en URL chargeable.
///
/// Schémas reconnus passent tels quels. Les schémas non-web
/// (`javascript:`, `data:`, `vbscript:`…) sont envoyés à DDG pour éviter
/// toute exécution de code injecté depuis la barre d'adresse.
pub fn resolve_input(input: &str) -> String {
    let s = input.trim();
    if s.is_empty() { return String::new(); }

    // Schémas web explicites.
    for prefix in ["https://", "http://", "file://", "nyx://"] {
        if s.starts_with(prefix) { return s.to_string(); }
    }

    // Détecter un schéma non-web : `scheme:non-chiffre` où scheme ne
    // contient pas de point (sinon c'est `host:port`).
    if let Some(colon) = s.find(':') {
        let after = &s[colon + 1..];
        let is_port   = after.starts_with(|c: char| c.is_ascii_digit());
        let is_scheme = after.starts_with("//");
        if !is_port && !is_scheme { return ddg(s); }
    }

    // Domaine bare ou adresse IP.
    if !s.contains(' ') && (s.contains('.') || s.starts_with("localhost")) {
        let scheme = if s.starts_with("localhost") { "http" } else { "https" };
        return format!("{scheme}://{s}");
    }

    ddg(s)
}

fn ddg(q: &str) -> String {
    format!("https://duckduckgo.com/?q={}", urlencode(q))
}

fn urlencode(s: &str) -> String {
    s.replace('&', "%26").replace('#', "%23").replace(' ', "+")
}

#[cfg(test)]
mod tests {
    use super::resolve_input;

    #[test] fn passthrough_https()       { assert_eq!(resolve_input("https://example.com"), "https://example.com"); }
    #[test] fn passthrough_nyx()         { assert_eq!(resolve_input("nyx://newtab"), "nyx://newtab"); }
    #[test] fn bare_domain_gets_https()  { assert_eq!(resolve_input("github.com"), "https://github.com"); }
    #[test] fn localhost_gets_http()     { assert_eq!(resolve_input("localhost:3000"), "http://localhost:3000"); }
    #[test] fn domain_with_port()        { assert_eq!(resolve_input("example.com:8080"), "https://example.com:8080"); }
    #[test] fn query_becomes_ddg()       { assert!(resolve_input("rust async book").contains("duckduckgo.com")); }
    #[test] fn empty_returns_empty()     { assert_eq!(resolve_input("   "), ""); }
    #[test] fn javascript_to_ddg()       { assert!(resolve_input("javascript:alert(1)").contains("duckduckgo.com")); }
    #[test] fn data_scheme_to_ddg()      { assert!(resolve_input("data:text/html,<b>xss</b>").contains("duckduckgo.com")); }
    #[test] fn vbscript_to_ddg()         { assert!(resolve_input("vbscript:msgbox(1)").contains("duckduckgo.com")); }
}
