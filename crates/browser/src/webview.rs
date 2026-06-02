use std::sync::Arc;

use gtk::prelude::*;
use webkit2gtk::{
    NavigationPolicyDecision, NavigationPolicyDecisionExt, PolicyDecisionExt,
    PolicyDecisionType, URIRequestExt, WebView, WebViewExt,
};

use crate::adblock::AdBlocker;
use crate::bookmarks::Bookmarks;
use crate::settings::Settings;
use crate::{newtab, settings, settings_page};

pub fn configure(
    webview:  &WebView,
    blocker:  Arc<AdBlocker>,
    prefs:    Settings,
    bm:       Bookmarks,
) {
    apply_privacy_settings(webview);
    wire_policy_filter(webview, blocker, prefs, bm);
}

fn apply_privacy_settings(webview: &WebView) {
    use webkit2gtk::SettingsExt;
    let s = WebViewExt::settings(webview).expect("WebView sans Settings");
    s.set_enable_media_stream(false);
    s.set_javascript_can_open_windows_automatically(false);
    s.set_enable_developer_extras(false);
    s.set_enable_smooth_scrolling(true);
}

fn wire_policy_filter(
    webview: &WebView,
    blocker: Arc<AdBlocker>,
    prefs:   Settings,
    bm:      Bookmarks,
) {
    webview.connect_decide_policy(move |wv, decision, dtype| {
        if dtype != PolicyDecisionType::NavigationAction { return false; }

        let url = extract_url(decision);

        // — Schéma interne nyx:// ────────────────────────────────────────
        if let Some(rest) = url.strip_prefix("nyx://") {
            decision.ignore();
            if rest.starts_with("apply") {
                settings::apply_from_url(&url, &prefs, &blocker);
                let html = settings_page::html(&prefs.borrow());
                wv.load_html(&html, Some(&settings_page::base_uri()));
            } else if rest.starts_with("newtab") {
                wv.load_html(newtab::html(), Some(&newtab::base_uri()));
            } else if rest.starts_with("settings") {
                let html = settings_page::html(&prefs.borrow());
                wv.load_html(&html, Some(&settings_page::base_uri()));
            } else if rest.starts_with("bookmarks") {
                let html = crate::bookmarks::page_html(&bm.borrow());
                wv.load_html(&html, None);
            }
            return true;
        }

        // — Adblock ──────────────────────────────────────────────────────
        if let Ok(nav) = decision.clone().downcast::<NavigationPolicyDecision>() {
            if blocker.should_block(&url) {
                nav.ignore();
                return true;
            }
        }
        false
    });
}

fn extract_url(decision: &webkit2gtk::PolicyDecision) -> String {
    decision.clone()
        .downcast::<NavigationPolicyDecision>()
        .ok()
        .and_then(|n| n.navigation_action())
        .and_then(|a| a.request())
        .and_then(|r| r.uri())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// Normalise une entrée barre d'adresse en URL.
/// Utilise le moteur de recherche configuré dans `prefs` pour les requêtes.
pub fn resolve_input(input: &str, prefs: &crate::settings::AppSettings) -> String {
    let s = input.trim();
    if s.is_empty() { return String::new(); }

    for prefix in ["https://", "http://", "file://", "nyx://"] {
        if s.starts_with(prefix) { return s.to_string(); }
    }

    if let Some(colon) = s.find(':') {
        let after = &s[colon + 1..];
        if !after.starts_with(|c: char| c.is_ascii_digit()) && !after.starts_with("//") {
            return prefs.search_engine.search_url(s);
        }
    }

    if !s.contains(' ') && (s.contains('.') || s.starts_with("localhost")) {
        let scheme = if s.starts_with("localhost") { "http" } else { "https" };
        return format!("{scheme}://{s}");
    }

    prefs.search_engine.search_url(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::AppSettings;
    fn p() -> AppSettings { AppSettings::default() }

    #[test] fn passthrough_https()     { assert_eq!(resolve_input("https://x.com", &p()), "https://x.com"); }
    #[test] fn passthrough_nyx()       { assert_eq!(resolve_input("nyx://newtab", &p()), "nyx://newtab"); }
    #[test] fn bare_domain()           { assert_eq!(resolve_input("github.com", &p()), "https://github.com"); }
    #[test] fn localhost()             { assert_eq!(resolve_input("localhost:3000", &p()), "http://localhost:3000"); }
    #[test] fn query_becomes_ddg()     { assert!(resolve_input("rust async", &p()).contains("duckduckgo.com")); }
    #[test] fn empty()                 { assert_eq!(resolve_input("  ", &p()), ""); }
    #[test] fn javascript_to_search()  { assert!(resolve_input("javascript:x", &p()).contains("duckduckgo.com")); }
    #[test] fn data_to_search()        { assert!(resolve_input("data:text/html,x", &p()).contains("duckduckgo.com")); }
}
