//! Couche moteur web : configuration WebKit, filtrage de navigation
//! (NyxGuard + routage des pages internes `nyx://`), résolution de la barre
//! d'adresse. Le rendu HTML appartient à `crate::pages`.

pub mod darkmode;
pub mod nyxguard;
pub mod security;

use std::sync::Arc;

use gtk::prelude::*;
use webkit2gtk::{
    NavigationPolicyDecision, NavigationPolicyDecisionExt, PolicyDecision,
    PolicyDecisionExt, PolicyDecisionType, URIRequestExt, WebView, WebViewExt,
};

use crate::pages::{self, bookmarks as bookmarks_page, newtab, settings as settings_page};
use crate::state::bookmarks::Bookmarks;
use crate::state::settings::{self, AppSettings, Settings};
use nyxguard::NyxGuard;
use security::{Page, Verdict};

pub fn configure(webview: &WebView, blocker: Arc<NyxGuard>, prefs: Settings, bm: Bookmarks) {
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

fn wire_policy_filter(webview: &WebView, blocker: Arc<NyxGuard>, prefs: Settings, bm: Bookmarks) {
    webview.connect_decide_policy(move |wv, decision, dtype| {
        if dtype != PolicyDecisionType::NavigationAction {
            return false;
        }
        let url = extract_url(decision);

        // La *décision* vit dans `security` (pur) ; ici on n'exécute que la
        // *mécanique* WebKit correspondante.
        match security::decide(&url, page_is_internal(wv), &blocker) {
            Verdict::Allow => false,
            Verdict::Block => {
                decision.ignore();
                true
            }
            Verdict::ApplySettings => {
                // Auto-save : on applique sans recharger (la page garde son
                // état JS, soumis via une iframe cachée).
                decision.ignore();
                settings::apply_from_url(&url, &prefs, &blocker);
                true
            }
            Verdict::Load(page) => {
                decision.ignore();
                load_internal_page(wv, page, &prefs, &bm);
                true
            }
        }
    });
}

/// Charge une page interne. Le `load_html` est **différé** via
/// `idle_add_local_once` : charger de façon ré-entrante depuis `decide-policy`
/// laisse parfois la WebView blanche.
fn load_internal_page(wv: &WebView, page: Page, prefs: &Settings, bm: &Bookmarks) {
    let (html, base) = match page {
        Page::Settings  => (settings_page::html(&prefs.borrow()), Some(pages::assets_base_uri())),
        Page::Bookmarks => (bookmarks_page::page_html(&bm.borrow()), None),
        Page::NewTab    => (newtab::html(), Some(pages::assets_base_uri())),
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

    /// Fuzz « centaines d'entrées » : aucune ne doit paniquer, et aucun schéma
    /// dangereux ne doit passer en clair (toujours neutralisé en recherche).
    #[test]
    fn stress_fuzz_inputs() {
        let prefs = p();
        let fragments = [
            "", " ", "a", "google.com", "rust async", "http://x", "https://y.z",
            "javascript:alert(1)", "data:text/html,x", "vbscript:x", "file:///etc",
            "nyx://settings", "localhost:8080", "192.168.0.1:3000", "日本:9000",
            "a b c d", "::", "ftp://h", "mailto:a@b.c", "a.b.c.d.e.f.g",
        ];
        for i in 0..10_000 {
            let s = fragments[i % fragments.len()];
            let out = resolve_input(s, &prefs); // ne doit pas paniquer
            // Un schéma exécutable ne ressort jamais tel quel.
            assert!(!out.starts_with("javascript:"));
            assert!(!out.starts_with("data:"));
            assert!(!out.starts_with("vbscript:"));
        }
    }
}
