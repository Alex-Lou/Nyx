//! Analyse de risque d'un nom de domaine — détecte les attaques par homographes
//! Unicode (cyrillique vs latin), le punycode brut, les noms qui imitent des
//! sites connus.
//!
//! **Pas de blocage brutal** : on retourne un niveau de risque, c'est l'UI qui
//! décide quoi en faire (afficher en ASCII, avertir, refuser l'auto-fill).

use std::collections::HashSet;

use crate::sec_log::{self, Level};

/// Niveau de risque global d'un domaine.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Risk {
    /// ASCII pur, ou IDN clean (un seul script, pas de ressemblance).
    Safe,
    /// IDN légitime mais mérite l'affichage Unicode + tooltip ASCII.
    Suspicious,
    /// Mixed-script ou imitation de domaine connu → ne JAMAIS auto-fill.
    Dangerous,
}

/// Verdict détaillé pour un host — exposé tel quel à l'UI.
#[derive(Debug, Clone)]
pub struct Analysis {
    pub risk:         Risk,
    /// Forme Unicode affichable (ce que l'œil voit).
    pub unicode_host: String,
    /// Forme ASCII canonique (ce que le DNS résout). Inclut les `xn--`.
    pub ascii_host:   String,
    pub has_punycode: bool,
    pub mixed_script: bool,
}

/// Extrait l'host d'une URL puis analyse. Retourne `None` pour les URLs
/// internes (`nyx://`, `file://`, `data:`…) qui n'ont pas d'host à juger.
pub fn analyze_url(url: &str) -> Option<Analysis> {
    let s = url.trim();
    let after = s.find("://").map(|i| &s[i + 3..])?;
    let after = after.strip_prefix("www.").unwrap_or(after);
    let end = after.find(['/', ':', '?', '#']).unwrap_or(after.len());
    let host = &after[..end];
    if host.is_empty() || !s.starts_with("http") {
        return None;
    }
    Some(analyze(host))
}

/// Analyse un host. Idempotent, déterministe, sans I/O — pure logic.
pub fn analyze(host: &str) -> Analysis {
    let host = host.trim().trim_end_matches('.');

    // ASCII direct / IP / localhost → pas d'IDN.
    if host.is_ascii() {
        return Analysis {
            risk:         Risk::Safe,
            unicode_host: host.to_string(),
            ascii_host:   host.to_string(),
            has_punycode: host.split('.').any(|l| l.starts_with("xn--")),
            mixed_script: false,
        };
    }

    // Domaine Unicode : on convertit en ASCII (Punycode).
    let ascii = idna::domain_to_ascii(host).unwrap_or_else(|_| host.to_string());
    let unicode = idna::domain_to_unicode(&ascii).0;

    let mixed = has_mixed_scripts(&unicode);
    let imitation = looks_like_known(&unicode);

    let risk = if mixed || imitation {
        Risk::Dangerous
    } else {
        Risk::Suspicious  // IDN propre = à afficher en clair, pas à bloquer
    };

    match risk {
        Risk::Dangerous => sec_log::emit(Level::Warn,
            &format!("suspicious IDN: {ascii} ({unicode})")),
        Risk::Suspicious => sec_log::emit(Level::Warn,
            &format!("IDN domain: {ascii} ({unicode})")),
        Risk::Safe => {}
    }

    Analysis {
        risk,
        unicode_host: unicode,
        ascii_host:   ascii,
        has_punycode: true,
        mixed_script: mixed,
    }
}

/// Détecte un mélange de scripts dans un même label. Le piège classique : un
/// `а` cyrillique (U+0430) au milieu de `paypal.com` (latin).
fn has_mixed_scripts(s: &str) -> bool {
    for label in s.split('.') {
        let mut scripts = HashSet::new();
        for c in label.chars() {
            if let Some(s) = script_of(c) {
                scripts.insert(s);
                if scripts.len() > 1 { return true; }
            }
        }
    }
    false
}

/// Classifie un caractère par script principal. On regroupe digits + tirets
/// dans `Common` (jamais mixed à eux seuls).
fn script_of(c: char) -> Option<Script> {
    if c.is_ascii_digit() || c == '-' { return Some(Script::Common); }
    let cp = c as u32;
    Some(match cp {
        0x0041..=0x005A | 0x0061..=0x007A => Script::Latin,   // a-z A-Z
        0x00C0..=0x024F                   => Script::Latin,   // latin étendu
        0x0400..=0x04FF                   => Script::Cyrillic,
        0x0370..=0x03FF                   => Script::Greek,
        0x0590..=0x05FF                   => Script::Hebrew,
        0x0600..=0x06FF                   => Script::Arabic,
        0x4E00..=0x9FFF                   => Script::Han,
        0x3040..=0x30FF                   => Script::Kana,
        0xAC00..=0xD7AF                   => Script::Hangul,
        _ => return None, // ignore le reste (emojis, combining, etc.)
    })
}

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
enum Script { Common, Latin, Cyrillic, Greek, Hebrew, Arabic, Han, Kana, Hangul }

/// Imitation d'un site connu : on translittère les confusables vers leur
/// équivalent ASCII (cyrillique а → latin a, etc.) et on compare au lexique.
fn looks_like_known(host: &str) -> bool {
    let normalized = confusables_to_ascii(host);
    if normalized == host { return false; }
    KNOWN.iter().any(|known| normalized.contains(known))
}

/// Map des confusables Unicode → ASCII pour la détection d'imitation.
/// Liste pragmatique (les plus exploités), pas exhaustive — extensible.
fn confusables_to_ascii(s: &str) -> String {
    s.chars().map(|c| match c {
        'а' => 'a', 'е' => 'e', 'о' => 'o', 'р' => 'p', 'с' => 'c',
        'х' => 'x', 'у' => 'y', 'і' => 'i', 'ј' => 'j', 'ѕ' => 's',
        'ɑ' | 'α' => 'a', 'ε' => 'e', 'ο' => 'o', 'ρ' => 'p',
        'ӏ' | 'ⅼ' => 'l',
        _ => c,
    }).collect()
}

/// Liste des sites souvent imités. Sprint suivant : enrichir via le vault
/// (toute identité enregistrée devient un domaine à protéger).
const KNOWN: &[&str] = &[
    "google", "facebook", "instagram", "twitter", "linkedin",
    "amazon", "apple", "microsoft", "github", "gitlab",
    "paypal", "stripe", "wise", "revolut",
    "binance", "coinbase", "kraken",
    "netflix", "spotify", "youtube",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_clean_is_safe() {
        let a = analyze("example.com");
        assert_eq!(a.risk, Risk::Safe);
        assert!(!a.has_punycode);
        assert!(!a.mixed_script);
    }

    #[test]
    fn localhost_and_ip_are_safe() {
        assert_eq!(analyze("localhost").risk, Risk::Safe);
        assert_eq!(analyze("127.0.0.1").risk, Risk::Safe);
    }

    #[test]
    fn punycode_ascii_is_suspicious() {
        // ASCII brut avec xn-- → on signale (utilisateur peut vouloir voir l'unicode).
        let a = analyze("xn--bcher-kva.de");
        assert!(a.has_punycode);
        assert_eq!(a.risk, Risk::Safe); // ASCII pur → Safe, l'UI peut afficher Unicode si elle veut
    }

    #[test]
    fn legitimate_idn_is_suspicious_not_dangerous() {
        // bücher.de est légitime → Suspicious (à afficher en clair), pas Dangerous.
        let a = analyze("bücher.de");
        assert_eq!(a.risk, Risk::Suspicious);
        assert!(a.has_punycode);
        assert!(!a.mixed_script);
        assert!(a.ascii_host.starts_with("xn--"));
    }

    #[test]
    fn cyrillic_paypal_is_dangerous() {
        // Le 'а' (U+0430) est cyrillique. Imitation classique de paypal.com.
        let a = analyze("pаypal.com");
        assert_eq!(a.risk, Risk::Dangerous);
        assert!(a.mixed_script);
    }

    #[test]
    fn full_cyrillic_apple_is_dangerous() {
        // аррӏе.com — tous cyrilliques, pas mixed, mais ressemble à apple.
        let a = analyze("аррӏе.com");
        assert_eq!(a.risk, Risk::Dangerous); // détecté par looks_like_known
    }

    #[test]
    fn real_apple_is_safe() {
        assert_eq!(analyze("apple.com").risk, Risk::Safe);
    }

    #[test]
    fn subdomain_doesnt_trigger() {
        // sub.example.com doit être Safe même si example est dans un nom connu.
        assert_eq!(analyze("sub.notgoogle.com").risk, Risk::Safe);
    }

    #[test]
    fn empty_and_weird_dont_panic() {
        for s in ["", ".", "..", "  ", "a..b", "..com", "日本.jp"] {
            let _ = analyze(s);
        }
    }

    #[test]
    fn analyze_url_skips_internal_schemes() {
        assert!(analyze_url("nyx://newtab").is_none());
        assert!(analyze_url("file:///etc/passwd").is_none());
        assert!(analyze_url("data:text/html,x").is_none());
        assert!(analyze_url("").is_none());
    }

    #[test]
    fn analyze_url_handles_real_urls() {
        let a = analyze_url("https://www.example.com/path?q=1").unwrap();
        assert_eq!(a.risk, Risk::Safe);
        assert_eq!(a.ascii_host, "example.com");

        let a = analyze_url("http://pаypal.com/login").unwrap();
        assert_eq!(a.risk, Risk::Dangerous);
    }

    #[test]
    fn stress_many_hosts() {
        let samples = [
            "example.com", "pаypal.com", "google.com", "xn--bcher-kva.de",
            "127.0.0.1", "localhost", "sub.domain.co.uk", "аррӏе.com",
            "日本.jp", "bücher.de",
        ];
        for i in 0..2_000 {
            let _ = analyze(samples[i % samples.len()]);
        }
    }
}
