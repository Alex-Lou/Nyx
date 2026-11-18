//! Filtre publicitaire production — moteur `nyx_core::adfilter` (Brave
//! `adblock`) nourri par EasyList + EasyPrivacy bundlées (~82 000 règles).
//!
//! Le moteur lui-même vit dans nyx-core (partagé avec Android, où il est
//! `Send + Sync`). Côté desktop on garde une instance par thread GTK,
//! consultée EN COMPLÉMENT à `NyxGuard` à la frontière decide-policy.
//! Les sous-ressources (img/script/xhr) sont bloquées par le content filter
//! au niveau navigation/iframes (decide-policy). Le filtrage des sous-
//! ressources via FFI a été retiré (segfault selon la version WebKitGTK).

use std::cell::OnceCell;

use nyx_core::adfilter::AdFilter;

const EASYLIST_GZ: &[u8] = include_bytes!("../../../../assets/filterlists/easylist.txt.gz");
const EASYPRIVACY_GZ: &[u8] = include_bytes!("../../../../assets/filterlists/easyprivacy.txt.gz");

thread_local! {
    /// Lazy : le parse des listes (~3,5 MB) ne se paie qu'à la première
    /// vérification d'URL, pas au démarrage.
    static ENGINE: OnceCell<AdFilter> = const { OnceCell::new() };
}

fn build() -> AdFilter {
    AdFilter::from_gz_lists([EASYLIST_GZ, EASYPRIVACY_GZ])
}

/// Construit le moteur à l'avance (appelé en idle après l'affichage de la
/// première fenêtre) pour que la première navigation ne paie pas le parse.
pub fn warm_up() {
    ENGINE.with(|cell| {
        cell.get_or_init(build);
    });
}

/// true si l'URL de navigation matche les règles EasyList/EasyPrivacy.
/// À consulter seulement si l'adblock est activé (le toggle vit dans
/// AppSettings / NyxGuard, pas ici).
pub fn should_block(url: &str) -> bool {
    ENGINE.with(|cell| cell.get_or_init(build).should_block(url, url, "document"))
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
