//! Tests SHA-256 — vecteurs NIST + équivalence streaming/one-shot + stress.

use super::*;

// ─── Vecteurs NIST FIPS 180-4 ──────────────────────────────────────────────

#[test]
fn empty_input_matches_nist() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    );
}

#[test]
fn abc_matches_nist() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    );
}

#[test]
fn long_56_chars_matches_nist() {
    assert_eq!(
        sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
    );
}

#[test]
fn million_a_matches_nist() {
    let data = vec![b'a'; 1_000_000];
    assert_eq!(
        sha256_hex(&data),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
    );
}

// ─── Streaming = one-shot ──────────────────────────────────────────────────

#[test]
fn streaming_equivalent_to_one_shot() {
    let payload = b"The quick brown fox jumps over the lazy dog";
    let one_shot = sha256_hex(payload);

    let mut h = Sha256Hasher::new();
    for chunk in payload.chunks(7) { h.update(chunk); }
    assert_eq!(h.finalize_hex(), one_shot);
}

#[test]
fn streaming_single_byte_chunks() {
    let payload = b"streaming test";
    let one_shot = sha256_hex(payload);

    let mut h = Sha256Hasher::new();
    for b in payload { h.update(std::slice::from_ref(b)); }
    assert_eq!(h.finalize_hex(), one_shot);
}

#[test]
fn streaming_zero_chunks_is_empty_hash() {
    let h = Sha256Hasher::new();
    assert_eq!(
        h.finalize_hex(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    );
}

// ─── Format de sortie ──────────────────────────────────────────────────────

#[test]
fn output_is_64_hex_chars_lowercase() {
    let hex = sha256_hex(b"anything");
    assert_eq!(hex.len(), 64);
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()
        && (c.is_ascii_digit() || c.is_ascii_lowercase())));
}

#[test]
fn default_constructor_equivalent_to_new() {
    let a = Sha256Hasher::new().finalize_hex();
    let b = Sha256Hasher::default().finalize_hex();
    assert_eq!(a, b);
}

// ─── Stress ────────────────────────────────────────────────────────────────

/// Hash 10 000 payloads variés — pas de panic, format constant, déterministe.
#[test]
fn stress_10k_payloads() {
    let mut seen = std::collections::HashSet::new();
    for i in 0..10_000usize {
        let payload = format!("payload-{i:08x}");
        let hex = sha256_hex(payload.as_bytes());
        assert_eq!(hex.len(), 64);
        // Confirme déterminisme (re-hash = même sortie).
        assert_eq!(hex, sha256_hex(payload.as_bytes()));
        seen.insert(hex);
    }
    // Aucune collision attendue sur 10k entrées uniques.
    assert_eq!(seen.len(), 10_000);
}
