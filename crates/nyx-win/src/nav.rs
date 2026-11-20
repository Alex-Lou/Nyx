//! Traduction URL moteur ↔ politique Nyx — pur, testé sur l'hôte.
//!
//! Sous Windows, wry sert le protocole personnalisé `nyx` à l'origine
//! `https://nyx.localhost/` (WebView2 ne connaît pas les schemes non
//! standards). La politique (`nyx_core::security`) raisonne, elle, en
//! `nyx://…` : ce module fait la conversion dans les deux sens et traduit le
//! `Verdict` en ce que la coquille exécute (navigation, page, action).

use std::borrow::Cow;

use nyx_core::nyxguard::NyxGuard;
use nyx_core::security::{self, Page, Verdict};
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

/// Pages internes servies par le protocole `nyx`.
pub const SETTINGS_URL: &str = "https://nyx.localhost/settings";
pub const BOOKMARKS_URL: &str = "https://nyx.localhost/bookmarks";
pub const HISTORY_URL: &str = "https://nyx.localhost/history";
/// Lien « Tout effacer » de la page historique (navigation principale).
pub const HISTORY_CLEAR_URL: &str = "https://nyx.localhost/history/clear";

/// Ce que la coquille doit faire d'une navigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Nav {
    Allow,
    Deny,
    /// Annuler puis charger cette URL (page interne).
    Redirect(&'static str),
    /// Annuler puis exécuter cette action interne (page émettrice interne).
    Action(Action),
}

/// Action interne autorisée par `security::decide` (émetteur interne). Les
/// URL portées sont au format politique (`nyx://…`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Apply(String),
    MoveBookmark(String),
    ClearHistory,
    ImportChrome(String),
    ImportCapsule,
}

/// URL moteur canonique d'une page interne.
pub fn page_url(page: &Page) -> &'static str {
    match page {
        Page::Settings => SETTINGS_URL,
        Page::Bookmarks => BOOKMARKS_URL,
        Page::History => HISTORY_URL,
        Page::NewTab => NEWTAB_URL,
    }
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
/// saisie n'est pas navigable (vide, `file://`…). Moteur de recherche des
/// réglages de l'utilisateur.
pub fn resolve_address(text: &str, settings: &AppSettings) -> Option<String> {
    let resolved = url::resolve_input(text, settings);
    url::is_navigable(&resolved).then(|| to_engine_url(&resolved).into_owned())
}

/// Page d'accueil des réglages (URL moteur) ; Nouvel onglet si absente ou
/// non navigable.
pub fn home_url(settings: &AppSettings) -> String {
    let home = settings.home_url.trim();
    if home.is_empty() || !url::is_navigable(home) {
        return NEWTAB_URL.to_string();
    }
    to_engine_url(home).into_owned()
}

/// Verdict → action interne (les autres verdicts ne sont pas des actions).
fn action_of(verdict: Verdict, policy_url: &str) -> Option<Action> {
    Some(match verdict {
        Verdict::ApplySettings => Action::Apply(policy_url.to_string()),
        Verdict::MoveBookmark => Action::MoveBookmark(policy_url.to_string()),
        Verdict::ClearHistory => Action::ClearHistory,
        Verdict::ImportChrome => Action::ImportChrome(policy_url.to_string()),
        Verdict::ImportCapsule => Action::ImportCapsule,
        _ => return None,
    })
}

/// Action interne demandée par `target` depuis la page `current` (canal
/// natif des pages internes) — `None` si ce n'est pas une action autorisée.
pub fn internal_action(target: &str, current: &str, guard: &NyxGuard) -> Option<Action> {
    let policy = to_policy_url(target);
    if url::scheme_of(&policy).as_deref() != Some("nyx") {
        return None;
    }
    action_of(security::decide(&policy, is_internal(current), guard), &policy)
}

/// Décide d'une navigation de contenu (`target`) depuis la page `current`.
/// Toute la politique vit dans `nyx_core::security::decide` ; ici on ne fait
/// que traduire le verdict.
pub fn decide(target: &str, current: &str, guard: &NyxGuard) -> Nav {
    let policy = to_policy_url(target);
    match security::decide(&policy, is_internal(current), guard) {
        Verdict::Allow => Nav::Allow,
        Verdict::Block | Verdict::BlockFileAccess => Nav::Deny,
        // Page interne : servie telle quelle à son URL canonique, sinon
        // redirigée vers elle (`nyx://settings`, `/settingsX`…).
        Verdict::Load(page) => {
            let canonical = page_url(&page);
            let path = target.split(['?', '#']).next().unwrap_or("").trim_end_matches('/');
            let exact = path.eq_ignore_ascii_case(canonical) || (page == Page::NewTab && is_newtab(target));
            if exact {
                Nav::Allow
            } else {
                Nav::Redirect(canonical)
            }
        }
        verdict => action_of(verdict, &policy).map_or(Nav::Deny, Nav::Action),
    }
}

/// Navigation principale d'un onglet. Comme [`decide`], mais une action
/// interne n'est exécutée que si la navigation précédente autorisée
/// (`last_target`) était elle-même interne : une redirection HTTP depuis un
/// site (lien de l'historique → `evil.com` → 302 `…/history/clear`) arrive
/// alors que la page engagée est encore interne, sans en venir.
pub fn decide_main(target: &str, current: &str, last_target: &str, guard: &NyxGuard) -> Nav {
    match decide(target, current, guard) {
        Nav::Action(_) if !is_internal(last_target) => Nav::Deny,
        nav => nav,
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
        let s = AppSettings::default();
        assert_eq!(resolve_address("github.com", &s).as_deref(), Some("https://github.com"));
        assert_eq!(resolve_address("nyx://newtab", &s).as_deref(), Some(NEWTAB_URL));
        assert_eq!(resolve_address("nyx://settings", &s).as_deref(), Some(SETTINGS_URL));
        assert!(resolve_address("rust async", &s).is_some_and(|u| u.starts_with("https://duckduckgo.com/")));
        assert_eq!(resolve_address("file:///C:/Windows/win.ini", &s), None);
        assert_eq!(resolve_address("   ", &s), None);
        assert!(resolve_address("javascript:alert(1)", &s).is_some_and(|u| u.contains("duckduckgo.com")));
        let brave = AppSettings { search_engine: nyx_core::state::settings::SearchEngine::Brave, ..AppSettings::default() };
        assert!(resolve_address("rust", &brave).is_some_and(|u| u.starts_with("https://search.brave.com/")));
    }

    #[test]
    fn home_page() {
        let mut s = AppSettings::default();
        assert_eq!(home_url(&s), NEWTAB_URL);
        s.home_url = "https://exemple.fr/".into();
        assert_eq!(home_url(&s), "https://exemple.fr/");
        s.home_url = "nyx://bookmarks".into();
        assert_eq!(home_url(&s), BOOKMARKS_URL);
        s.home_url = "javascript:alert(1)".into();
        assert_eq!(home_url(&s), NEWTAB_URL);
        s.home_url = String::new();
        assert_eq!(home_url(&s), NEWTAB_URL);
    }

    #[test]
    fn content_navigation() {
        let remote = "https://example.com/";
        assert_eq!(decide("https://rust-lang.org/", remote, &g()), Nav::Allow);
        assert_eq!(decide("https://rust-lang.org/", NEWTAB_URL, &g()), Nav::Allow);
        // Première navigation d'une WebView neuve (page courante vide).
        assert_eq!(decide(NEWTAB_URL, "", &g()), Nav::Allow);
        assert_eq!(decide("about:blank", "", &g()), Nav::Allow);
        // Pages internes servies à leur URL canonique (lecture seule : un
        // lien depuis le web est permis, comme sur le desktop).
        assert_eq!(decide(SETTINGS_URL, remote, &g()), Nav::Allow);
        assert_eq!(decide("https://nyx.localhost/history/", remote, &g()), Nav::Allow);
        assert_eq!(decide(BOOKMARKS_URL, NEWTAB_URL, &g()), Nav::Allow);
        assert_eq!(decide("nyx://bookmarks", remote, &g()), Nav::Redirect(BOOKMARKS_URL));
        assert_eq!(decide("https://nyx.localhost/settingsX", remote, &g()), Nav::Redirect(SETTINGS_URL));
        assert_eq!(decide("https://nyx.localhost/wat", remote, &g()), Nav::Redirect(NEWTAB_URL));
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
        for action in ["apply?adblock=false", "move?idx=0", "history/clear", "import/chrome", "import/capsule"] {
            assert_eq!(decide(&format!("nyx://{action}"), remote, &g()), Nav::Deny, "{action}");
            assert_eq!(decide(&format!("{INTERNAL_ORIGIN}{action}"), remote, &g()), Nav::Deny, "{action}");
            assert_eq!(decide(&format!("{INTERNAL_ORIGIN}{action}"), "", &g()), Nav::Deny, "{action}");
            assert_eq!(internal_action(&format!("nyx://{action}"), remote, &g()), None, "{action}");
            // … exécutées depuis une page interne.
            assert!(matches!(decide(&format!("{INTERNAL_ORIGIN}{action}"), HISTORY_URL, &g()), Nav::Action(_)), "{action}");
        }
        assert_eq!(decide(HISTORY_CLEAR_URL, HISTORY_URL, &g()), Nav::Action(Action::ClearHistory));
        assert_eq!(
            decide("https://nyx.localhost/move?idx=1&to=A", BOOKMARKS_URL, &g()),
            Nav::Action(Action::MoveBookmark("nyx://move?idx=1&to=A".into()))
        );
        assert_eq!(internal_action("nyx://import/capsule", SETTINGS_URL, &g()), Some(Action::ImportCapsule));
        assert_eq!(internal_action("https://example.com/", SETTINGS_URL, &g()), None);
        assert_eq!(internal_action("nyx://settings", SETTINGS_URL, &g()), None);
        // Un sous-domaine piège n'est pas « interne ».
        assert!(!is_internal("https://nyx.localhost.evil.com/newtab"));
        assert!(!is_internal("https://nyx.localhostevil.com/"));
    }

    #[test]
    fn redirected_actions_are_denied() {
        // Clic direct sur « Tout effacer » : précédente navigation = la page.
        assert_eq!(decide_main(HISTORY_CLEAR_URL, HISTORY_URL, HISTORY_URL, &g()), Nav::Action(Action::ClearHistory));
        // Lien vers un site qui redirige vers l'action : refusé.
        assert_eq!(decide_main(HISTORY_CLEAR_URL, HISTORY_URL, "https://evil.example/r", &g()), Nav::Deny);
        assert_eq!(decide_main("https://nyx.localhost/apply?adblock=false", SETTINGS_URL, "", &g()), Nav::Deny);
        // Le reste est inchangé.
        assert_eq!(decide_main("https://a.org/", HISTORY_URL, "https://evil.example/", &g()), Nav::Allow);
        assert_eq!(decide_main("nyx://settings", "", "", &g()), Nav::Redirect(SETTINGS_URL));
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
