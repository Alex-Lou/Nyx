// Sprint 3 remplacera ce module par la crate `adblock = "0.9"` (Brave)
// avec EasyList + EasyPrivacy embarqués. Pour l'instant : règles en dur.

use std::collections::HashSet;

pub struct AdBlocker {
    domains: HashSet<String>, // match hostname exact + tous sous-domaines
    paths:   Vec<String>,     // match partiel dans l'URL complète (délimité)
}

impl AdBlocker {
    pub fn new() -> Self {
        let (mut domains, mut paths) = (HashSet::new(), Vec::new());
        for rule in RULES {
            if rule.contains('/') { paths.push(rule.to_string()); }
            else                  { domains.insert(rule.to_string()); }
        }
        Self { domains, paths }
    }

    pub fn should_block(&self, url: &str) -> bool {
        let host = extract_host(url);
        self.domains.iter().any(|d| host == d || host.ends_with(&format!(".{d}")))
            || self.paths.iter().any(|p| url_contains_path(url, p))
    }
}

/// Extrait le hostname — `"https://ads.x.com/img"` → `"ads.x.com"`.
fn extract_host(url: &str) -> &str {
    let after_scheme = url.find("://").map(|i| &url[i + 3..]).unwrap_or(url);
    let end = after_scheme
        .find(|c: char| c == '/' || c == ':' || c == '?' || c == '#')
        .unwrap_or(after_scheme.len());
    &after_scheme[..end]
}

/// Vérifie que la règle est présente dans l'URL et suivie d'un délimiteur,
/// pour éviter `"facebook.com/tr"` → faux positif sur `"facebook.com/trending"`.
fn url_contains_path(url: &str, rule: &str) -> bool {
    url.find(rule).is_some_and(|pos| {
        let tail = &url[pos + rule.len()..];
        tail.is_empty() || matches!(tail.chars().next(), Some('?' | '#' | '/' | '&'))
    })
}

impl Default for AdBlocker {
    fn default() -> Self { Self::new() }
}

const RULES: &[&str] = &[
    // Régies publicitaires
    "doubleclick.net",
    "googlesyndication.com",
    "googletagmanager.com",
    "googletagservices.com",
    // Trackers réseaux sociaux
    "ads.twitter.com",
    "facebook.com/tr",
    // Analytics
    "analytics.google.com",
    "hotjar.com",
    "scorecardresearch.com",
    "quantserve.com",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn b() -> AdBlocker { AdBlocker::new() }

    #[test] fn blocks_exact_domain() {
        assert!(b().should_block("https://doubleclick.net/ad.js"));
        assert!(b().should_block("https://hotjar.com/pixel.png"));
    }
    #[test] fn blocks_subdomain() {
        assert!(b().should_block("https://ad.doubleclick.net/pfadx/N1234"));
        assert!(b().should_block("https://cdn.googlesyndication.com/pagead/js/adsbygoogle.js"));
    }
    #[test] fn no_fp_similar_domain() {
        // "notdoubleclick.net" ne finit pas par ".doubleclick.net"
        assert!(!b().should_block("https://notdoubleclick.net/img.png"));
    }
    #[test] fn no_fp_subdomain_injection() {
        // Tentative d'injection via sous-domaine
        assert!(!b().should_block("https://doubleclick.net.evil.com/x"));
    }
    #[test] fn blocks_path_rule() {
        assert!(b().should_block("https://www.facebook.com/tr?id=123&ev=PageView"));
        assert!(b().should_block("https://www.facebook.com/tr/"));
    }
    #[test] fn no_fp_path_prefix() {
        assert!(!b().should_block("https://www.facebook.com/trending/news"));
    }
    #[test] fn allows_clean_sites() {
        assert!(!b().should_block("https://duckduckgo.com/?q=rust"));
        assert!(!b().should_block("https://crates.io/crates/gtk"));
        assert!(!b().should_block("https://doc.rust-lang.org/std/"));
    }
}
