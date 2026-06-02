//! Résolution d'une entrée barre d'adresse en URL.

use crate::state::settings::AppSettings;

/// Normalise une entrée barre d'adresse en URL, via le moteur configuré.
pub fn resolve_input(input: &str, prefs: &AppSettings) -> String {
    let s = input.trim();
    if s.is_empty() {
        return String::new();
    }
    for prefix in ["https://", "http://", "file://", "nyx://"] {
        if s.starts_with(prefix) {
            return s.to_string();
        }
    }
    if let Some(colon) = s.find(':') {
        let after = &s[colon + 1..];
        if !after.starts_with(|c: char| c.is_ascii_digit()) && !after.starts_with("//") {
            return prefs.search_engine.search_url(s);
        }
    }
    if !s.contains(' ') && (s.contains('.') || s.starts_with("localhost")) {
        let scheme = if s.starts_with("localhost") { "http" } else { "https" };
        return format!("{scheme}://{s}");
    }
    prefs.search_engine.search_url(s)
}

#[cfg(test)]
mod tests {
    use super::resolve_input;
    use crate::state::settings::AppSettings;
    fn p() -> AppSettings { AppSettings::default() }

    #[test] fn passthrough_https()   { assert_eq!(resolve_input("https://x.com", &p()), "https://x.com"); }
    #[test] fn passthrough_nyx()     { assert_eq!(resolve_input("nyx://newtab", &p()), "nyx://newtab"); }
    #[test] fn bare_domain()         { assert_eq!(resolve_input("github.com", &p()), "https://github.com"); }
    #[test] fn localhost()           { assert_eq!(resolve_input("localhost:3000", &p()), "http://localhost:3000"); }
    #[test] fn query_to_engine()     { assert!(resolve_input("rust async", &p()).contains("duckduckgo.com")); }
    #[test] fn empty()               { assert_eq!(resolve_input("  ", &p()), ""); }
    #[test] fn javascript_neutered() { assert!(resolve_input("javascript:x", &p()).contains("duckduckgo.com")); }
    #[test] fn data_neutered()       { assert!(resolve_input("data:text/html,x", &p()).contains("duckduckgo.com")); }

    #[test]
    fn stress_fuzz_inputs() {
        let prefs = p();
        let fragments = [
            "", " ", "a", "google.com", "rust async", "http://x", "https://y.z",
            "javascript:alert(1)", "data:text/html,x", "vbscript:x", "file:///etc",
            "nyx://settings", "localhost:8080", "192.168.0.1:3000", "日本:9000",
            "a b c d", "::", "ftp://h", "mailto:a@b.c", "a.b.c.d.e.f.g",
        ];
        for i in 0..10_000 {
            let s = fragments[i % fragments.len()];
            let out = resolve_input(s, &prefs);
            assert!(!out.starts_with("javascript:"));
            assert!(!out.starts_with("data:"));
            assert!(!out.starts_with("vbscript:"));
        }
    }
}
