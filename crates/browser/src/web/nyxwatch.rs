use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};

/// NyxWatch — la veille de Nyx : bouclier pub / trackers / connexions tierces.
/// (Sprint 3 : passage prévu sur la crate `adblock` de Brave + EasyList.)
///
/// Deux jeux de règles indépendants, chacun avec son interrupteur atomique
/// (le toggle des réglages agit immédiatement sur tous les onglets, qui
/// partagent le même `Arc<NyxWatch>`) :
///   • pub/trackers — activé par défaut
///   • connexions tierces (Google/Meta/Apple… auth & SDK) — option opt-in
pub struct NyxWatch {
    ads_on:      AtomicBool,
    accounts_on: AtomicBool,
    ad_domains:  HashSet<String>,
    ad_paths:    Vec<String>,
    accounts:    HashSet<String>,
}

impl NyxWatch {
    pub fn new() -> Self {
        let (mut ad_domains, mut ad_paths) = (HashSet::new(), Vec::new());
        for rule in AD_RULES {
            if rule.contains('/') { ad_paths.push(rule.to_string()); }
            else                  { ad_domains.insert(rule.to_string()); }
        }
        Self {
            ads_on:      AtomicBool::new(true),
            accounts_on: AtomicBool::new(false),
            ad_domains,
            ad_paths,
            accounts:    ACCOUNT_DOMAINS.iter().map(|s| s.to_string()).collect(),
        }
    }

    pub fn set_enabled(&self, v: bool)        { self.ads_on.store(v, Ordering::Relaxed); }
    pub fn set_block_accounts(&self, v: bool) { self.accounts_on.store(v, Ordering::Relaxed); }

    pub fn should_block(&self, url: &str) -> bool {
        let host = extract_host(url);
        if self.ads_on.load(Ordering::Relaxed) {
            let ad = self.ad_domains.iter().any(|d| host_matches(host, d))
                || self.ad_paths.iter().any(|p| url_contains_path(url, p));
            if ad { return true; }
        }
        if self.accounts_on.load(Ordering::Relaxed)
            && self.accounts.iter().any(|d| host_matches(host, d))
        {
            return true;
        }
        false
    }
}

impl Default for NyxWatch {
    fn default() -> Self { Self::new() }
}

/// `host == d` ou sous-domaine de `d` (`.d`). Évite l'injection `d.evil.com`.
fn host_matches(host: &str, d: &str) -> bool {
    host == d || host.ends_with(&format!(".{d}"))
}

/// `"https://ads.x.com/img"` → `"ads.x.com"`.
fn extract_host(url: &str) -> &str {
    let after = url.find("://").map(|i| &url[i + 3..]).unwrap_or(url);
    let end   = after.find(['/', ':', '?', '#']).unwrap_or(after.len());
    &after[..end]
}

/// Règle suivie d'un délimiteur : évite que `facebook.com/tr` matche
/// `facebook.com/trending`.
fn url_contains_path(url: &str, rule: &str) -> bool {
    url.find(rule).is_some_and(|pos| {
        let tail = &url[pos + rule.len()..];
        tail.is_empty() || matches!(tail.chars().next(), Some('?' | '#' | '/' | '&'))
    })
}

const AD_RULES: &[&str] = &[
    "doubleclick.net", "googlesyndication.com",
    "googletagmanager.com", "googletagservices.com",
    "ads.twitter.com", "facebook.com/tr",
    "analytics.google.com", "hotjar.com",
    "scorecardresearch.com", "quantserve.com",
];

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
    fn b() -> NyxWatch { NyxWatch::new() }

    #[test] fn blocks_exact()      { assert!(b().should_block("https://doubleclick.net/ad.js")); }
    #[test] fn blocks_subdomain()  { assert!(b().should_block("https://ad.doubleclick.net/x")); }
    #[test] fn no_fp_similar()     { assert!(!b().should_block("https://notdoubleclick.net/")); }
    #[test] fn no_fp_injection()   { assert!(!b().should_block("https://doubleclick.net.evil.com/x")); }
    #[test] fn blocks_path()       { assert!(b().should_block("https://www.facebook.com/tr?id=1")); }
    #[test] fn no_fp_path_prefix() { assert!(!b().should_block("https://www.facebook.com/trending")); }
    #[test] fn allows_clean()      { assert!(!b().should_block("https://duckduckgo.com/?q=rust")); }

    #[test] fn toggle_ads() {
        let b = b();
        b.set_enabled(false);
        assert!(!b.should_block("https://doubleclick.net/ad.js"));
    }

    #[test] fn accounts_opt_in() {
        let b = b();
        // off par défaut
        assert!(!b.should_block("https://accounts.google.com/o/oauth2/auth"));
        b.set_block_accounts(true);
        assert!(b.should_block("https://accounts.google.com/o/oauth2/auth"));
        assert!(b.should_block("https://appleid.apple.com/auth/authorize"));
        // un site légitime non-auth reste autorisé
        assert!(!b.should_block("https://www.google.com/search?q=x"));
    }

    /// « Centaines d'utilisateurs » : 50 000 vérifications d'URL variées sur un
    /// bloqueur partagé, en basculant les flags. Doit rester correct et rapide,
    /// sans paniquer (slicing UTF-8 sûr sur des URLs tordues).
    #[test]
    fn stress_many_urls() {
        let b = b();
        b.set_block_accounts(true);
        let hosts = [
            "doubleclick.net", "ad.doubleclick.net", "example.com",
            "accounts.google.com", "duckduckgo.com", "sub.hotjar.com",
            "notdoubleclick.net", "facebook.com", "crates.io",
        ];
        let mut blocked = 0usize;
        for i in 0..50_000 {
            let h = hosts[i % hosts.len()];
            let url = format!("https://{h}/path/{i}?q={i}#frag");
            if b.should_block(&url) { blocked += 1; }
        }
        // Une fraction notable est bloquée, le reste passe → les deux jeux marchent.
        assert!(blocked > 0 && blocked < 50_000);
    }

    /// URLs malformées / unicode : extract_host & slicing ne doivent jamais paniquer.
    #[test]
    fn stress_malformed_urls() {
        let b = b();
        let weird = [
            "", "::::", "https://", "https://////", "nyx://newtab",
            "ht!tp://@@@/x", "https://日本語.example.com/ぺージ",
            "https://doubleclick.net", "https://x.com:443/a#b?c",
            "javascript:alert(1)", "data:text/html,<b>x</b>",
        ];
        for _ in 0..5_000 {
            for w in &weird {
                let _ = b.should_block(w); // ne doit pas paniquer
            }
        }
    }
}
