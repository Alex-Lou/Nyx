//! Parsing d'URL — **source unique** pour tout nyx-core et le browser.
//!
//! Avant : six copies divergentes (`extract_host`, `host_of`, `origin_of`,
//! `canonical_origin`, `is_localhost`, `is_trustworthy`…), chacune avec sa
//! faille (userinfo `user@evil` non retiré, autorité coupée au seul `/` donc
//! `evil.com?x@bank.com` → `bank.com`, `localhost.evil.com` pris pour local,
//! casse non normalisée). Ici, une seule implémentation, testée en hostile.
//!
//! Règles (RFC 3986 simplifiée, suffisante pour nos politiques) :
//!   1. l'autorité finit au PREMIER `/`, `?` ou `#` après `://` ;
//!   2. l'userinfo est tout ce qui précède le DERNIER `@` de l'autorité ;
//!   3. host IPv6 `[…]` gardé entre crochets jusqu'au split du port ;
//!   4. host normalisé : minuscules, sans point final, sans crochets.

use std::borrow::Cow;
use std::net::IpAddr;

/// Scheme en minuscules (`HTTPS://x` → `https`). `None` si pas de `://`.
pub fn scheme_of(url: &str) -> Option<String> {
    let s = url.trim();
    let idx = s.find("://")?;
    let scheme = &s[..idx];
    if scheme.is_empty() || !scheme.bytes().all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b)) {
        return None;
    }
    Some(scheme.to_ascii_lowercase())
}

/// Autorité brute (`user@host:port`), bornée au premier `/?#`.
fn authority(url: &str) -> Option<&str> {
    let s = url.trim();
    let rest = &s[s.find("://")? + 3..];
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    Some(&rest[..end])
}

/// Sépare `host` et `port` d'une autorité, userinfo retiré.
/// Le host IPv6 est rendu SANS crochets.
fn host_port(authority: &str) -> (&str, Option<&str>) {
    let hp = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    if let Some(v6) = hp.strip_prefix('[') {
        let Some(close) = v6.find(']') else { return (v6, None) };
        let port = v6[close + 1..].strip_prefix(':').filter(|p| !p.is_empty());
        return (&v6[..close], port);
    }
    match hp.rsplit_once(':') {
        Some((h, p)) => (h, Some(p).filter(|p| !p.is_empty())),
        None => (hp, None),
    }
}

fn normalize(host: &str) -> Cow<'_, str> {
    let h = host.trim_end_matches('.');
    if h.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Owned(h.to_ascii_lowercase())
    } else {
        Cow::Borrowed(h)
    }
}

/// Host normalisé d'une URL hiérarchique. `None` si absent.
///
/// `https://User:pw@WWW.Bank.com.:443/p?q=x@evil#f` → `www.bank.com`.
pub fn host_of(url: &str) -> Option<Cow<'_, str>> {
    let (host, _) = host_port(authority(url)?);
    let host = normalize(host);
    (!host.is_empty()).then_some(host)
}

/// Origine canonique RFC 6454 pour `http(s)` : `scheme://host[:port]`,
/// port par défaut retiré, host normalisé, IPv6 re-crocheté.
/// `None` pour tout autre scheme (pas d'origine tuple exploitable).
pub fn origin_of(url: &str) -> Option<String> {
    let scheme = scheme_of(url)?;
    let default_port = match scheme.as_str() {
        "https" => "443",
        "http" => "80",
        _ => return None,
    };
    let (host, port) = host_port(authority(url)?);
    let host = normalize(host);
    if host.is_empty() {
        return None;
    }
    let host = if host.contains(':') { format!("[{host}]") } else { host.into_owned() };
    Some(match port {
        Some(p) if p != default_port => format!("{scheme}://{host}:{p}"),
        _ => format!("{scheme}://{host}"),
    })
}

/// `true` pour un host de bouclage EXACT : `localhost`, `*.localhost`,
/// `127.0.0.0/8`, `::1`. `localhost.evil.com` et `127.evil.com` → `false`.
/// Attend un host déjà normalisé (sortie de [`host_of`]).
pub fn is_loopback_host(host: &str) -> bool {
    host == "localhost"
        || host.ends_with(".localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// `true` si l'URL pointe sur un host de bouclage exact.
pub fn is_loopback_url(url: &str) -> bool {
    host_of(url).is_some_and(|h| is_loopback_host(&h))
}

/// `true` si le scheme est `http` (insensible à la casse).
pub fn is_http(url: &str) -> bool {
    scheme_of(url).as_deref() == Some("http")
}

/// Contexte « potentiellement digne de confiance » (W3C Secure Contexts) :
/// `https` avec host, ou `http` sur bouclage exact.
pub fn is_potentially_trustworthy(url: &str) -> bool {
    match (scheme_of(url).as_deref(), host_of(url)) {
        (Some("https"), Some(_)) => true,
        (Some("http"), Some(h)) => is_loopback_host(&h),
        _ => false,
    }
}

/// URL qu'on accepte de stocker puis de recharger depuis une page interne
/// (accueil, favoris) : `http`, `https` ou `nyx` uniquement — jamais
/// `javascript:`, `data:`, `file:`…
pub fn is_navigable(url: &str) -> bool {
    matches!(scheme_of(url).as_deref(), Some("http" | "https" | "nyx"))
}

/// `host == domain` ou sous-domaine de `domain` (frontière de label :
/// `notdoubleclick.net` ne matche pas `doubleclick.net`).
pub fn host_matches(host: &str, domain: &str) -> bool {
    host.strip_suffix(domain)
        .is_some_and(|rest| rest.is_empty() || rest.ends_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_strips_userinfo_port_case_and_dot() {
        assert_eq!(host_of("https://User:pw@WWW.Bank.com.:443/p").as_deref(), Some("www.bank.com"));
    }

    #[test]
    fn authority_ends_at_query_and_fragment() {
        // Faille historique : coupe au seul '/' → host = bank.com.
        assert_eq!(host_of("https://evil.com?x@bank.com").as_deref(), Some("evil.com"));
        assert_eq!(host_of("https://evil.com#@bank.com").as_deref(), Some("evil.com"));
        assert_eq!(host_of("https://evil.com/?u=x@bank.com").as_deref(), Some("evil.com"));
    }

    #[test]
    fn ipv6_hosts() {
        assert_eq!(host_of("http://[::1]:8080/").as_deref(), Some("::1"));
        assert_eq!(origin_of("http://[::1]:8080/x").as_deref(), Some("http://[::1]:8080"));
        assert_eq!(origin_of("https://[2001:db8::1]/").as_deref(), Some("https://[2001:db8::1]"));
    }

    #[test]
    fn origin_canonical() {
        assert_eq!(origin_of("HTTPS://Sub.Example.com:443/a?b").as_deref(), Some("https://sub.example.com"));
        assert_eq!(origin_of("http://example.com:8080/").as_deref(), Some("http://example.com:8080"));
        assert_eq!(origin_of("https://u:p@evil.com?x@bank.com").as_deref(), Some("https://evil.com"));
        assert_eq!(origin_of("file:///etc/passwd"), None);
        assert_eq!(origin_of("https:///nohost"), None);
    }

    #[test]
    fn loopback_is_exact() {
        for ok in ["localhost", "a.localhost", "127.0.0.1", "127.9.9.9", "::1"] {
            assert!(is_loopback_host(ok), "{ok}");
        }
        for bad in ["localhost.evil.com", "127.evil.com", "evil-localhost", "128.0.0.1"] {
            assert!(!is_loopback_host(bad), "{bad}");
        }
    }

    #[test]
    fn trustworthy_contexts() {
        assert!(is_potentially_trustworthy("https://x.com/"));
        assert!(is_potentially_trustworthy("http://localhost:3000/"));
        assert!(!is_potentially_trustworthy("http://localhost.evil.com/"));
        assert!(!is_potentially_trustworthy("http://x.com/"));
        assert!(!is_potentially_trustworthy("file:///x"));
    }

    #[test]
    fn host_matching_respects_labels() {
        assert!(host_matches("ads.doubleclick.net", "doubleclick.net"));
        assert!(host_matches("doubleclick.net", "doubleclick.net"));
        assert!(!host_matches("notdoubleclick.net", "doubleclick.net"));
        assert!(!host_matches("doubleclick.net.evil.com", "doubleclick.net"));
    }

    #[test]
    fn navigable_schemes_only() {
        for ok in ["https://x.com", "HTTP://x", "nyx://newtab"] { assert!(is_navigable(ok), "{ok}"); }
        for bad in ["javascript:alert(1)", "data:text/html,x", "file:///etc/passwd", "", "x"] {
            assert!(!is_navigable(bad), "{bad}");
        }
    }

    #[test]
    fn hostile_inputs_never_panic() {
        for s in ["", "::::", "https://", "https://@", "https://[", "https://[::1", "日本://x",
                  "https://日本語.example/ぺ", "http://a@b@c:d:e/", "https://:443", "x://"] {
            let _ = (host_of(s), origin_of(s), scheme_of(s), is_potentially_trustworthy(s));
        }
    }
}
