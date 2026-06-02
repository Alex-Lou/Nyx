//! Couche moteur web : configuration WebKit, filtrage de navigation
//! (NyxGuard + routage des pages internes `nyx://`). La logique pure
//! (sécurité, URL, NyxGuard) vit dans `nyx-core`.

pub mod darkmode;

pub use nyx_core::nyxguard;
pub use nyx_core::security;

use std::sync::Arc;

use gtk::prelude::*;
use webkit2gtk::{
    NavigationPolicyDecision, NavigationPolicyDecisionExt, PolicyDecision,
    PolicyDecisionExt, PolicyDecisionType, URIRequestExt, WebView, WebViewExt,
};

use crate::pages::{self, bookmarks as bookmarks_page, newtab, settings as settings_page};
use crate::state::bookmarks::Bookmarks;
use crate::state::settings::{self, Settings};
use nyxguard::NyxGuard;
use security::{Page, Verdict};

pub use nyx_core::url::resolve_input;

pub fn configure(webview: &WebView, blocker: Arc<NyxGuard>, prefs: Settings, bm: Bookmarks) {
    apply_privacy_settings(webview);
    wire_policy_filter(webview, blocker, prefs, bm);
}

fn apply_privacy_settings(webview: &WebView) {
    use webkit2gtk::SettingsExt;
    let s = WebViewExt::settings(webview).expect("WebView sans Settings");
    s.set_enable_media_stream(false);
    s.set_javascript_can_open_windows_automatically(false);
    s.set_javascript_can_access_clipboard(false);
    s.set_enable_developer_extras(false);
    s.set_enable_smooth_scrolling(true);
}

fn wire_policy_filter(webview: &WebView, blocker: Arc<NyxGuard>, prefs: Settings, bm: Bookmarks) {
    webview.connect_decide_policy(move |wv, decision, dtype| {
        if dtype != PolicyDecisionType::NavigationAction {
            return false;
        }
        let url = extract_url(decision);

        match security::decide(&url, page_is_internal(wv), &blocker) {
            Verdict::Allow => false,
            Verdict::Block => {
                decision.ignore();
                true
            }
            Verdict::ApplySettings => {
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

fn page_is_internal(wv: &WebView) -> bool {
    match wv.uri() {
        Some(u) => {
            let u = u.as_str();
            u.starts_with("nyx://") || (u.starts_with("file://") && u.contains("/assets/"))
        }
        None => true,
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
