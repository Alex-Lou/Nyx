//! Messages des pages de contenu → Rust — pur, testé sur l'hôte.
//!
//! Deux émetteurs possibles sur `window.chrome.webview` d'un onglet :
//! • le script de capture (`assets/password_capture.js`), cadre principal ;
//! • les pages internes (`https://nyx.localhost/…`) pour leurs actions
//!   (`nyx://move`, `nyx://apply`…).
//!
//! N'importe quelle page peut poster n'importe quoi : chaque message est
//! revalidé ici contre l'URL de l'émetteur (fournie par WebView2, pas par la
//! page) ET l'URL courante de l'onglet.

use serde::Deserialize;

use nyx_core::nyxguard::NyxGuard;
use nyx_core::url::{host_of, scheme_of};

use crate::nav::{self, Action};
use crate::secret::Secret;

/// Taille max d'un message de contenu (identifiant + mot de passe, URL d'action).
pub const MAX_MESSAGE_LEN: usize = 8 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "nyx", rename_all = "camelCase")]
pub enum ContentMsg {
    /// Identifiants soumis (script de capture).
    Password {
        #[serde(default)]
        username: String,
        password: Secret,
        #[serde(default)]
        host: String,
    },
    /// Action interne demandée par une page interne.
    Action { url: String },
}

impl ContentMsg {
    pub fn parse(raw: &str) -> Option<Self> {
        if raw.len() > MAX_MESSAGE_LEN {
            return None;
        }
        serde_json::from_str(raw).ok()
    }
}

/// Hôte sous lequel enregistrer un identifiant capturé, ou `None` si le
/// message doit être ignoré. Même contrôle que le desktop (http(s), hôte
/// annoncé = hôte de la page), plus : l'émetteur doit être la page
/// principale actuellement engagée dans l'onglet (même hôte).
pub fn password_host(source_url: &str, tab_url: &str, claimed_host: &str) -> Option<String> {
    if !matches!(scheme_of(source_url).as_deref(), Some("http" | "https")) || nav::is_internal(source_url) {
        return None;
    }
    let source = host_of(source_url)?;
    let tab = host_of(tab_url)?;
    if source.is_empty() || !source.eq_ignore_ascii_case(&tab) || !claimed_host.eq_ignore_ascii_case(&source) {
        return None;
    }
    Some(source.to_ascii_lowercase())
}

/// Action interne demandée par la page `source_url` : décidée par
/// `nyx_core::security::decide` avec `page_internal` = émetteur interne.
/// Une page web ne peut donc jamais déclencher `move`, `apply`, etc.
pub fn action(source_url: &str, action_url: &str, guard: &NyxGuard) -> Option<Action> {
    if !nav::is_internal(source_url) {
        return None;
    }
    nav::internal_action(action_url, source_url, guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_capture_message() {
        let raw = r#"{"nyx":"password","username":"alex","password":"hunter2","host":"example.com"}"#;
        let msg = ContentMsg::parse(raw).expect("message");
        assert!(matches!(&msg, ContentMsg::Password { username, password, host }
            if username == "alex" && password.expose() == "hunter2" && host == "example.com"));
        assert!(!format!("{msg:?}").contains("hunter2"));
        assert_eq!(
            ContentMsg::parse(r#"{"nyx":"action","url":"nyx://move?idx=0&to=x"}"#),
            Some(ContentMsg::Action { url: "nyx://move?idx=0&to=x".into() })
        );
    }

    #[test]
    fn rejects_garbage() {
        for raw in ["", "{}", "null", r#"{"nyx":"password"}"#, r#"{"nyx":"eval","js":"x"}"#, r#"{"cmd":"newTab"}"#] {
            assert_eq!(ContentMsg::parse(raw), None, "{raw}");
        }
        let big = format!(r#"{{"nyx":"action","url":"{}"}}"#, "a".repeat(MAX_MESSAGE_LEN));
        assert_eq!(ContentMsg::parse(&big), None);
    }

    #[test]
    fn password_origin_checks() {
        let page = "https://login.example.com/signin";
        assert_eq!(password_host(page, page, "login.example.com").as_deref(), Some("login.example.com"));
        assert_eq!(password_host(page, page, "LOGIN.example.com").as_deref(), Some("login.example.com"));
        // Hôte annoncé ≠ page (message forgé, ou autre onglet).
        assert_eq!(password_host(page, page, "bank.com"), None);
        // L'onglet a déjà navigué ailleurs.
        assert_eq!(password_host(page, "https://other.org/", "login.example.com"), None);
        // Pages internes, file:, data: → jamais.
        assert_eq!(password_host("https://nyx.localhost/settings", "https://nyx.localhost/settings", "nyx.localhost"), None);
        assert_eq!(password_host("file:///C:/x.html", "file:///C:/x.html", ""), None);
        assert_eq!(password_host("data:text/html,x", "data:text/html,x", ""), None);
        assert_eq!(password_host("", "", ""), None);
    }

    #[test]
    fn actions_only_from_internal_pages() {
        let g = NyxGuard::new();
        let internal = "https://nyx.localhost/bookmarks";
        assert!(matches!(action(internal, "nyx://move?idx=0&to=A", &g), Some(Action::MoveBookmark(_))));
        assert_eq!(action(internal, "nyx://history/clear", &g), Some(Action::ClearHistory));
        assert!(matches!(action(internal, "nyx://apply?engine=brave", &g), Some(Action::Apply(_))));
        assert_eq!(action(internal, "https://example.com/", &g), None);
        assert_eq!(action(internal, "nyx://settings", &g), None, "page, pas action");
        let settings = "https://nyx.localhost/settings";
        assert_eq!(
            action(settings, "nyx://master/remove", &g),
            Some(Action::MasterPassword(nyx_core::security::MasterOp::Remove))
        );
        for remote in ["https://evil.example/", "https://nyx.localhost.evil.com/", "", "about:blank"] {
            for a in [
                "nyx://move?idx=0&to=A",
                "nyx://history/clear",
                "nyx://apply?adblock=false",
                "nyx://import/capsule",
                "nyx://master/set",
                "nyx://master/remove",
            ] {
                assert_eq!(action(remote, a, &g), None, "{remote} {a}");
            }
        }
    }
}
