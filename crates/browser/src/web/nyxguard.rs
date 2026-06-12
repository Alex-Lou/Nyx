//! NyxGuard — le bouclier de Nyx : pub / trackers / connexions tierces.
//!
//! Sprint 3 (fait) : le jeu de règles pub est le moteur `adblock` de Brave,
//! nourri par EasyList + EasyPrivacy bundlées (assets/filterlists/, ~82 000
//! règles). Il filtre ici les *navigations* ; les sous-ressources (img,
//! script, xhr…) sont bloquées par le content filter WebKit compilé depuis
//! les mêmes listes (web/content_filter.rs).
//!
//! Deux jeux de règles indépendants, chacun avec son interrupteur atomique
//! (le toggle des réglages agit immédiatement sur tous les onglets, qui
//! partagent le même `Rc<NyxGuard>`) :
//!   • pub/trackers (moteur Brave) — activé par défaut
//!   • connexions tierces (Google/Meta/Apple… auth & SDK) — option opt-in

use std::cell::OnceCell;
use std::collections::HashSet;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};

use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::Engine;

const EASYLIST_GZ: &[u8] = include_bytes!("../../../../assets/filterlists/easylist.txt.gz");
const EASYPRIVACY_GZ: &[u8] = include_bytes!("../../../../assets/filterlists/easyprivacy.txt.gz");

pub struct NyxGuard {
    ads_on:      AtomicBool,
    accounts_on: AtomicBool,
    /// Lazy : le parse des listes (~3,5 MB) ne se paie qu'à la première
    /// vérification d'URL, pas au démarrage (la fenêtre s'ouvre tout de suite).
    engine:      OnceCell<Engine>,
    accounts:    HashSet<String>,
}

impl NyxGuard {
    pub fn new() -> Self {
        Self {
            ads_on:      AtomicBool::new(true),
            accounts_on: AtomicBool::new(false),
            engine:      OnceCell::new(),
            accounts:    ACCOUNT_DOMAINS.iter().map(|s| s.to_string()).collect(),
        }
    }

    pub fn set_enabled(&self, v: bool)        { self.ads_on.store(v, Ordering::Relaxed); }
    pub fn set_block_accounts(&self, v: bool) { self.accounts_on.store(v, Ordering::Relaxed); }

    pub fn should_block(&self, url: &str) -> bool {
        let host = extract_host(url);
        if self.ads_on.load(Ordering::Relaxed) {
            let ad = Request::new(url, url, "document")
                .map(|r| self.engine().check_network_request(&r).matched)
                .unwrap_or(false);
            if ad { return true; }
        }
        if self.accounts_on.load(Ordering::Relaxed)
            && self.accounts.iter().any(|d| host_matches(host, d))
        {
            return true;
        }
        false
    }

    fn engine(&self) -> &Engine {
        self.engine
            .get_or_init(|| Engine::from_filter_set(bundled_filter_set(false), true))
    }
}

impl Default for NyxGuard {
    fn default() -> Self { Self::new() }
}

/// JSON content-blocker WebKit généré depuis les listes bundlées.
/// Coûteux (re-parse en mode debug, requis par la conversion) : appelé une
/// seule fois, à la première compilation du filtre (cache disque ensuite).
pub fn content_blocking_json() -> String {
    match bundled_filter_set(true).into_content_blocking() {
        Ok((rules, _unsupported)) => serde_json::to_string(&rules).unwrap_or_else(|_| "[]".into()),
        Err(_) => "[]".into(),
    }
}

fn bundled_filter_set(debug: bool) -> FilterSet {
    let mut set = FilterSet::new(debug);
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

/// `host == d` ou sous-domaine de `d` (`.d`). Évite l'injection `d.evil.com`.
fn host_matches(host: &str, d: &str) -> bool {
    host.strip_suffix(d)
        .is_some_and(|rest| rest.is_empty() || rest.ends_with('.'))
}

/// `"https://ads.x.com/img"` → `"ads.x.com"`.
pub fn extract_host(url: &str) -> &str {
    let after = url.find("://").map(|i| &url[i + 3..]).unwrap_or(url);
    let end   = after.find(['/', ':', '?', '#']).unwrap_or(after.len());
    &after[..end]
}

/// Domaines d'authentification / SDK tiers — « Se connecter avec Google… ».
const ACCOUNT_DOMAINS: &[&str] = &[
    "accounts.google.com", "apis.google.com", "oauth2.googleapis.com",
    "connect.facebook.net", "graph.facebook.com",
    "appleid.apple.com",
    "login.microsoftonline.com", "login.live.com",
    "api.linkedin.com", "platform.twitter.com",
];

#[cfg(test)]
mod tests {
    use super::*;

    // Le moteur parse ~3,5 MB de règles : on le construit UNE fois par test
    // groupé plutôt qu'un test par cas.

    /// URL d'iframe pub réelle d'EasyList (les règles modernes ciblent des
    /// motifs précis, pas le domaine nu).
    const AD_IFRAME: &str = "https://googleads.g.doubleclick.net/pagead/ads?client=x";

    #[test]
    fn moteur_easylist() {
        let b = NyxGuard::new();
        assert!(b.should_block(AD_IFRAME));
        assert!(b.should_block("https://securepubads.g.doubleclick.net/tag/js/gpt.js"));
        assert!(!b.should_block("https://notdoubleclick.net/"));
        assert!(!b.should_block("https://duckduckgo.com/?q=rust"));
        assert!(!b.should_block("https://example.com/?ref=doubleclick.net"));

        // Toggle pub
        b.set_enabled(false);
        assert!(!b.should_block(AD_IFRAME));
    }

    #[test]
    fn accounts_opt_in() {
        let b = NyxGuard::new();
        // off par défaut
        assert!(!b.should_block("https://accounts.google.com/o/oauth2/auth"));
        b.set_block_accounts(true);
        assert!(b.should_block("https://accounts.google.com/o/oauth2/auth"));
        assert!(b.should_block("https://appleid.apple.com/auth/authorize"));
        // un site légitime non-auth reste autorisé
        assert!(!b.should_block("https://www.google.com/search?q=x"));
    }

    /// « Centaines d'utilisateurs » : 50 000 vérifications d'URL variées sur
    /// un bloqueur partagé, en basculant les flags. Doit rester correct et
    /// rapide, sans paniquer.
    #[test]
    fn stress_many_urls() {
        let b = NyxGuard::new();
        b.set_block_accounts(true);
        let urls = [
            AD_IFRAME, "https://example.com/page",
            "https://accounts.google.com/x", "https://duckduckgo.com/",
            "https://crates.io/crates/adblock", "https://notdoubleclick.net/a",
        ];
        let mut blocked = 0usize;
        for i in 0..50_000 {
            if b.should_block(urls[i % urls.len()]) { blocked += 1; }
        }
        assert!(blocked > 0 && blocked < 50_000);
    }

    /// URLs malformées / unicode : ne doit jamais paniquer.
    #[test]
    fn stress_malformed_urls() {
        let b = NyxGuard::new();
        let weird = [
            "", "::::", "https://", "https://////", "nyx://newtab",
            "ht!tp://@@@/x", "https://日本語.example.com/ぺージ",
            "https://doubleclick.net", "https://x.com:443/a#b?c",
            "javascript:alert(1)", "data:text/html,<b>x</b>",
        ];
        for _ in 0..1_000 {
            for w in &weird {
                let _ = b.should_block(w);
            }
        }
    }
}
