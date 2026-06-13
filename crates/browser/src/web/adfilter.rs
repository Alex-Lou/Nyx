//! Filtre publicitaire production — moteur `adblock` de Brave nourri par
//! EasyList + EasyPrivacy bundlées (~82 000 règles).
//!
//! Pourquoi ici et pas dans `nyx_core::nyxguard` ? Le moteur Brave n'est ni
//! `Send` ni `Sync`, or NyxGuard est partagé en `Arc` et stressé en
//! multi-thread (tests `hostile_stress`). On garde donc la politique pure
//! (rapide, sûre, multi-thread) dans nyx-core, et ce moteur lourd vit sur le
//! thread GTK uniquement, consulté EN COMPLÉMENT à la frontière decide-policy.
//! Les sous-ressources (img/script/xhr) sont bloquées par le content filter
//! au niveau navigation/iframes (decide-policy). Le filtrage des sous-
//! ressources via FFI a été retiré (segfault selon la version WebKitGTK).

use std::cell::OnceCell;
use std::io::Read;

use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::Engine;

const EASYLIST_GZ: &[u8] = include_bytes!("../../../../assets/filterlists/easylist.txt.gz");
const EASYPRIVACY_GZ: &[u8] = include_bytes!("../../../../assets/filterlists/easyprivacy.txt.gz");

thread_local! {
    /// Lazy : le parse des listes (~3,5 MB) ne se paie qu'à la première
    /// vérification d'URL, pas au démarrage.
    static ENGINE: OnceCell<Engine> = const { OnceCell::new() };
}

/// true si l'URL de navigation matche les règles EasyList/EasyPrivacy.
/// À consulter seulement si l'adblock est activé (le toggle vit dans
/// AppSettings / NyxGuard, pas ici).
pub fn should_block(url: &str) -> bool {
    ENGINE.with(|cell| {
        let engine = cell.get_or_init(|| Engine::from_filter_set(bundled_filter_set(), true));
        Request::new(url, url, "document")
            .map(|r| engine.check_network_request(&r).matched)
            .unwrap_or(false)
    })
}

fn bundled_filter_set() -> FilterSet {
    let mut set = FilterSet::new(false);
    for gz in [EASYLIST_GZ, EASYPRIVACY_GZ] {
        set.add_filters(gunzip(gz).lines(), ParseOptions::default());
    }
    set
}

fn gunzip(data: &[u8]) -> String {
    let mut out = String::new();
    let _ = flate2::read::GzDecoder::new(data).read_to_string(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// URL d'iframe pub réelle d'EasyList (les règles modernes ciblent des
    /// motifs précis, pas le domaine nu). Un seul test : la construction du
    /// moteur parse ~3,5 MB.
    #[test]
    fn moteur_easylist() {
        assert!(should_block("https://googleads.g.doubleclick.net/pagead/ads?client=x"));
        assert!(should_block("https://securepubads.g.doubleclick.net/tag/js/gpt.js"));
        assert!(!should_block("https://notdoubleclick.net/"));
        assert!(!should_block("https://duckduckgo.com/?q=rust"));
        assert!(!should_block("https://example.com/?ref=doubleclick.net"));
    }
}
