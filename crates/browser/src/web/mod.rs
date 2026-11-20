//! Couche moteur web : configuration WebKit, filtrage de navigation
//! (NyxGuard + routage des pages internes `nyx://`). La logique pure
//! (sécurité, URL, NyxGuard) vit dans `nyx-core`.

pub mod adfilter;
pub mod cookies;
pub mod darkmode;
pub mod downloads;
pub mod inspector;
pub mod password_capture;
pub mod permissions;
pub mod site_data;

pub use nyx_core::nyxguard;
pub use nyx_core::security;

use std::sync::Arc;

use gtk::prelude::*;
use webkit2gtk::{
    NavigationPolicyDecision, NavigationPolicyDecisionExt, PolicyDecision,
    PolicyDecisionExt, PolicyDecisionType, ResponsePolicyDecision,
    ResponsePolicyDecisionExt, URIRequestExt, URIResponseExt, WebView, WebViewExt,
};

use std::rc::Rc;

use vault::Vault;
use webkit2gtk::LoadEvent;

use crate::pages::{self, bookmarks as bookmarks_page, history as history_page, newtab, settings as settings_page};
use crate::state::bookmarks::{self, Bookmarks};
use crate::state::settings::{self, Settings};
use nyxguard::NyxGuard;
use security::{Page, Verdict};

pub use nyx_core::url::resolve_input;

pub fn configure(
    webview: &WebView, blocker: Arc<NyxGuard>, prefs: Settings, bm: Bookmarks,
    perms: nyx_core::permissions::PermissionStore, vault: Rc<Vault>,
) {
    apply_privacy_settings(webview);
    wire_history_autosave(webview, vault.clone(), prefs.clone());
    password_capture::wire(webview, vault.clone(), prefs.clone());
    if prefs.borrow().reject_cookies {
        cookies::wire(webview); // refus auto des bannières de consentement
    }
    wire_policy_filter(webview, blocker, prefs, bm, vault);
    permissions::wire(webview, perms);
}

/// Historique automatique : chaque chargement http(s) terminé est poussé
/// dans le vault, sans doublon consécutif (reload, redirect).
fn wire_history_autosave(webview: &WebView, vault: Rc<Vault>, prefs: Settings) {
    // Dernière URL enregistrée, gardée en mémoire (pas de relecture en base).
    let last = std::cell::RefCell::new(String::new());
    webview.connect_load_changed(move |wv, event| {
        if event != LoadEvent::Finished || prefs.borrow().private_mode {
            return; // mode privé : aucune trace dans l'historique
        }
        let Some(uri) = wv.uri() else { return };
        if !matches!(nyx_core::url::scheme_of(&uri).as_deref(), Some("http" | "https")) {
            return; // pages internes (nyx://, file://assets) hors historique
        }
        if *last.borrow() == uri.as_str() {
            return;
        }
        let title = wv.title().unwrap_or_default();
        if vault.push_history(&uri, &title).is_ok() {
            *last.borrow_mut() = uri.to_string();
        }
    });
}

fn apply_privacy_settings(webview: &WebView) {
    use webkit2gtk::SettingsExt;
    let s = WebViewExt::settings(webview).expect("WebView sans Settings");
    s.set_enable_media_stream(false);
    s.set_javascript_can_open_windows_automatically(false);
    s.set_javascript_can_access_clipboard(false);
    // Inspector activé : F12 / Ctrl+Shift+I ouvrent l'inspecteur WebKit
    // (équivalent DevTools). Aussi exposé via le menu contextuel
    // « Inspecter » de WebKit. Pas de surface d'attaque ajoutée — c'est
    // l'inspecteur sandboxé du moteur, pas un eval JS distant.
    s.set_enable_developer_extras(true);
    s.set_enable_smooth_scrolling(true);
}

/// `Content-Disposition: attachment` : le serveur demande un téléchargement
/// même pour un type affichable (PDF, image, texte…).
fn is_attachment(r: &ResponsePolicyDecision) -> bool {
    r.response()
        .and_then(|resp| resp.http_headers())
        .and_then(|h| h.one("Content-Disposition"))
        .is_some_and(|v| v.trim_start().to_ascii_lowercase().starts_with("attachment"))
}

fn wire_policy_filter(
    webview: &WebView, blocker: Arc<NyxGuard>, prefs: Settings, bm: Bookmarks, vault: Rc<Vault>,
) {
    webview.connect_decide_policy(move |wv, decision, dtype| {
        let url = match dtype {
            PolicyDecisionType::NavigationAction => extract_url(decision),
            // window.open / target=_blank : la vue créée n'a pas encore d'URI,
            // on juge donc la cible avec la confiance de l'OUVREUR. Seuls les
            // refus s'appliquent ici ; la navigation réelle est re-jugée dans
            // la nouvelle vue.
            PolicyDecisionType::NewWindowAction => {
                let url = extract_url(decision);
                if security::decide(&url, page_is_internal(wv), &blocker).is_block() {
                    decision.ignore();
                    return true;
                }
                return false;
            }
            // Réponse que WebKit ne sait pas afficher (zip, exe, pdf forcé…) :
            // sans décision explicite, la navigation reste figée. On la
            // bascule en téléchargement → pont downloads (politique + shelf).
            PolicyDecisionType::Response => {
                let Some(r) = decision.downcast_ref::<ResponsePolicyDecision>() else { return false };
                if r.is_mime_type_supported() && !is_attachment(r) {
                    return false;
                }
                r.download();
                return true;
            }
            _ => return false,
        };

        match security::decide(&url, page_is_internal(wv), &blocker) {
            Verdict::Allow => {
                // Second rideau : le moteur Brave (EasyList complet), trop
                // lourd pour la couche pure — consulté ici, main-thread.
                if prefs.borrow().adblock_enabled && adfilter::should_block(&url) {
                    decision.ignore();
                    return true;
                }
                false
            }
            // Import de la capsule WSL : propre à Windows natif.
            Verdict::Block | Verdict::BlockFileAccess | Verdict::ImportCapsule => {
                decision.ignore();
                true
            }
            Verdict::ApplySettings => {
                decision.ignore();
                settings::apply_from_url(&url, &prefs, &blocker);
                // Réglages persistés dans le vault — survivent au redémarrage.
                let _ = vault.set_setting("app_settings", &settings::to_query(&prefs.borrow()));
                true
            }
            Verdict::MoveBookmark => {
                decision.ignore();
                apply_move(&url, &bm);
                crate::state::vault_sync::persist_bookmarks(&bm, &vault);
                true
            }
            Verdict::ClearHistory => {
                decision.ignore();
                let _ = vault.clear_history();
                load_internal_page(wv, Page::History, &prefs, &bm, &vault);
                true
            }
            Verdict::ImportChrome => {
                decision.ignore();
                let parent = wv.toplevel().and_then(|w| w.downcast::<gtk::Window>().ok());
                let (bm, vault) = (bm.clone(), vault.clone());
                // Hors du callback de politique : le dialog ne réentre pas WebKit.
                gtk::glib::idle_add_local_once(move || {
                    crate::ui::import_chrome::run(parent.as_ref(), &bm, &vault);
                });
                true
            }
            Verdict::Load(page) => {
                decision.ignore();
                load_internal_page(wv, page, &prefs, &bm, &vault);
                true
            }
        }
    });
}

fn load_internal_page(wv: &WebView, page: Page, prefs: &Settings, bm: &Bookmarks, vault: &Rc<Vault>) {
    let (html, base) = match page {
        Page::Settings  => (settings_page::html(&prefs.borrow()), Some(pages::assets_base_uri())),
        Page::Bookmarks => (bookmarks_page::page_html(&bm.borrow()), Some(pages::assets_base_uri())),
        Page::History => {
            let entries = vault.recent_history(200).unwrap_or_default();
            (history_page::html(&entries), Some(pages::assets_base_uri()))
        }
        Page::NewTab    => (newtab::html(), Some(pages::assets_base_uri())),
    };
    let wv = wv.clone();
    gtk::glib::idle_add_local_once(move || {
        wv.load_html(&html, base);
    });
}

/// Page interne = `nyx://…` ou notre base `file://…/assets/` EXACTE.
/// Sans URI (vue tout juste créée, p. ex. par `window.open`) → NON fiable :
/// sinon une page distante pourrait ouvrir `nyx://apply?…` dans une vue vierge.
fn page_is_internal(wv: &WebView) -> bool {
    wv.uri().is_some_and(|u| {
        let u = u.as_str();
        match nyx_core::url::scheme_of(u).as_deref() {
            Some("nyx") => true,
            Some("file") => u.starts_with(pages::assets_base_uri()),
            _ => false,
        }
    })
}

/// Parse `nyx://move?idx=N&to=Folder` et applique le déplacement.
/// Folder est urldecoded (+ → espace, %xx → octet).
fn apply_move(url: &str, bm: &Bookmarks) {
    if let Some((i, to)) = bookmarks::parse_move(url) {
        bookmarks::move_to(bm, i, to);
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
