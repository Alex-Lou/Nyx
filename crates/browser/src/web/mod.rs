//! Couche moteur web : configuration WebKit, filtrage de navigation
//! (NyxGuard + routage des pages internes `nyx://`). La logique pure
//! (sécurité, URL, NyxGuard) vit dans `nyx-core`.

pub mod darkmode;
pub mod permissions;
pub mod site_data;

pub use nyx_core::nyxguard;
pub use nyx_core::security;

use std::sync::Arc;

use gtk::prelude::*;
use webkit2gtk::{
    NavigationPolicyDecision, NavigationPolicyDecisionExt, PolicyDecision,
    PolicyDecisionExt, PolicyDecisionType, URIRequestExt, WebView, WebViewExt,
};

use crate::pages::{self, bookmarks as bookmarks_page, newtab, settings as settings_page};
use crate::state::bookmarks::{self, Bookmarks};
use crate::state::settings::{self, Settings};
use nyxguard::NyxGuard;
use security::{Page, Verdict};

pub use nyx_core::url::resolve_input;

pub fn configure(
    webview: &WebView, blocker: Arc<NyxGuard>, prefs: Settings, bm: Bookmarks,
    perms: nyx_core::permissions::PermissionStore,
) {
    apply_privacy_settings(webview);
    wire_policy_filter(webview, blocker, prefs, bm);
    permissions::wire(webview, perms);
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
            Verdict::Block | Verdict::BlockFileAccess => {
                decision.ignore();
                true
            }
            Verdict::ApplySettings => {
                decision.ignore();
                settings::apply_from_url(&url, &prefs, &blocker);
                true
            }
            Verdict::MoveBookmark => {
                decision.ignore();
                apply_move(&url, &bm);
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

/// Parse `nyx://move?idx=N&to=Folder` et applique le déplacement.
/// Folder est urldecoded (+ → espace, %xx → octet).
fn apply_move(url: &str, bm: &Bookmarks) {
    let Some(query) = url.split_once('?').map(|x| x.1) else { return };
    let (mut idx, mut to) = (None::<usize>, String::new());
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        match k {
            "idx" => idx = v.parse().ok(),
            "to"  => to = urldecode(v),
            _ => {}
        }
    }
    if let Some(i) = idx {
        bookmarks::move_to(bm, i, to);
    }
}

fn urldecode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => { out.push(' '); i += 1; }
            b'%' if i + 2 < b.len() => {
                let h = std::str::from_utf8(&b[i + 1..i + 3]).ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok());
                if let Some(byte) = h {
                    out.push(byte as char);
                    i += 3;
                } else {
                    out.push(b[i] as char);
                    i += 1;
                }
            }
            c => { out.push(c as char); i += 1; }
        }
    }
    out
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
