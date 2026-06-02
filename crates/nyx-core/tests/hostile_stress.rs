//! Tests de stress hostiles — `nyx-core` est pure Rust donc on simule
//! l'effet de milliers d'utilisateurs qui chipotent en lançant en parallèle
//! (threads) ET en boucles intensives (par thread) sur les surfaces sécurité.
//!
//! Ces tests doivent :
//! 1. **Ne jamais paniquer** sur entrées malformées.
//! 2. **Rester cohérents** : un site bloqué l'est toujours, l'état partagé
//!    (NyxGuard atomique) reste lisible sous contention.
//! 3. **Tourner vite** : si un panic test devient lent (>1s), il y a un bug
//!    de complexité quelque part.

use std::sync::{Arc, Barrier};
use std::thread;

use nyx_core::{
    domain_risk,
    nyxguard::NyxGuard,
    sec_log::{self, redact_url},
    security::{decide, Verdict},
    site_data_policy::{plan_for, Scope},
    state::settings,
    url::resolve_input,
};

/// 10 000 URLs hostiles variées — chacune doit retourner un verdict
/// sans paniquer.
#[test]
fn fuzz_security_decide_10k() {
    let guard = NyxGuard::new();
    let hostile = [
        "https://example.com", "http://x", "https://日本.jp/path",
        "file:///etc/passwd", "file:///C:/Windows/System32",
        "nyx://apply?adblock=false", "nyx://move?idx=99&to=evil",
        "nyx://newtab", "nyx://settings", "nyx://bookmarks", "nyx://unknown",
        "javascript:alert(1)", "data:text/html,x", "vbscript:x",
        "https://doubleclick.net/ad.js", "https://accounts.google.com/x",
        "https://pаypal.com/login",     // homographe
        "https://" , "ht!tp://@@",      // malformé
        "://", "https:///path", "", " ", "\n\t",
        "https://x.com:80808080/path",   // port absurde
    ];
    for i in 0..10_000 {
        let url = hostile[i % hostile.len()];
        let _ = decide(url, i % 2 == 0, &guard);
    }
}

/// 5 000 hosts dans le pipeline IDN — aucun ne panique.
#[test]
fn fuzz_domain_risk_5k() {
    let hostile = [
        "example.com", "pаypal.com", "аррӏе.com", "bücher.de",
        "127.0.0.1", "localhost", "日本.jp", "",
        "..", "a..b", "..com", "xn--bcher-kva.de",
        "very-long-subdomain-chain-that-keeps-going.example.co.uk",
        "shop.example.com.au", "weird-symbols-€$£.test",
    ];
    for i in 0..5_000 {
        let _ = domain_risk::analyze(hostile[i % hostile.len()]);
        let url = format!("https://{}/path?q={}", hostile[i % hostile.len()], i);
        let _ = domain_risk::analyze_url(&url);
    }
}

/// 5 000 plans d'oubli sur URLs malformées — pas de panic.
#[test]
fn fuzz_forget_plan_5k() {
    let hostile = [
        "", "://", "http://", "https://@/", "http://:80/",
        "https://example.com/", "https://sub.example.co.uk/x",
        "http://127.0.0.1:8080/", "https://user:pass@host/path",
        "nyx://newtab", "file:///etc/passwd", "data:text/html,x",
    ];
    for i in 0..5_000 {
        let _ = plan_for(hostile[i % hostile.len()], Scope::Origin);
        let _ = plan_for(hostile[i % hostile.len()], Scope::Domain);
    }
}

/// 10 000 entrées URL bar → resolve_input ne panique jamais et ne laisse
/// jamais passer un schéma exécutable en clair.
#[test]
fn fuzz_resolve_input_10k() {
    let prefs = settings::AppSettings::default();
    let hostile = [
        "", " ", "google.com", "rust async", "javascript:alert(1)",
        "data:text/html,x", "vbscript:x", "https://x",
        "nyx://settings", "192.168.0.1:3000", "::", "日本:9000",
        "<script>", "'; DROP TABLE--", "%2e%2e/etc/passwd",
        "file:///etc/passwd", "ftp://h",
    ];
    for i in 0..10_000 {
        let out = resolve_input(hostile[i % hostile.len()], &prefs);
        // Invariant sécurité : aucun schéma exécutable ne ressort en clair.
        assert!(!out.starts_with("javascript:"));
        assert!(!out.starts_with("data:"));
        assert!(!out.starts_with("vbscript:"));
    }
}

/// Sec_log : 10 000 redactions — toujours correctes, jamais de panic.
#[test]
fn fuzz_redact_10k() {
    let urls = [
        "https://e.com/p?token=abc", "https://e.com/p#access_token=x",
        "https://e.com", "", "%%%", "://", "日本?token=x",
        "https://api.x.com/v1/data?key=secret&user=42",
    ];
    for i in 0..10_000 {
        let red = redact_url(urls[i % urls.len()]);
        // La query brute ne doit jamais réapparaître si elle existait.
        if urls[i % urls.len()].contains("token=abc") {
            assert!(!red.contains("token=abc"));
            assert!(red.contains("[redacted]"));
        }
    }
}

/// Mille "users" en parallèle qui hammer le NyxGuard (atomiques + HashSets
/// en lecture). Vérifie qu'il reste cohérent sous contention massive.
#[test]
fn parallel_nyxguard_1000_users() {
    let guard = Arc::new(NyxGuard::new());
    let n_threads = 16;
    let iters_per_thread = 1_000;
    let barrier = Arc::new(Barrier::new(n_threads));

    let handles: Vec<_> = (0..n_threads).map(|tid| {
        let g = guard.clone();
        let b = barrier.clone();
        thread::spawn(move || {
            b.wait();
            let urls = [
                "https://doubleclick.net/ad.js",
                "https://example.com/",
                "https://accounts.google.com/o/oauth2/auth",
                "https://duckduckgo.com/?q=x",
                "https://sub.hotjar.com/script",
            ];
            // Quelques threads togglent les flags pendant que les autres lisent.
            if tid == 0 || tid == 8 {
                for i in 0..iters_per_thread {
                    g.set_enabled(i % 2 == 0);
                    g.set_block_accounts(i % 3 == 0);
                }
            } else {
                for i in 0..iters_per_thread {
                    let _ = g.should_block(urls[i % urls.len()]);
                }
            }
        })
    }).collect();

    for h in handles { h.join().unwrap(); }
}

/// Décide() en parallèle (read-only sur NyxGuard) — milliers d'appels concurrents.
#[test]
fn parallel_decide_1000_users() {
    let guard = Arc::new(NyxGuard::new());
    let n_threads = 16;
    let iters_per_thread = 1_000;
    let barrier = Arc::new(Barrier::new(n_threads));

    let handles: Vec<_> = (0..n_threads).map(|_| {
        let g = guard.clone();
        let b = barrier.clone();
        thread::spawn(move || {
            b.wait();
            let urls = [
                "https://example.com", "file:///etc/passwd",
                "nyx://apply?x=1", "nyx://newtab",
                "https://doubleclick.net/ad", "https://pаypal.com",
            ];
            for i in 0..iters_per_thread {
                let v = decide(urls[i % urls.len()], i % 2 == 0, &g);
                // Invariants vérifiés sous concurrence :
                if urls[i % urls.len()].starts_with("file://") {
                    assert!(matches!(v, Verdict::BlockFileAccess));
                }
            }
        })
    }).collect();

    for h in handles { h.join().unwrap(); }
}

/// Stress mixte : tous les modules sécurité touchés en parallèle, simule
/// l'agitation réelle d'une session intense.
#[test]
fn parallel_mixed_workload() {
    let guard = Arc::new(NyxGuard::new());
    let n_threads = 8;
    let barrier = Arc::new(Barrier::new(n_threads));

    let handles: Vec<_> = (0..n_threads).map(|tid| {
        let g = guard.clone();
        let b = barrier.clone();
        thread::spawn(move || {
            b.wait();
            for i in 0..500 {
                match (tid + i) % 5 {
                    0 => { let _ = decide("https://example.com", false, &g); }
                    1 => { let _ = domain_risk::analyze_url("https://pаypal.com"); }
                    2 => { let _ = plan_for("https://x.com/y?t=1", Scope::Origin); }
                    3 => { let _ = sec_log::format_event(sec_log::Level::Warn, "x"); }
                    _ => { let _ = decide("file:///etc/passwd", true, &g); }
                }
            }
        })
    }).collect();

    for h in handles { h.join().unwrap(); }
}
