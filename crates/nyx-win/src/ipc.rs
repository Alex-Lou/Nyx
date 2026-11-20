//! Protocole chrome (JS) ↔ Rust — pur, testé sur l'hôte.
//!
//! Entrant : `window.ipc.postMessage(JSON)` depuis la barre d'outils,
//! désérialisé en [`ChromeMsg`] (enum fermé ; champs inconnus refusés pour
//! les commandes à paramètres, ignorés pour les commandes nues).
//! Sortant : un [`ChromeState`] sérialisé en JSON et injecté via
//! `evaluate_script` — jamais de concaténation de chaînes non échappées.
//!
//! Les passphrases n'arrivent QUE par ce canal (origine `nyxui` vérifiée par
//! `app::build_chrome`), dans des [`Secret`] jamais affichés ni journalisés.

use serde::{Deserialize, Serialize};

use crate::secret::Secret;
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
    /// Étoile : ajoute / retire la page courante des favoris.
    ToggleBookmark,
    /// Réponse à la question affichée (enregistrer un mot de passe…).
    #[serde(rename_all = "camelCase")]
    PromptAnswer { id: u32, yes: bool },
    /// Déverrouillage d'un coffre existant.
    Unlock { passphrase: Secret },
    /// Création du coffre (premier lancement), avec import optionnel de la
    /// capsule. `capsule_passphrase` vide = même passphrase que le nouveau.
    #[serde(rename_all = "camelCase")]
    Create {
        passphrase: Secret,
        confirm: Secret,
        #[serde(default)]
        import_capsule: bool,
        #[serde(default)]
        capsule_passphrase: Secret,
    },
    /// Import de la capsule depuis les réglages (coffre déjà ouvert).
    ImportCapsule { passphrase: Secret },
    /// Sonder à nouveau la capsule (après `wsl -d NyxCapsule -e true`).
    RetryCapsule,
    /// Fermer le panneau d'import de la capsule.
    CloseOverlay,
    /// Quitter Nyx (écran de déverrouillage).
    Quit,
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

    /// Commandes acceptées tant que le coffre est verrouillé.
    pub fn allowed_while_locked(&self) -> bool {
        matches!(
            self,
            Self::Ready | Self::Unlock { .. } | Self::Create { .. } | Self::RetryCapsule | Self::Quit
        )
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

/// Question Oui/Non affichée sous la barre (jamais le mot de passe lui-même).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromptView {
    pub id: u32,
    pub text: String,
}

/// État de la capsule WSL tel qu'affiché.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CapsuleView {
    Probing,
    Found,
    Missing,
    Unreachable,
}

/// Panneau plein écran du chrome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Overlay {
    /// Déverrouillage (`creating` : premier lancement).
    Unlock { creating: bool, capsule: CapsuleView, error: String },
    /// Import de la capsule depuis les réglages.
    Capsule { capsule: CapsuleView, error: String },
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
    pub bookmarked: bool,
    pub can_bookmark: bool,
    pub prompt: Option<PromptView>,
    pub overlay: Option<Overlay>,
    /// Commande à lancer si la capsule est injoignable.
    pub capsule_cmd: &'static str,
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
pub fn js_literal<T: Serialize + ?Sized>(value: &T) -> String {
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
        assert_eq!(ChromeMsg::parse(r#"{"cmd":"toggleBookmark"}"#), Some(ChromeMsg::ToggleBookmark));
        assert_eq!(
            ChromeMsg::parse(r#"{"cmd":"promptAnswer","id":7,"yes":true}"#),
            Some(ChromeMsg::PromptAnswer { id: 7, yes: true })
        );
    }

    #[test]
    fn parses_vault_commands() {
        assert_eq!(
            ChromeMsg::parse(r#"{"cmd":"unlock","passphrase":"pw"}"#),
            Some(ChromeMsg::Unlock { passphrase: Secret::new("pw".into()) })
        );
        assert_eq!(
            ChromeMsg::parse(r#"{"cmd":"create","passphrase":"a","confirm":"b"}"#),
            Some(ChromeMsg::Create {
                passphrase: Secret::new("a".into()),
                confirm: Secret::new("b".into()),
                import_capsule: false,
                capsule_passphrase: Secret::default(),
            })
        );
        let full = r#"{"cmd":"create","passphrase":"a","confirm":"a","importCapsule":true,"capsulePassphrase":"old"}"#;
        assert!(matches!(
            ChromeMsg::parse(full),
            Some(ChromeMsg::Create { import_capsule: true, capsule_passphrase, .. }) if capsule_passphrase.expose() == "old"
        ));
        assert_eq!(
            ChromeMsg::parse(r#"{"cmd":"importCapsule","passphrase":"x"}"#),
            Some(ChromeMsg::ImportCapsule { passphrase: Secret::new("x".into()) })
        );
        for bare in ["retryCapsule", "closeOverlay", "quit"] {
            assert!(ChromeMsg::parse(&format!(r#"{{"cmd":"{bare}"}}"#)).is_some(), "{bare}");
        }
    }

    #[test]
    fn passphrase_never_in_debug() {
        let msg = ChromeMsg::parse(r#"{"cmd":"unlock","passphrase":"hunter2"}"#).expect("msg");
        assert!(!format!("{msg:?}").contains("hunter2"));
    }

    #[test]
    fn locked_gate() {
        assert!(ChromeMsg::Ready.allowed_while_locked());
        assert!(ChromeMsg::Quit.allowed_while_locked());
        assert!(ChromeMsg::Unlock { passphrase: Secret::default() }.allowed_while_locked());
        assert!(!ChromeMsg::NewTab.allowed_while_locked());
        assert!(!ChromeMsg::Navigate { text: "x".into() }.allowed_while_locked());
        assert!(!ChromeMsg::ImportCapsule { passphrase: Secret::default() }.allowed_while_locked());
        assert!(!ChromeMsg::PromptAnswer { id: 1, yes: true }.allowed_while_locked());
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
            r#"{"cmd":"unlock"}"#,
            r#"{"cmd":"unlock","passphrase":1}"#,
            r#"{"cmd":"unlock","passphrase":"x","path":"C:\\evil.db"}"#,
            r#"{"cmd":"create","passphrase":"a"}"#,
            r#"{"cmd":"promptAnswer","id":1}"#,
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
            prompt: Some(PromptView { id: 1, text: "<b>".into() }),
            ..ChromeState::default()
        };
        let js = update_script(&state);
        assert!(js.starts_with("window.nyx&&window.nyx.update({"));
        assert!(!js.contains('<'));
        assert!(!js.contains('\u{2028}') && !js.contains('\u{2029}'));
        assert!(js.contains("canGoBack") && js.contains("canBookmark"));

        let toast = toast_script("a\"b<c");
        assert_eq!(toast, r#"window.nyx&&window.nyx.toast("a\"b\u003cc")"#);
    }

    #[test]
    fn overlay_serialization() {
        let state = ChromeState {
            overlay: Some(Overlay::Unlock { creating: true, capsule: CapsuleView::Unreachable, error: String::new() }),
            capsule_cmd: crate::paths::START_CAPSULE_CMD,
            ..ChromeState::default()
        };
        let js = update_script(&state);
        assert!(js.contains(r#""overlay":{"kind":"unlock","creating":true,"capsule":"unreachable","error":""}"#), "{js}");
        assert!(js.contains("wsl -d NyxCapsule -e true"));
    }
}
