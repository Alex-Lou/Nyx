use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct AdBlocker {
    enabled: AtomicBool,
    domains: HashSet<String>,
    paths:   Vec<String>,
}

impl AdBlocker {
    pub fn new() -> Self {
        let (mut domains, mut paths) = (HashSet::new(), Vec::new());
        for rule in RULES {
            if rule.contains('/') { paths.push(rule.to_string()); }
            else                  { domains.insert(rule.to_string()); }
        }
        Self { enabled: AtomicBool::new(true), domains, paths }
    }

    pub fn set_enabled(&self, v: bool) { self.enabled.store(v, Ordering::Relaxed); }

    pub fn should_block(&self, url: &str) -> bool {
        if !self.enabled.load(Ordering::Relaxed) { return false; }
        let host = extract_host(url);
        self.domains.iter().any(|d| host == d || host.ends_with(&format!(".{d}")))
            || self.paths.iter().any(|p| url_contains_path(url, p))
    }
}

fn extract_host(url: &str) -> &str {
    let after = url.find("://").map(|i| &url[i + 3..]).unwrap_or(url);
    let end   = after.find(|c: char| c == '/' || c == ':' || c == '?' || c == '#')
                     .unwrap_or(after.len());
    &after[..end]
}

fn url_contains_path(url: &str, rule: &str) -> bool {
    url.find(rule).is_some_and(|pos| {
        let tail = &url[pos + rule.len()..];
        tail.is_empty() || matches!(tail.chars().next(), Some('?' | '#' | '/' | '&'))
    })
}

impl Default for AdBlocker { fn default() -> Self { Self::new() } }

const RULES: &[&str] = &[
    "doubleclick.net", "googlesyndication.com",
    "googletagmanager.com", "googletagservices.com",
    "ads.twitter.com", "facebook.com/tr",
    "analytics.google.com", "hotjar.com",
    "scorecardresearch.com", "quantserve.com",
];

#[cfg(test)]
mod tests {
    use super::*;
    fn b() -> AdBlocker { AdBlocker::new() }

    #[test] fn blocks_exact()        { assert!(b().should_block("https://doubleclick.net/ad.js")); }
    #[test] fn blocks_subdomain()    { assert!(b().should_block("https://ad.doubleclick.net/x")); }
    #[test] fn no_fp_similar()       { assert!(!b().should_block("https://notdoubleclick.net/")); }
    #[test] fn no_fp_injection()     { assert!(!b().should_block("https://doubleclick.net.evil.com/x")); }
    #[test] fn blocks_path()         { assert!(b().should_block("https://www.facebook.com/tr?id=1")); }
    #[test] fn no_fp_path_prefix()   { assert!(!b().should_block("https://www.facebook.com/trending")); }
    #[test] fn allows_clean()        { assert!(!b().should_block("https://duckduckgo.com/?q=rust")); }
    #[test] fn toggle_disable() {
        let b = b(); b.set_enabled(false);
        assert!(!b.should_block("https://doubleclick.net/ad.js"));
    }
}
