//! Traduction URL moteur ↔ politique Nyx — pur, testé sur l'hôte.
//!
//! Sous Windows, wry sert le protocole personnalisé `nyx` à l'origine
//! `https://nyx.localhost/` (WebView2 ne connaît pas les schemes non
//! standards). La politique (`nyx_core::security`) raisonne, elle, en
//! `nyx://…` : ce module fait la conversion dans les deux sens et réduit le
//! `Verdict` à ce que la coquille sait exécuter en phase 1.

use std::borrow::Cow;

use nyx_core::nyxguard::NyxGuard;
use nyx_core::security::{self, Verdict};
use nyx_core::state::settings::AppSettings;
use nyx_core::url;

/// Origine des pages internes (contenu). Toujours en minuscules : Chromium
/// normalise scheme et hôte.
pub const INTERNAL_ORIGIN: &str = "https://nyx.localhost/";
/// Page Nouvel onglet / Accueil.
pub const NEWTAB_URL: &str = "https://nyx.localhost/newtab";
/// Origine du chrome (barre d'outils), protocole distinct `nyxui` enregistré
/// uniquement sur la WebView du chrome.
pub const CHROME_ORIGIN: &str = "https://nyxui.localhost/";
pub const CHROME_URL: &str = "https://nyxui.localhost/index.html";

/// Ce que la coquille doit faire d'une navigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Nav {
    Allow,
    Deny,
    /// Annuler puis charger cette URL (page interne).
    Redirect(&'static str),
}

/// `true` si `url` appartient à l'origine `origin` (préfixe exact, casse
/// ignorée sur la partie origine).
fn has_origin(url: &str, origin: &str) -> bool {
    url.get(..origin.len()).is_some_and(|p| p.eq_ignore_ascii_case(origin))
}

/// Page interne de confiance (servie par notre protocole `nyx`) ?
pub fn is_internal(url: &str) -> bool {
    has_origin(url, INTERNAL_ORIGIN)
}

/// URL issue du chrome ?
pub fn is_chrome(url: &str) -> bool {
    has_origin(url, CHROME_ORIGIN)
}

/// URL moteur → URL politique : `https://nyx.localhost/x` → `nyx://x`.
pub fn to_policy_url(url: &str) -> Cow<'_, str> {
    if is_internal(url) {
        Cow::Owned(format!("nyx://{}", &url[INTERNAL_ORIGIN.len()..]))
    } else {
        Cow::Borrowed(url)
    }
}

/// URL politique → URL moteur : `nyx://x` → `https://nyx.localhost/x`.
pub fn to_engine_url(url: &str) -> Cow<'_, str> {
    const NYX: &str = "nyx://";
    if has_origin(url, NYX) {
        Cow::Owned(format!("{INTERNAL_ORIGIN}{}", &url[NYX.len()..]))
    } else {
        Cow::Borrowed(url)
    }
}

/// `true` pour la page Nouvel onglet (avec ou sans `/newtab`, query, ancre).
pub fn is_newtab(url: &str) -> bool {
    if !is_internal(url) {
        return false;
    }
    let path = url[INTERNAL_ORIGIN.len()..].split(['?', '#']).next().unwrap_or("");
    matches!(path, "" | "newtab" | "newtab/")
}

/// URL affichée dans la barre d'adresse : vide pour Nouvel onglet (le
/// placeholder s'affiche), `nyx://…` pour les autres pages internes.
pub fn display_url(url: &str) -> String {
    if is_newtab(url) || url == "about:blank" {
        String::new()
    } else {
        to_policy_url(url).into_owned()
    }
}

/// Saisie de la barre d'adresse → URL moteur à charger, ou `None` si la
/// saisie n'est pas navigable (vide, `file://`…). Réglages par défaut en
/// phase 1 (le coffre/les réglages persistants viendront avec `vault`).
pub fn resolve_address(text: &str) -> Option<String> {
    let resolved = url::resolve_input(text, &AppSettings::default());
    url::is_navigable(&resolved).then(|| to_engine_url(&resolved).into_owned())
}

/// Décide d'une navigation de contenu (`target`) depuis la page `current`.
/// Toute la politique vit dans `nyx_core::security::decide` ; ici on ne fait
/// que réduire le verdict à ce que la phase 1 implémente.
pub fn decide(target: &str, current: &str, guard: &NyxGuard) -> Nav {
    let page_internal = is_internal(current);
    match security::decide(&to_policy_url(target), page_internal, guard) {
        Verdict::Allow => Nav::Allow,
        // Phase 1 : toutes les pages internes (réglages, favoris,
        // historique…) retombent sur Nouvel onglet.
        Verdict::Load(_) if is_newtab(target) => Nav::Allow,
        Verdict::Load(_) => Nav::Redirect(NEWTAB_URL),
        // Refus purs + actions internes (apply/move/history-clear/import) :
        // non implémentées en phase 1 → refusées partout.
        Verdict::Block
        | Verdict::BlockFileAccess
        | Verdict::ApplySettings
        | Verdict::MoveBookmark
        | Verdict::ClearHistory
        | Verdict::ImportChrome => Nav::Deny,
    }
}

/// Le chrome ne quitte jamais sa propre origine.
pub fn chrome_may_navigate(target: &str) -> bool {
    is_chrome(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g() -> NyxGuard {
        NyxGuard::new()
    }

    #[test]
    fn origin_mapping_round_trips() {
        assert_eq!(to_policy_url("https://nyx.localhost/newtab"), "nyx://newtab");
        assert_eq!(to_policy_url("HTTPS://NYX.LOCALHOST/apply?x=1"), "nyx://apply?x=1");
        assert_eq!(to_policy_url("https://nyx.localhost.evil.com/"), "https://nyx.localhost.evil.com/");
        assert_eq!(to_policy_url("http://nyx.localhost/newtab"), "http://nyx.localhost/newtab");
        assert_eq!(to_engine_url("nyx://newtab"), NEWTAB_URL);
        assert_eq!(to_engine_url("NYX://settings"), "https://nyx.localhost/settings");
        assert_eq!(to_engine_url("https://a.b/"), "https://a.b/");
        assert_eq!(to_engine_url("ny"), "ny");
        assert_eq!(to_policy_url(""), "");
    }

    #[test]
    fn newtab_detection() {
        assert!(is_newtab(NEWTAB_URL));
        assert!(is_newtab("https://nyx.localhost/"));
        assert!(is_newtab("https://nyx.localhost/newtab?q=1#x"));
        assert!(!is_newtab("https://nyx.localhost/settings"));
        assert!(!is_newtab("https://example.com/newtab"));
        assert_eq!(display_url(NEWTAB_URL), "");
        assert_eq!(display_url("about:blank"), "");
        assert_eq!(display_url("https://nyx.localhost/history"), "nyx://history");
        assert_eq!(display_url("https://a.b/c"), "https://a.b/c");
    }

    #[test]
    fn address_bar() {
        assert_eq!(resolve_address("github.com").as_deref(), Some("https://github.com"));
        assert_eq!(resolve_address("nyx://newtab").as_deref(), Some(NEWTAB_URL));
        assert!(resolve_address("rust async").is_some_and(|u| u.starts_with("https://duckduckgo.com/")));
        assert_eq!(resolve_address("file:///C:/Windows/win.ini"), None);
        assert_eq!(resolve_address("   "), None);
        assert!(resolve_address("javascript:alert(1)").is_some_and(|u| u.contains("duckduckgo.com")));
    }

    #[test]
    fn content_navigation() {
        let remote = "https://example.com/";
        assert_eq!(decide("https://rust-lang.org/", remote, &g()), Nav::Allow);
        assert_eq!(decide("https://rust-lang.org/", NEWTAB_URL, &g()), Nav::Allow);
        // Première navigation d'une WebView neuve (page courante vide).
        assert_eq!(decide(NEWTAB_URL, "", &g()), Nav::Allow);
        assert_eq!(decide("about:blank", "", &g()), Nav::Allow);
        // Pages internes non implémentées → Nouvel onglet.
        assert_eq!(decide("https://nyx.localhost/settings", remote, &g()), Nav::Redirect(NEWTAB_URL));
        assert_eq!(decide("nyx://bookmarks", remote, &g()), Nav::Redirect(NEWTAB_URL));
        // Pub / tracker en navigation principale : refusé par la politique.
        assert_eq!(decide("https://ad.doubleclick.net/x", remote, &g()), Nav::Deny);
    }

    #[test]
    fn security_boundaries() {
        let remote = "https://evil.example/";
        // file:// jamais depuis le web.
        assert_eq!(decide("file:///C:/Users/me/.ssh/id_rsa", remote, &g()), Nav::Deny);
        assert_eq!(decide("FILE:///C:/Windows/System32/config", remote, &g()), Nav::Deny);
        // Actions internes : refusées depuis le web…
        for action in ["apply?adblock=false", "move?idx=0", "history/clear", "import/chrome"] {
            assert_eq!(decide(&format!("nyx://{action}"), remote, &g()), Nav::Deny, "{action}");
            assert_eq!(decide(&format!("{INTERNAL_ORIGIN}{action}"), remote, &g()), Nav::Deny, "{action}");
            // … et aussi depuis une page interne (non implémentées en phase 1).
            assert_eq!(decide(&format!("{INTERNAL_ORIGIN}{action}"), NEWTAB_URL, &g()), Nav::Deny, "{action}");
        }
        // Un sous-domaine piège n'est pas « interne ».
        assert!(!is_internal("https://nyx.localhost.evil.com/newtab"));
        assert!(!is_internal("https://nyx.localhostevil.com/"));
    }

    #[test]
    fn chrome_is_pinned() {
        assert!(chrome_may_navigate(CHROME_URL));
        assert!(!chrome_may_navigate("https://example.com/"));
        assert!(!chrome_may_navigate(NEWTAB_URL));
        assert!(!chrome_may_navigate("https://nyxui.localhost.evil.com/"));
        assert!(!chrome_may_navigate("about:blank"));
    }
}
