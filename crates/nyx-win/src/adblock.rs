//! Blocage des sous-ressources — même modèle que `nyx-ffi` (Android).
//!
//! `NyxGuard` (règles intégrées) s'applique dès le démarrage ; le moteur
//! EasyList/EasyPrivacy (`AdFilter`) est construit sur un thread d'arrière-
//! plan puis publié dans un `OnceLock` (lecture sans verrou ensuite).

use std::sync::OnceLock;

use nyx_core::adfilter::AdFilter;
use nyx_core::nyxguard::NyxGuard;
use nyx_core::url;

static FILTER: OnceLock<AdFilter> = OnceLock::new();
static GUARD: OnceLock<NyxGuard> = OnceLock::new();

const EASYLIST: &[u8] = include_bytes!("../../../assets/filterlists/easylist.txt.gz");
const EASYPRIVACY: &[u8] = include_bytes!("../../../assets/filterlists/easyprivacy.txt.gz");

/// Garde partagée (politique de navigation + blocage).
pub fn guard() -> &'static NyxGuard {
    GUARD.get_or_init(NyxGuard::new)
}

/// Lance la construction du moteur EasyList hors du thread UI (~1 s).
/// Idempotent ; un échec de thread laisse simplement `NyxGuard` seul.
pub fn init_in_background() {
    if FILTER.get().is_some() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("nyx-adfilter".into())
        .spawn(|| {
            FILTER.get_or_init(|| AdFilter::from_gz_lists([EASYLIST, EASYPRIVACY]));
        });
    if spawned.is_err() {
        nyx_core::sec_log::emit(nyx_core::sec_log::Level::Warn, "adfilter: thread non lancé");
    }
}

/// Décision de blocage d'une sous-ressource. Ne bloque JAMAIS un document
/// principal (`resource_type == "document"`) ni un scheme non-http(s).
pub fn should_block(target: &str, source_url: &str, resource_type: &str) -> bool {
    if resource_type == "document" {
        return false;
    }
    if !matches!(url::scheme_of(target).as_deref(), Some("http" | "https")) {
        return false;
    }
    if guard().should_block(target) {
        return true;
    }
    // EasyList suit l'interrupteur NyxGuard des réglages.
    guard().is_enabled()
        && FILTER
            .get()
            .is_some_and(|f| f.should_block(target, source_url, resource_type))
}

/// WebView2 range les documents principaux ET les (i)frames sous le même
/// contexte `DOCUMENT`. Seule la navigation principale en cours
/// (`pending_main`) est un « document » ; le reste est une `subdocument`.
/// L'ancre (`#…`) n'est jamais envoyée au réseau : on l'ignore.
pub fn document_type(uri: &str, pending_main: &str) -> &'static str {
    if uri.split('#').next() == pending_main.split('#').next() {
        "document"
    } else {
        "subdocument"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_subdocuments() {
        let main = "https://news.example/article";
        assert_eq!(document_type(main, main), "document");
        assert_eq!(document_type(main, "https://news.example/article#comments"), "document");
        assert_eq!(document_type("https://ad.doubleclick.net/frame", main), "subdocument");
        assert!(!should_block(main, "", document_type(main, main)));
        assert!(should_block("https://ad.doubleclick.net/frame", main, document_type("https://ad.doubleclick.net/frame", main)));
    }

    #[test]
    fn builtin_rules_apply_before_init() {
        assert!(should_block("https://ad.doubleclick.net/x.js", "https://news.example/", "script"));
        assert!(!should_block("https://ad.doubleclick.net/", "", "document"));
        assert!(!should_block("data:text/javascript,1", "", "script"));
        assert!(!should_block("https://nyx.localhost/newtab", "", "other"));
        assert!(!should_block("https://duckduckgo.com/", "", "xmlhttprequest"));
    }

    #[test]
    fn embedded_lists_build() {
        // Construit le vrai moteur (listes embarquées) : vérifie que les
        // fichiers gz sont lisibles et qu'une règle EasyList connue matche.
        let f = AdFilter::from_gz_lists([EASYLIST, EASYPRIVACY]);
        assert!(f.should_block("https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js", "https://news.example/", "script"));
        assert!(!f.should_block("https://news.example/app.js", "https://news.example/", "script"));
    }
}
