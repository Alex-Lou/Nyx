//! Tests cookie_policy — nominal, hostile, modes, stress.
//!
//! Couvre : Secure/SameSite, 3rd-party trackers, public suffix, taille,
//! expiry, modes Normal/Shadow/Banking, panic-safety, stress 10 000.

use super::*;

fn attrs(name: &'static str) -> CookieAttrs<'static> {
    CookieAttrs {
        name,
        value_len: 32,
        domain: "example.com",
        path: "/",
        secure: true,
        http_only: true,
        same_site: Some(SameSite::Lax),
        expires_seconds: Some(3600),
    }
}

fn ctx(page: &'static str) -> Context<'static> {
    Context {
        page_url: page,
        request_url: page,
        mode: Mode::Normal,
        is_third_party: false,
    }
}

// ─── Cas nominal ───────────────────────────────────────────────────────────

#[test]
fn https_secure_samesite_lax_allows() {
    let r = evaluate(&attrs("sid"), &ctx("https://example.com/"));
    assert_eq!(r.decision, Decision::Allow);
    assert!(r.reasons.is_empty(), "{:?}", r.reasons);
}

#[test]
fn https_secure_samesite_strict_allows() {
    let mut a = attrs("sid");
    a.same_site = Some(SameSite::Strict);
    assert_eq!(evaluate(&a, &ctx("https://example.com/")).decision, Decision::Allow);
}

// ─── Secure flag ───────────────────────────────────────────────────────────

#[test]
fn https_no_secure_strips() {
    let mut a = attrs("sid");
    a.secure = false;
    let r = evaluate(&a, &ctx("https://example.com/"));
    assert_eq!(r.decision, Decision::Strip);
    assert!(r.reasons.contains(&Reason::MissingSecureOnHttps));
}

#[test]
fn http_page_claiming_secure_blocks() {
    let r = evaluate(&attrs("sid"), &ctx("http://example.com/"));
    assert_eq!(r.decision, Decision::Block);
    assert!(r.reasons.contains(&Reason::InsecureSecureClaim));
}

#[test]
fn http_localhost_does_not_flag_insecure_scheme() {
    let mut a = attrs("sid");
    a.secure = false;
    let r = evaluate(&a, &ctx("http://localhost:3000/"));
    assert!(!r.reasons.contains(&Reason::InsecureScheme));
    assert!(!r.reasons.contains(&Reason::MissingSecureOnHttps));
    // SameSite=Lax + pas Secure sur localhost http → Allow (signal MissingSameSite absent).
    assert_eq!(r.decision, Decision::Allow);
}

#[test]
fn http_non_local_flagged_signal_only() {
    let mut a = attrs("sid");
    a.secure = false;
    let r = evaluate(&a, &ctx("http://example.com/"));
    assert!(r.reasons.contains(&Reason::InsecureScheme));
    // Pas de MissingSecureOnHttps (page n'est pas HTTPS), pas de Strip de ce fait.
    assert_eq!(r.decision, Decision::Allow);
}

// ─── SameSite ──────────────────────────────────────────────────────────────

#[test]
fn samesite_none_without_secure_blocks() {
    let mut a = attrs("sid");
    a.same_site = Some(SameSite::None);
    a.secure = false;
    // Page HTTPS → on attend AUSSI MissingSecureOnHttps mais le Block domine.
    let r = evaluate(&a, &ctx("https://example.com/"));
    assert_eq!(r.decision, Decision::Block);
    assert!(r.reasons.contains(&Reason::ImplicitSameSiteNone));
}

#[test]
fn samesite_none_with_secure_allows() {
    let mut a = attrs("sid");
    a.same_site = Some(SameSite::None);
    assert_eq!(evaluate(&a, &ctx("https://example.com/")).decision, Decision::Allow);
}

#[test]
fn missing_samesite_is_signal_only() {
    let mut a = attrs("sid");
    a.same_site = None;
    let r = evaluate(&a, &ctx("https://example.com/"));
    assert!(r.reasons.contains(&Reason::MissingSameSite));
    assert_eq!(r.decision, Decision::Allow);
}

// ─── 3rd-party / tracker ───────────────────────────────────────────────────

#[test]
fn third_party_tracker_blocks() {
    let mut c = ctx("https://news.example.com/");
    c.request_url = "https://doubleclick.net/pixel.gif";
    c.is_third_party = true;
    let r = evaluate(&attrs("uid"), &c);
    assert_eq!(r.decision, Decision::Block);
    assert!(r.reasons.contains(&Reason::ThirdPartyTracker));
}

#[test]
fn third_party_tracker_subdomain_blocks() {
    let mut c = ctx("https://news.example.com/");
    c.request_url = "https://ads.doubleclick.net/x";
    c.is_third_party = true;
    let r = evaluate(&attrs("uid"), &c);
    assert!(r.reasons.contains(&Reason::ThirdPartyTracker));
}

#[test]
fn third_party_non_tracker_allows() {
    let mut c = ctx("https://news.example.com/");
    c.request_url = "https://cdn.example.org/img";
    c.is_third_party = true;
    let r = evaluate(&attrs("uid"), &c);
    assert_eq!(r.decision, Decision::Allow);
}

// ─── Public suffix ─────────────────────────────────────────────────────────

#[test]
fn bare_tld_domain_blocks() {
    let mut a = attrs("sid");
    a.domain = ".com";
    let r = evaluate(&a, &ctx("https://example.com/"));
    assert_eq!(r.decision, Decision::Block);
    assert!(r.reasons.contains(&Reason::PublicSuffixDomain));
}

#[test]
fn bare_etld_co_uk_blocks() {
    let mut a = attrs("sid");
    a.domain = ".co.uk";
    let r = evaluate(&a, &ctx("https://example.co.uk/"));
    assert_eq!(r.decision, Decision::Block);
}

#[test]
fn etld_plus_one_allowed() {
    let mut a = attrs("sid");
    a.domain = "example.co.uk";
    let r = evaluate(&a, &ctx("https://example.co.uk/"));
    assert_eq!(r.decision, Decision::Allow);
}

#[test]
fn tld_no_leading_dot_blocks() {
    let mut a = attrs("sid");
    a.domain = "com";
    let r = evaluate(&a, &ctx("https://example.com/"));
    assert_eq!(r.decision, Decision::Block);
}

// ─── Taille ────────────────────────────────────────────────────────────────

#[test]
fn value_at_4096_allowed() {
    let mut a = attrs("sid");
    a.value_len = 4096;
    assert_eq!(evaluate(&a, &ctx("https://example.com/")).decision, Decision::Allow);
}

#[test]
fn value_above_4096_blocked() {
    let mut a = attrs("sid");
    a.value_len = 4097;
    let r = evaluate(&a, &ctx("https://example.com/"));
    assert_eq!(r.decision, Decision::Block);
    assert!(r.reasons.contains(&Reason::OversizedValue));
}

#[test]
fn value_10k_blocked() {
    let mut a = attrs("sid");
    a.value_len = 10_000;
    assert_eq!(evaluate(&a, &ctx("https://example.com/")).decision, Decision::Block);
}

// ─── Expiry ────────────────────────────────────────────────────────────────

#[test]
fn expiry_400_days_allowed() {
    let mut a = attrs("sid");
    a.expires_seconds = Some(400 * 86_400);
    assert_eq!(evaluate(&a, &ctx("https://example.com/")).decision, Decision::Allow);
}

#[test]
fn expiry_500_days_strips() {
    let mut a = attrs("sid");
    a.expires_seconds = Some(500 * 86_400);
    let r = evaluate(&a, &ctx("https://example.com/"));
    assert_eq!(r.decision, Decision::Strip);
    assert!(r.reasons.contains(&Reason::OversizedExpiry));
}

#[test]
fn expiry_none_session_cookie_allowed() {
    let mut a = attrs("sid");
    a.expires_seconds = None;
    assert_eq!(evaluate(&a, &ctx("https://example.com/")).decision, Decision::Allow);
}

// ─── Modes ─────────────────────────────────────────────────────────────────

#[test]
fn shadow_mode_strips_otherwise_allowed_cookie() {
    let mut c = ctx("https://example.com/");
    c.mode = Mode::Shadow;
    let r = evaluate(&attrs("sid"), &c);
    assert_eq!(r.decision, Decision::Strip);
    assert!(r.reasons.contains(&Reason::ShadowMode));
}

#[test]
fn banking_mode_first_party_signal_only() {
    let mut c = ctx("https://bank.com/");
    c.mode = Mode::Banking;
    let r = evaluate(&attrs("sid"), &c);
    assert!(r.reasons.contains(&Reason::BankingMode));
    assert_eq!(r.decision, Decision::Allow);
}

#[test]
fn banking_mode_third_party_blocks() {
    let mut c = ctx("https://bank.com/");
    c.mode = Mode::Banking;
    c.request_url = "https://cdn.other.com/img";
    c.is_third_party = true;
    let r = evaluate(&attrs("uid"), &c);
    assert_eq!(r.decision, Decision::Block);
    assert!(r.reasons.contains(&Reason::BankingThirdParty));
}

#[test]
fn dev_mode_behaves_like_normal() {
    let mut c = ctx("https://example.com/");
    c.mode = Mode::Dev;
    let r = evaluate(&attrs("sid"), &c);
    assert_eq!(r.decision, Decision::Allow);
    assert!(!r.reasons.iter().any(|x|
        matches!(x, Reason::ShadowMode | Reason::BankingMode | Reason::BankingThirdParty)));
}

// ─── Synthèse : la plus restrictive l'emporte ──────────────────────────────

#[test]
fn strip_and_block_combine_to_block() {
    // HTTPS sans Secure (Strip) + Domaine bare TLD (Block) → Block.
    let mut a = attrs("sid");
    a.secure = false;
    a.domain = ".com";
    let r = evaluate(&a, &ctx("https://example.com/"));
    assert_eq!(r.decision, Decision::Block);
}

#[test]
fn many_signals_one_block_still_blocks() {
    let mut a = attrs("sid");
    a.secure = false;
    a.same_site = None;
    a.value_len = 10_000;
    let r = evaluate(&a, &ctx("https://example.com/"));
    assert_eq!(r.decision, Decision::Block);
    assert!(r.reasons.contains(&Reason::OversizedValue));
}

// ─── Sanity / name ─────────────────────────────────────────────────────────

#[test]
fn empty_name_blocks() {
    let r = evaluate(&attrs(""), &ctx("https://example.com/"));
    assert_eq!(r.decision, Decision::Block);
    assert!(r.reasons.contains(&Reason::EmptyName));
}

// ─── Panic test ────────────────────────────────────────────────────────────

#[test]
fn malformed_urls_dont_panic() {
    let urls = [
        "", "://", "http://", "https://", "https:///path",
        "ws://x", "javascript:alert(1)", "file:///etc/passwd",
        "https://日本.example.com/", "https://x:x@y:0/",
        "https://[::1]:65535/", "\0\n\t",
    ];
    let weird_domains = ["", ".", "..", "...", "\0", "日本.com", ".\0.", "-.-",
                          ".com", "com", "co.uk"];
    for u in &urls {
        for d in &weird_domains {
            let mut a = attrs("x");
            a.domain = d;
            let _ = evaluate(&a, &ctx(u));
        }
    }
}

#[test]
fn empty_attrs_dont_panic() {
    let a = CookieAttrs {
        name: "", value_len: 0, domain: "", path: "",
        secure: false, http_only: false, same_site: None,
        expires_seconds: None,
    };
    let c = Context {
        page_url: "", request_url: "", mode: Mode::Normal, is_third_party: false,
    };
    let _ = evaluate(&a, &c);
}

#[test]
fn nul_bytes_in_fields_no_panic() {
    let mut a = attrs("nul\0name");
    a.domain = "evil\0.com";
    a.path = "/p\0";
    let _ = evaluate(&a, &ctx("https://example.com/"));
}

// ─── Stress ────────────────────────────────────────────────────────────────

/// 10 000 évaluations variées. Pas de panic, distribution non-triviale.
#[test]
fn stress_10k_evaluations() {
    let pages = [
        "https://example.com/", "http://example.com/", "https://bank.com/",
        "http://localhost/", "https://news.com/",
    ];
    let requests = [
        "https://example.com/", "https://doubleclick.net/p",
        "https://cdn.example.org/", "https://ads.hotjar.com/",
    ];
    let domains = ["example.com", ".example.com", ".com", "co.uk", "bank.com"];
    let modes   = [Mode::Normal, Mode::Shadow, Mode::Banking, Mode::Dev];
    let samesites = [None, Some(SameSite::Lax), Some(SameSite::Strict), Some(SameSite::None)];

    let mut counts = [0usize; 3]; // Allow, Strip, Block
    for i in 0..10_000 {
        let a = CookieAttrs {
            name: "sid",
            value_len: (i % 6000),
            domain: domains[i % domains.len()],
            path: "/",
            secure: i % 2 == 0,
            http_only: true,
            same_site: samesites[i % samesites.len()],
            expires_seconds: Some((i as i64 % 800) * 86_400),
        };
        let c = Context {
            page_url:        pages[i % pages.len()],
            request_url:     requests[i % requests.len()],
            mode:            modes[i % modes.len()],
            is_third_party:  i % 3 == 0,
        };
        let r = evaluate(&a, &c);
        counts[rank(r.decision) as usize] += 1;
    }
    // Au moins une décision de chaque niveau dans 10 000 tirages variés.
    assert!(counts[0] > 0 && counts[1] > 0 && counts[2] > 0,
        "distribution Allow={}, Strip={}, Block={}", counts[0], counts[1], counts[2]);
}
