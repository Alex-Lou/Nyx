use webkit2gtk::{
    NavigationPolicyDecision, NavigationPolicyDecisionExt, PolicyDecisionExt,
    PolicyDecisionType, URIRequest, URIRequestExt, WebView, WebViewExt,
};

use crate::adblock::AdBlocker;

pub fn configure(webview: &WebView, blocker: std::sync::Arc<AdBlocker>) {
    apply_settings(webview);
    wire_policy_filter(webview, blocker);
}

fn apply_settings(webview: &WebView) {
    use webkit2gtk::SettingsExt;
    let settings = WebViewExt::settings(webview).unwrap();

    // Privacy : désactiver tout ce qui parle à l'extérieur sans raison
    settings.set_enable_java(false);
    settings.set_enable_plugins(false);
    settings.set_enable_private_browsing(false); // géré au niveau vault
    settings.set_enable_hyperlink_auditing(false);

    // Désactiver la géolocalisation
    settings.set_enable_geolocation(false);

    // WebRTC leak prevention
    settings.set_enable_media_stream(false);
}

fn wire_policy_filter(webview: &WebView, blocker: std::sync::Arc<AdBlocker>) {
    use gtk::glib;
    use webkit2gtk::WebViewExt;

    webview.connect_decide_policy(move |_wv, decision, decision_type| {
        if decision_type != PolicyDecisionType::NavigationAction {
            return false;
        }

        if let Ok(nav) = decision.clone().downcast::<NavigationPolicyDecision>() {
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
    format!(
        "https://duckduckgo.com/?q={}",
        urlencoding_minimal(trimmed)
    )
}

fn urlencoding_minimal(s: &str) -> String {
    s.replace(' ', "+")
        .replace('&', "%26")
        .replace('#', "%23")
}
