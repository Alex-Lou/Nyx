//! Couche moteur web : configuration WebKit, filtrage de navigation
//! (adblock + routage des pages internes `nyx://`), résolution de la barre
//! d'adresse. Le rendu HTML appartient à `crate::pages`.

pub mod adblock;
pub mod darkmode;

use std::sync::Arc;

use gtk::prelude::*;
use webkit2gtk::{
    NavigationPolicyDecision, NavigationPolicyDecisionExt, PolicyDecision,
    PolicyDecisionExt, PolicyDecisionType, URIRequestExt, WebView, WebViewExt,
};

use crate::pages::{self, bookmarks as bookmarks_page, newtab, settings as settings_page};
use crate::state::bookmarks::Bookmarks;
use crate::state::settings::{self, AppSettings, Settings};
use adblock::AdBlocker;

pub fn configure(webview: &WebView, blocker: Arc<AdBlocker>, prefs: Settings, bm: Bookmarks) {
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

fn wire_policy_filter(webview: &WebView, blocker: Arc<AdBlocker>, prefs: Settings, bm: Bookmarks) {
    webview.connect_decide_policy(move |wv, decision, dtype| {
        if dtype != PolicyDecisionType::NavigationAction {
            return false;
        }
        let url = extract_url(decision);

        if url.starts_with("nyx://") {
            decision.ignore();
            route_internal(wv, &url, &prefs, &blocker, &bm);
            return true;
        }

        if let Ok(nav) = decision.clone().downcast::<NavigationPolicyDecision>() {
            if blocker.should_block(&url) {
                nav.ignore();
                return true;
            }
        }
        false
    });
}

/// Route un schéma `nyx://` vers la page interne correspondante.
///
/// Le `load_html` est **différé** via `idle_add_local_once` : charger de façon
/// ré-entrante depuis `decide-policy` laisse parfois la WebView blanche.
fn route_internal(wv: &WebView, url: &str, prefs: &Settings, blocker: &Arc<AdBlocker>, bm: &Bookmarks) {
    let rest = url.trim_start_matches("nyx://");

    let (html, base) = if rest.starts_with("apply") {
        // SÉCURITÉ : la mutation des réglages n'est honorée que si la page
        // émettrice est interne. Une page distante ne peut pas désactiver
        // l'adblock via location='nyx://apply?adblock=false'.
        if !page_is_internal(wv) {
            return;
        }
        settings::apply_from_url(url, prefs, blocker);
        (settings_page::html(&prefs.borrow()), Some(pages::assets_base_uri()))
    } else if rest.starts_with("settings") {
        (settings_page::html(&prefs.borrow()), Some(pages::assets_base_uri()))
    } else if rest.starts_with("bookmarks") {
        (bookmarks_page::page_html(&bm.borrow()), None)
    } else {
        (newtab::html().to_string(), Some(pages::assets_base_uri()))
    };

    let wv = wv.clone();
    gtk::glib::idle_add_local_once(move || {
        wv.load_html(&html, base.as_deref());
    });
}

/// Frontière de confiance : une page est interne si elle vient de nos assets
/// (`file://…/assets/`) ou d'un schéma `nyx://`.
fn page_is_internal(wv: &WebView) -> bool {
    match wv.uri() {
        Some(u) => {
            let u = u.as_str();
            u.starts_with("nyx://") || (u.starts_with("file://") && u.contains("/assets/"))
        }
        None => true, // WebView fraîche ouverte par nous, avant tout load
    }
}

fn extract_url(decision: &PolicyDecision) -> String {
    decision
        .clone()
        .downcast::<NavigationPolicyDecision>()
        .ok()
        .and_then(|n| n.navigation_action())
        .and_then(|a| a.request())
        .and_then(|r| r.uri())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// Normalise une entrée barre d'adresse en URL, via le moteur configuré.
pub fn resolve_input(input: &str, prefs: &AppSettings) -> String {
    let s = input.trim();
    if s.is_empty() {
        return String::new();
    }
    for prefix in ["https://", "http://", "file://", "nyx://"] {
        if s.starts_with(prefix) {
            return s.to_string();
        }
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
    use super::resolve_input;
    use crate::state::settings::AppSettings;
    fn p() -> AppSettings { AppSettings::default() }

    #[test] fn passthrough_https()   { assert_eq!(resolve_input("https://x.com", &p()), "https://x.com"); }
    #[test] fn passthrough_nyx()     { assert_eq!(resolve_input("nyx://newtab", &p()), "nyx://newtab"); }
    #[test] fn bare_domain()         { assert_eq!(resolve_input("github.com", &p()), "https://github.com"); }
    #[test] fn localhost()           { assert_eq!(resolve_input("localhost:3000", &p()), "http://localhost:3000"); }
    #[test] fn query_to_engine()     { assert!(resolve_input("rust async", &p()).contains("duckduckgo.com")); }
    #[test] fn empty()               { assert_eq!(resolve_input("  ", &p()), ""); }
    #[test] fn javascript_neutered() { assert!(resolve_input("javascript:x", &p()).contains("duckduckgo.com")); }
    #[test] fn data_neutered()       { assert!(resolve_input("data:text/html,x", &p()).contains("duckduckgo.com")); }
}
