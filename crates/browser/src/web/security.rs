//! Décision de navigation — couche de sécurité **pure**.
//!
//! Aucune dépendance GTK/WebKit ici : on prend une URL + le contexte de
//! confiance et on rend un verdict. `web/mod.rs` se contente d'exécuter ce
//! verdict (ignore/charge) — la *politique* vit ici, la *mécanique* là-bas.
//! (Sprint 4-5 : TLS, IDN/punycode, permissions viendront enrichir ce module,
//! candidat à devenir la crate `nyx-security`.)

use crate::web::nyxguard::NyxGuard;

/// Ce que la couche moteur doit faire d'une navigation.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Laisser WebKit poursuivre.
    Allow,
    /// Annuler la navigation, ne rien charger (pub bloquée, action non autorisée).
    Block,
    /// Annuler la navigation et appliquer les réglages (`nyx://apply`).
    ApplySettings,
    /// Annuler la navigation et effacer l'historique (`nyx://history/clear`).
    ClearHistory,
    /// Annuler la navigation et charger une page interne.
    Load(Page),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Page { Settings, Bookmarks, History, NewTab }

/// Décide du sort d'une navigation.
///
/// `page_internal` indique si la page émettrice est de confiance
/// (`file://…/assets/` ou `nyx://`). C'est la frontière de sécurité : une
/// page web distante ne peut ni muter les réglages (`nyx://apply`) ni
/// effacer l'historique (`nyx://history/clear`).
pub fn decide(url: &str, page_internal: bool, guard: &NyxGuard) -> Verdict {
    if let Some(rest) = url.strip_prefix("nyx://") {
        if rest.starts_with("apply") {
            return if page_internal { Verdict::ApplySettings } else { Verdict::Block };
        }
        if rest.starts_with("history/clear") {
            return if page_internal { Verdict::ClearHistory } else { Verdict::Block };
        }
        if rest.starts_with("settings")  { return Verdict::Load(Page::Settings); }
        if rest.starts_with("bookmarks") { return Verdict::Load(Page::Bookmarks); }
        if rest.starts_with("history")   { return Verdict::Load(Page::History); }
        return Verdict::Load(Page::NewTab);
    }
    if guard.should_block(url) {
        return Verdict::Block;
    }
    Verdict::Allow
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> NyxGuard { NyxGuard::new() }

    #[test] fn allows_normal()       { assert_eq!(decide("https://example.com", true, &guard()), Verdict::Allow); }
    #[test] fn blocks_ad() {
        // URL d'iframe pub réelle d'EasyList (les règles modernes ciblent des
        // motifs précis, pas le domaine nu).
        assert_eq!(
            decide("https://googleads.g.doubleclick.net/pagead/ads?client=x", true, &guard()),
            Verdict::Block
        );
    }
    #[test] fn settings_page()       { assert_eq!(decide("nyx://settings", false, &guard()), Verdict::Load(Page::Settings)); }
    #[test] fn bookmarks_page()      { assert_eq!(decide("nyx://bookmarks", false, &guard()), Verdict::Load(Page::Bookmarks)); }
    #[test] fn newtab_fallback()     { assert_eq!(decide("nyx://newtab", false, &guard()), Verdict::Load(Page::NewTab)); }
    #[test] fn unknown_nyx_to_newtab(){ assert_eq!(decide("nyx://wat", false, &guard()), Verdict::Load(Page::NewTab)); }

    #[test] fn history_page() {
        assert_eq!(decide("nyx://history", false, &guard()), Verdict::Load(Page::History));
    }

    #[test] fn apply_from_internal_ok() {
        assert_eq!(decide("nyx://apply?dark=true", true, &guard()), Verdict::ApplySettings);
    }
    #[test] fn apply_from_remote_blocked() {
        // SÉCURITÉ : une page distante ne peut pas muter les réglages.
        assert_eq!(decide("nyx://apply?adblock=false", false, &guard()), Verdict::Block);
    }
    #[test] fn clear_history_from_internal_ok() {
        assert_eq!(decide("nyx://history/clear", true, &guard()), Verdict::ClearHistory);
    }
    #[test] fn clear_history_from_remote_blocked() {
        // SÉCURITÉ : une page distante ne peut pas effacer l'historique.
        assert_eq!(decide("nyx://history/clear", false, &guard()), Verdict::Block);
    }
    #[test] fn guard_off_allows_ad() {
        let g = guard();
        g.set_enabled(false);
        assert_eq!(
            decide("https://googleads.g.doubleclick.net/pagead/ads?client=x", true, &g),
            Verdict::Allow
        );
    }
}
