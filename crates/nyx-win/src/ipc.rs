//! Protocole chrome (JS) ↔ Rust — pur, testé sur l'hôte.
//!
//! Entrant : `window.ipc.postMessage(JSON)` depuis la barre d'outils,
//! désérialisé en [`ChromeMsg`] (enum fermé ; champs inconnus refusés pour
//! les commandes à paramètres, ignorés pour les commandes nues).
//! Sortant : un [`ChromeState`] sérialisé en JSON et injecté via
//! `evaluate_script` — jamais de concaténation de chaînes non échappées.

use serde::{Deserialize, Serialize};

use crate::tabs::TabId;

/// Taille max d'un message chrome : une barre d'adresse n'a aucune raison
/// d'envoyer plus. Au-delà, le message est ignoré.
pub const MAX_MESSAGE_LEN: usize = 16 * 1024;

/// Commande envoyée par le chrome.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "cmd", rename_all = "camelCase", deny_unknown_fields)]
pub enum ChromeMsg {
    /// Le chrome est chargé : renvoyer l'état complet.
    Ready,
    /// Saisie de la barre d'adresse (URL ou recherche).
    Navigate { text: String },
    Back,
    Forward,
    Reload,
    Stop,
    Home,
    NewTab,
    CloseTab { id: TabId },
    SelectTab { id: TabId },
    /// Ouvre le panneau de téléchargements natif de WebView2.
    Downloads,
}

impl ChromeMsg {
    /// Désérialise un message brut. `None` si trop long, mal formé ou inconnu
    /// — jamais de panique sur une entrée JS.
    pub fn parse(raw: &str) -> Option<Self> {
        if raw.len() > MAX_MESSAGE_LEN {
            return None;
        }
        serde_json::from_str(raw).ok()
    }
}

/// Onglet tel qu'affiché dans la barre d'onglets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabView {
    pub id: TabId,
    pub title: String,
    pub active: bool,
    pub loading: bool,
}

/// État complet poussé au chrome après chaque changement.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromeState {
    pub tabs: Vec<TabView>,
    /// URL affichée dans la barre d'adresse (vide pour la page Nouvel onglet).
    pub url: String,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

/// Script de mise à jour du chrome.
pub fn update_script(state: &ChromeState) -> String {
    format!("window.nyx&&window.nyx.update({})", js_literal(state))
}

/// Script d'affichage d'une notification éphémère (« toast »).
pub fn toast_script(text: &str) -> String {
    format!("window.nyx&&window.nyx.toast({})", js_literal(&text))
}

/// Script donnant le focus à la barre d'adresse (Ctrl+L).
pub const FOCUS_URLBAR_SCRIPT: &str = "window.nyx&&window.nyx.focusUrl()";

/// JSON → littéral JS sûr. JSON est (presque) un sous-ensemble de JS : on
/// échappe en plus U+2028/U+2029 et `<` par prudence.
fn js_literal<T: Serialize + ?Sized>(value: &T) -> String {
    serde_json::to_string(value)
        .unwrap_or_else(|_| "null".into())
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
        .replace('<', "\\u003c")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_commands() {
        assert_eq!(ChromeMsg::parse(r#"{"cmd":"ready"}"#), Some(ChromeMsg::Ready));
        assert_eq!(
            ChromeMsg::parse(r#"{"cmd":"navigate","text":"rust"}"#),
            Some(ChromeMsg::Navigate { text: "rust".into() })
        );
        assert_eq!(ChromeMsg::parse(r#"{"cmd":"closeTab","id":3}"#), Some(ChromeMsg::CloseTab { id: 3 }));
        assert_eq!(ChromeMsg::parse(r#"{"cmd":"selectTab","id":0}"#), Some(ChromeMsg::SelectTab { id: 0 }));
        assert_eq!(ChromeMsg::parse(r#"{"cmd":"newTab"}"#), Some(ChromeMsg::NewTab));
        assert_eq!(ChromeMsg::parse(r#"{"cmd":"downloads"}"#), Some(ChromeMsg::Downloads));
    }

    #[test]
    fn rejects_garbage() {
        for raw in [
            "",
            "null",
            "42",
            "{}",
            r#"{"cmd":"rm -rf"}"#,
            r#"{"cmd":"navigate"}"#,
            r#"{"cmd":"navigate","text":1}"#,
            r#"{"cmd":"closeTab","id":-1}"#,
            r#"{"cmd":"closeTab","id":"1"}"#,
            r#"{"cmd":"closeTab","id":99999999999}"#,
            r#"{"cmd":"navigate","text":"x","extra":true}"#,
            r#"{"CMD":"back"}"#,
            "{\"cmd\":\"navigate\",\"text\":\"\u{0}\"",
        ] {
            assert_eq!(ChromeMsg::parse(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn rejects_oversized() {
        let big = format!(r#"{{"cmd":"navigate","text":"{}"}}"#, "a".repeat(MAX_MESSAGE_LEN));
        assert_eq!(ChromeMsg::parse(&big), None);
    }

    #[test]
    fn scripts_are_escaped() {
        let state = ChromeState {
            tabs: vec![TabView {
                id: 1,
                title: "</script><img onerror=x>\u{2028}\"'".into(),
                active: true,
                loading: false,
            }],
            url: "https://x/\u{2029}".into(),
            ..ChromeState::default()
        };
        let js = update_script(&state);
        assert!(js.starts_with("window.nyx&&window.nyx.update({"));
        assert!(!js.contains('<'));
        assert!(!js.contains('\u{2028}') && !js.contains('\u{2029}'));
        assert!(js.contains("canGoBack"));

        let toast = toast_script("a\"b<c");
        assert_eq!(toast, r#"window.nyx&&window.nyx.toast("a\"b\u003cc")"#);
    }
}
