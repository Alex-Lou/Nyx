use gtk::prelude::*;
use webkit2gtk::{
    NavigationPolicyDecision, NavigationPolicyDecisionExt, PolicyDecisionExt,
    PolicyDecisionType, URIRequestExt, WebView, WebViewExt,
};

use crate::adblock::AdBlocker;

pub fn configure(webview: &WebView, blocker: std::sync::Arc<AdBlocker>) {
    apply_settings(webview);
    wire_policy_filter(webview, blocker);
}

fn apply_settings(webview: &WebView) {
    use webkit2gtk::SettingsExt;
    let settings = WebViewExt::settings(webview).unwrap();

    // Privacy : couper tout ce qui parle à l'extérieur sans raison.
    settings.set_enable_java(false);
    settings.set_enable_plugins(false);
    settings.set_enable_private_browsing(false); // géré au niveau vault
    settings.set_enable_hyperlink_auditing(false);

    // WebRTC leak prevention.
    settings.set_enable_media_stream(false);

    // Note : WebKitGTK 4.1 a retiré `enable-geolocation` au profit d'une
    //        permission demandée à l'utilisateur. Sera gérée Sprint 5
    //        via `connect_permission_request`.
}

fn wire_policy_filter(webview: &WebView, blocker: std::sync::Arc<AdBlocker>) {
    webview.connect_decide_policy(move |_wv, decision, decision_type| {
        if decision_type != PolicyDecisionType::NavigationAction {
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
///   `""`              → `""` (l'appelant doit no-op)
///   `"google.com"`    → `"https://google.com"`
///   `"rust lang"`     → recherche DuckDuckGo
pub fn resolve_input(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("file://")
        || trimmed.starts_with("nyx://")
    {
        return trimmed.to_string();
    }
    if trimmed.contains('.') && !trimmed.contains(' ') {
        return format!("https://{}", trimmed);
    }
    format!("https://duckduckgo.com/?q={}", urlencode(trimmed))
}

fn urlencode(s: &str) -> String {
    s.replace('&', "%26").replace('#', "%23").replace(' ', "+")
}
