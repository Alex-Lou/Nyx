//! Tests hsts_policy — header parsing, upsert/decide, preload, stress.

use super::*;

const NOW: i64 = 1_700_000_000; // ancrage stable pour tests

// ─── parse_sts_header ──────────────────────────────────────────────────────

#[test]
fn parse_max_age_only() {
    assert_eq!(parse_sts_header("max-age=3600"), Some((3600, false)));
}

#[test]
fn parse_max_age_and_subdomains() {
    assert_eq!(parse_sts_header("max-age=3600; includeSubDomains"),
               Some((3600, true)));
}

#[test]
fn parse_case_insensitive_directives() {
    assert_eq!(parse_sts_header("MAX-AGE=10; INCLUDESUBDOMAINS"),
               Some((10, true)));
}

#[test]
fn parse_quoted_value() {
    assert_eq!(parse_sts_header(r#"max-age="3600""#), Some((3600, false)));
}

#[test]
fn parse_whitespace_tolerant() {
    assert_eq!(parse_sts_header("  max-age = 7200 ;  includeSubDomains  "),
               Some((7200, true)));
}

#[test]
fn parse_ignores_unknown_directives() {
    assert_eq!(parse_sts_header("max-age=10; preload; foo=bar"),
               Some((10, false)));
}

#[test]
fn parse_missing_max_age_returns_none() {
    assert_eq!(parse_sts_header("includeSubDomains"), None);
    assert_eq!(parse_sts_header(""), None);
    assert_eq!(parse_sts_header(";;;"), None);
}

#[test]
fn parse_malformed_max_age_returns_none() {
    assert_eq!(parse_sts_header("max-age=abc"), None);
    assert_eq!(parse_sts_header("max-age=-1"),  None); // u64 parse fail
}

#[test]
fn parse_max_age_zero_preserved() {
    assert_eq!(parse_sts_header("max-age=0"), Some((0, false)));
}

// ─── upsert / decide ───────────────────────────────────────────────────────

#[test]
fn upsert_then_decide_upgrades() {
    let mut s = HstsStore::new();
    s.upsert_from_header("example.com", "max-age=3600", NOW);
    assert_eq!(s.decide("http://example.com/page", NOW), Decision::UpgradeToHttps);
}

#[test]
fn https_decide_always_passes() {
    let mut s = HstsStore::new();
    s.upsert_from_header("example.com", "max-age=3600", NOW);
    assert_eq!(s.decide("https://example.com/", NOW), Decision::Pass);
}

#[test]
fn max_age_zero_purges_entry() {
    let mut s = HstsStore::new();
    s.upsert_from_header("example.com", "max-age=3600", NOW);
    assert_eq!(s.len(), 1);
    s.upsert_from_header("example.com", "max-age=0", NOW);
    assert!(s.is_empty());
    assert_eq!(s.decide("http://example.com/", NOW), Decision::Pass);
}

#[test]
fn unknown_host_passes() {
    let s = HstsStore::new();
    assert_eq!(s.decide("http://random.tld/x", NOW), Decision::Pass);
}

#[test]
fn subdomains_only_match_with_flag() {
    let mut s = HstsStore::new();
    s.upsert_from_header("example.com", "max-age=3600; includeSubDomains", NOW);
    assert_eq!(s.decide("http://sub.example.com/", NOW), Decision::UpgradeToHttps);
    assert_eq!(s.decide("http://deep.sub.example.com/", NOW),
               Decision::UpgradeToHttps);
}

#[test]
fn subdomain_request_without_flag_passes() {
    let mut s = HstsStore::new();
    s.upsert_from_header("example.com", "max-age=3600", NOW);
    // Sans includeSubDomains, sub.example.com ne doit PAS être upgradé.
    assert_eq!(s.decide("http://sub.example.com/", NOW), Decision::Pass);
    // Mais example.com lui-même reste upgradé.
    assert_eq!(s.decide("http://example.com/", NOW), Decision::UpgradeToHttps);
}

#[test]
fn expired_entry_passes_and_can_be_purged() {
    let mut s = HstsStore::new();
    s.upsert_from_header("example.com", "max-age=10", NOW);
    assert_eq!(s.decide("http://example.com/", NOW + 100), Decision::Pass);
    s.purge_expired(NOW + 100);
    assert!(s.is_empty());
}

#[test]
fn max_age_clamped_to_two_years() {
    let mut s = HstsStore::new();
    // 10 ans demandés
    s.upsert_from_header("example.com", "max-age=315360000", NOW);
    // À +2 ans + 1 s, l'entrée doit déjà être expirée.
    let after_two_years = NOW + (2 * 365 * 86_400) + 1;
    assert_eq!(s.decide("http://example.com/", after_two_years), Decision::Pass);
}

#[test]
fn upsert_overwrites_previous_entry() {
    let mut s = HstsStore::new();
    s.upsert_from_header("example.com", "max-age=3600", NOW);
    s.upsert_from_header("example.com", "max-age=7200; includeSubDomains", NOW);
    assert_eq!(s.len(), 1);
    // includeSubDomains s'applique maintenant.
    assert_eq!(s.decide("http://sub.example.com/", NOW), Decision::UpgradeToHttps);
}

#[test]
fn host_normalized_case_insensitive() {
    let mut s = HstsStore::new();
    s.upsert_from_header("ExAmple.COM", "max-age=3600", NOW);
    assert_eq!(s.decide("http://EXAMPLE.com/", NOW), Decision::UpgradeToHttps);
}

#[test]
fn host_trailing_dot_normalized() {
    let mut s = HstsStore::new();
    s.upsert_from_header("example.com.", "max-age=3600", NOW);
    assert_eq!(s.decide("http://example.com/", NOW), Decision::UpgradeToHttps);
}

#[test]
fn empty_host_or_no_max_age_no_op() {
    let mut s = HstsStore::new();
    s.upsert_from_header("", "max-age=3600", NOW);
    s.upsert_from_header("x.com", "no-max-age", NOW);
    assert!(s.is_empty());
}

// ─── Non-HTTP schemes ──────────────────────────────────────────────────────

#[test]
fn file_scheme_passes() {
    let s = HstsStore::new();
    assert_eq!(s.decide("file:///etc/passwd", NOW), Decision::Pass);
}

#[test]
fn nyx_scheme_passes() {
    let s = HstsStore::new();
    assert_eq!(s.decide("nyx://newtab", NOW), Decision::Pass);
}

#[test]
fn data_scheme_passes() {
    let s = HstsStore::new();
    assert_eq!(s.decide("data:text/html,<b>x</b>", NOW), Decision::Pass);
}

#[test]
fn javascript_scheme_passes() {
    let s = HstsStore::new();
    assert_eq!(s.decide("javascript:alert(1)", NOW), Decision::Pass);
}

// ─── Preload list ──────────────────────────────────────────────────────────

#[test]
fn preload_google_upgrades_without_header() {
    let s = HstsStore::new();
    assert_eq!(s.decide("http://google.com/", NOW), Decision::UpgradeToHttps);
    assert_eq!(s.decide("http://mail.google.com/", NOW), Decision::UpgradeToHttps);
}

#[test]
fn preload_paypal_upgrades() {
    let s = HstsStore::new();
    assert_eq!(s.decide("http://paypal.com/login", NOW), Decision::UpgradeToHttps);
    assert_eq!(s.decide("http://www.paypal.com/login", NOW),
               Decision::UpgradeToHttps);
}

#[test]
fn preload_random_unrelated_passes() {
    let s = HstsStore::new();
    assert_eq!(s.decide("http://random-unknown-site.tld/", NOW), Decision::Pass);
}

#[test]
fn preload_injection_does_not_match() {
    // github.com.attacker.tld ne doit pas être traité comme github.com.
    let s = HstsStore::new();
    assert_eq!(s.decide("http://github.com.attacker.tld/", NOW), Decision::Pass);
}

// ─── URL parsing edge cases ────────────────────────────────────────────────

#[test]
fn url_with_port_handled() {
    let mut s = HstsStore::new();
    s.upsert_from_header("example.com", "max-age=3600", NOW);
    assert_eq!(s.decide("http://example.com:8080/path", NOW),
               Decision::UpgradeToHttps);
}

#[test]
fn url_with_userinfo_handled() {
    let mut s = HstsStore::new();
    s.upsert_from_header("example.com", "max-age=3600", NOW);
    assert_eq!(s.decide("http://user:pw@example.com/", NOW),
               Decision::UpgradeToHttps);
}

#[test]
fn url_with_ipv6_passes_through_parsing() {
    // Pas d'entrée HSTS sur IPv6 ici : on vérifie juste que parser ne paniquera pas.
    let s = HstsStore::new();
    let _ = s.decide("http://[::1]:8080/", NOW);
    let _ = s.decide("http://[2001:db8::1]/", NOW);
}

// ─── Panic safety ──────────────────────────────────────────────────────────

#[test]
fn malformed_urls_dont_panic() {
    let s = HstsStore::new();
    for u in &["", "://", "http://", "http:///", "://example.com",
               "https:///", "ftp://x", "h\0t\nt p:/x", "http://[::]/",
               "http://例え.com/", "http://x:abc/"]
    {
        let _ = s.decide(u, NOW);
    }
}

#[test]
fn weird_headers_dont_panic() {
    let mut s = HstsStore::new();
    for h in &["", ";", ";;;", "max-age=", "max-age", "=10",
               "max-age=99999999999999999999999999", "\0\n",
               "max-age=1; max-age=2; includeSubDomains"]
    {
        s.upsert_from_header("example.com", h, NOW);
    }
}

#[test]
fn parents_iteration_handles_edge_inputs() {
    assert_eq!(parents("").count(), 0);
    assert_eq!(parents("com").count(), 0); // no dot
    let p: Vec<_> = parents("a.b.c.com").collect();
    assert_eq!(p, vec!["b.c.com", "c.com", "com"]);
}

// ─── Stress ────────────────────────────────────────────────────────────────

#[test]
fn stress_10k_upsert_decide_alternation() {
    let mut s = HstsStore::new();
    let hosts = ["a.com", "b.com", "c.com", "d.com", "e.com",
                 "sub.a.com", "x.b.com", "deep.sub.c.com"];
    let headers = ["max-age=3600", "max-age=3600; includeSubDomains",
                   "max-age=0", "garbage", "max-age=99"];
    let urls = ["http://a.com/", "http://sub.a.com/", "http://b.com/x",
                "https://a.com/", "http://e.com/", "nyx://x",
                "http://github.com/", "http://random.tld/"];
    let mut upgrades = 0usize;
    let mut passes = 0usize;
    for i in 0..10_000 {
        let host = hosts[i % hosts.len()];
        let header = headers[i % headers.len()];
        s.upsert_from_header(host, header, NOW + i as i64);
        let dec = s.decide(urls[i % urls.len()], NOW + i as i64);
        match dec {
            Decision::UpgradeToHttps => upgrades += 1,
            Decision::Pass           => passes += 1,
            Decision::Block          => {}
        }
    }
    // Au moins quelques upgrades ET quelques passes — la séquence n'est pas
    // dégénérée et le store gère bien les deux paths.
    assert!(upgrades > 100, "trop peu d'upgrades: {upgrades}");
    assert!(passes > 100,   "trop peu de passes : {passes}");
}
