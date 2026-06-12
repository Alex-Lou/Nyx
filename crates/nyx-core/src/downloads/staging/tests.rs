//! Tests staging — temp path build avec random déterministe + hostiles + stress.

use std::path::Path;
use super::*;

fn root() -> &'static Path { Path::new("/tmp/nyx") }

fn fixed_bytes(b: u8) -> [u8; RANDOM_BYTES] { [b; RANDOM_BYTES] }

// ─── Nominal ───────────────────────────────────────────────────────────────

#[test]
fn build_produces_hex_underscore_name() {
    let p = build_temp_path(root(), &fixed_bytes(0x00), "photo.jpg");
    let name = p.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(name, "00000000000000000000000000000000_photo.jpg");
}

#[test]
fn build_uses_full_hex_for_random_bytes() {
    let mut b = [0u8; RANDOM_BYTES];
    for (i, slot) in b.iter_mut().enumerate() { *slot = i as u8; }
    let p = build_temp_path(root(), &b, "x");
    let name = p.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("000102030405060708090a0b0c0d0e0f_"),
        "got {name}");
}

#[test]
fn build_lowercases_hex() {
    let p = build_temp_path(root(), &[0xff; RANDOM_BYTES], "x");
    let name = p.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("ffffffffffffffffffffffffffffffff_"),
        "expected lowercase hex, got {name}");
}

#[test]
fn build_is_inside_temp_root() {
    let p = build_temp_path(root(), &fixed_bytes(0x42), "f.pdf");
    assert!(p.starts_with(root()));
}

// ─── Empty / fallback ──────────────────────────────────────────────────────

#[test]
fn build_substitutes_download_for_empty_name() {
    let p = build_temp_path(root(), &fixed_bytes(0x01), "");
    let name = p.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.ends_with("_download"), "got {name}");
}

#[test]
fn build_substitutes_for_whitespace_only_name() {
    let p = build_temp_path(root(), &fixed_bytes(0x01), "   \t  ");
    let name = p.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.ends_with("_download"));
}

#[test]
fn build_substitutes_for_control_only_name() {
    let p = build_temp_path(root(), &fixed_bytes(0x01), "\0\n\r\x07");
    let name = p.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.ends_with("_download"));
}

// ─── Sanitize ──────────────────────────────────────────────────────────────

#[test]
fn build_strips_slashes() {
    let p = build_temp_path(root(), &fixed_bytes(0x02), "evil/../etc/passwd");
    let name = p.file_name().unwrap().to_string_lossy().into_owned();
    assert!(!name.contains('/'));
    assert!(!name.contains('\\'));
    // Le résultat doit rester dans temp_root (la composante est plate).
    assert_eq!(p.parent().unwrap(), root());
}

#[test]
fn build_strips_backslashes() {
    let p = build_temp_path(root(), &fixed_bytes(0x03), "C:\\Windows\\evil.exe");
    let name = p.file_name().unwrap().to_string_lossy().into_owned();
    assert!(!name.contains('\\'));
}

#[test]
fn build_strips_nul_bytes() {
    let p = build_temp_path(root(), &fixed_bytes(0x04), "evil\0.exe");
    let name = p.file_name().unwrap().to_string_lossy().into_owned();
    assert!(!name.contains('\0'));
}

#[test]
fn build_strips_control_chars() {
    let p = build_temp_path(root(), &fixed_bytes(0x05), "evil\n\r\x07.exe");
    let name = p.file_name().unwrap().to_string_lossy().into_owned();
    assert!(!name.contains('\n'));
    assert!(!name.contains('\r'));
    assert!(!name.contains('\x07'));
}

#[test]
fn build_truncates_overly_long_name() {
    let long = "a".repeat(500);
    let p = build_temp_path(root(), &fixed_bytes(0x06), &long);
    let name = p.file_name().unwrap().to_string_lossy().into_owned();
    // 32 hex + '_' + ≤200 chars = ≤233
    assert!(name.len() <= 233, "name too long: {} chars", name.len());
}

// ─── Determinism ───────────────────────────────────────────────────────────

#[test]
fn build_is_deterministic_for_same_inputs() {
    let a = build_temp_path(root(), &fixed_bytes(0x77), "x.pdf");
    let b = build_temp_path(root(), &fixed_bytes(0x77), "x.pdf");
    assert_eq!(a, b);
}

#[test]
fn build_differs_when_random_differs() {
    let a = build_temp_path(root(), &fixed_bytes(0x01), "x.pdf");
    let b = build_temp_path(root(), &fixed_bytes(0x02), "x.pdf");
    assert_ne!(a, b);
}

// ─── fresh_random_bytes ───────────────────────────────────────────────────

#[test]
fn fresh_random_returns_16_bytes() {
    let bytes = fresh_random_bytes();
    assert_eq!(bytes.len(), RANDOM_BYTES);
}

#[test]
fn fresh_random_not_all_zeros() {
    // Probabilité d'avoir 16 octets nuls par hasard ≈ 1/2^128.
    let bytes = fresh_random_bytes();
    assert!(bytes.iter().any(|&b| b != 0),
        "CSPRNG returned all zeros — suspicious");
}

#[test]
fn fresh_random_two_calls_differ() {
    // Collision sur 2^128 → astronomique.
    let a = fresh_random_bytes();
    let b = fresh_random_bytes();
    assert_ne!(a, b, "two CSPRNG calls returned identical bytes");
}

// ─── Stress ────────────────────────────────────────────────────────────────

/// 10 000 paths construits — pas de panic, format invariant, déterminisme.
#[test]
fn stress_10k_build() {
    let names = ["a.pdf", "photo.jpg", "x", "", "../../evil",
                 "long_long_name.bin", "\0\n", "日本.png"];
    for i in 0..10_000usize {
        let mut bytes = [0u8; RANDOM_BYTES];
        for (j, slot) in bytes.iter_mut().enumerate() {
            *slot = ((i + j) % 256) as u8;
        }
        let n = names[i % names.len()];
        let p = build_temp_path(root(), &bytes, n);
        // Toujours sous temp_root.
        assert!(p.starts_with(root()));
        // Préfixe hex (32 chars) + `_`.
        let name = p.file_name().unwrap().to_string_lossy();
        assert!(name.chars().nth(32) == Some('_'),
            "missing underscore at pos 32: {name}");
    }
}

/// 5 000 appels `fresh_random_bytes` — pas de panic, pas de doublons
/// dans une fenêtre raisonnable.
#[test]
fn stress_5k_fresh_random_unique() {
    let mut seen = std::collections::HashSet::new();
    for _ in 0..5_000 {
        let b = fresh_random_bytes();
        assert!(seen.insert(b), "CSPRNG produced duplicate in 5k window");
    }
}
