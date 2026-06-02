//! Pont entre les `PermissionRequest` WebKit et `nyx_core::permissions::Store`.
//!
//! Mécanique seulement — la politique vit dans nyx-core. Ici on :
//! 1. Convertit la `PermissionRequest` WebKit en `Permission` nyx-core.
//! 2. Extrait l'origine de la WebView émettrice.
//! 3. Appelle `Store::decide` → `Allow` / `Deny` / `Ask`.
//! 4. Pour `Ask` : prompt utilisateur (GTK dialog) → `grant`/`deny` + re-décide.

use gtk::prelude::*;
use gtk::{ButtonsType, DialogFlags, MessageDialog, MessageType, ResponseType};
use webkit2gtk::{
    GeolocationPermissionRequest, NotificationPermissionRequest,
    PermissionRequest, PermissionRequestExt, UserMediaPermissionRequest,
    UserMediaPermissionRequestExt, WebView, WebViewExt,
};

use nyx_core::permissions::{Decision, Permission, PermissionStore};

/// Câble la gestion des permissions sur une WebView. Idempotent par WebView.
pub fn wire(webview: &WebView, store: PermissionStore) {
    webview.connect_permission_request(move |wv, req| {
        let Some(perm) = classify(req) else {
            req.deny();
            return true;
        };
        let origin = origin_of(wv);
        match store.borrow().decide(&origin, perm) {
            Decision::Allow => req.allow(),
            Decision::Deny  => req.deny(),
            Decision::Ask   => ask_user(wv, req, &store, origin, perm),
        }
        true
    });
}

/// Classifie la demande WebKit. Renvoie `None` pour les types non gérés
/// (refusés sec par sécurité — opt-in plutôt qu'opt-out).
fn classify(req: &PermissionRequest) -> Option<Permission> {
    if let Some(um) = req.downcast_ref::<UserMediaPermissionRequest>() {
        return Some(if um.is_for_video_device() {
            Permission::Camera
        } else {
            Permission::Microphone
        });
    }
    if req.is::<GeolocationPermissionRequest>() {
        return Some(Permission::Geolocation);
    }
    if req.is::<NotificationPermissionRequest>() {
        return Some(Permission::Notifications);
    }
    None
}

/// Origine canonique de la WebView. Vide si non-http(s) → `decide` retourne `Deny`.
fn origin_of(wv: &WebView) -> String {
    wv.uri().map(|u| canonical_origin(u.as_str())).unwrap_or_default()
}

/// `https://user:pass@host:443/path?x` → `https://host`.
fn canonical_origin(url: &str) -> String {
    let (scheme, default_port) = if url.starts_with("https://") {
        ("https", "443")
    } else if url.starts_with("http://") {
        ("http", "80")
    } else {
        return String::new();
    };
    let after = &url[scheme.len() + 3..];
    let end = after.find('/').unwrap_or(after.len());
    let authority = &after[..end];
    let host_part = authority.rsplit('@').next().unwrap_or(authority);
    let host = match host_part.rsplit_once(':') {
        Some((h, p)) if p == default_port => h,
        _ => host_part,
    };
    if host.is_empty() { String::new() } else { format!("{scheme}://{host}") }
}

/// Demande à l'utilisateur via un GtkDialog modal. Sa réponse est persistée
/// dans le store, puis appliquée à la requête WebKit.
fn ask_user(
    wv: &WebView, req: &PermissionRequest,
    store: &PermissionStore, origin: String, perm: Permission,
) {
    let parent = wv.toplevel().and_then(|t| t.downcast::<gtk::Window>().ok());
    let msg = format!(
        "{} demande l'accès à : {}",
        if origin.is_empty() { "Ce site" } else { &origin },
        label(perm)
    );
    let dialog = MessageDialog::new(
        parent.as_ref(),
        DialogFlags::MODAL,
        MessageType::Question,
        ButtonsType::YesNo,
        &msg,
    );
    let store = store.clone();
    let req = req.clone();
    dialog.connect_response(move |d, resp| {
        let allow = resp == ResponseType::Yes;
        if allow {
            store.borrow_mut().grant(origin.clone(), perm);
            req.allow();
        } else {
            store.borrow_mut().deny(origin.clone(), perm);
            req.deny();
        }
        d.close();
    });
    dialog.show_all();
}

fn label(p: Permission) -> &'static str {
    match p {
        Permission::Camera        => "la caméra",
        Permission::Microphone    => "le microphone",
        Permission::Geolocation   => "votre position",
        Permission::Notifications => "les notifications",
        Permission::ClipboardRead => "le presse-papiers",
        Permission::MidiSysex     => "MIDI SysEx",
    }
}
