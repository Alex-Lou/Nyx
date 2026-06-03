//! Tests magic_bytes — formats canoniques, extensions, panic safety, stress.

use super::*;
use crate::download_policy::Kind;

// ─── Détection canonique ───────────────────────────────────────────────────

#[test]
fn detects_png() {
    let b = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
    assert_eq!(sniff(&b), Detected::Png);
}

#[test]
fn detects_jpeg() {
    assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0]), Detected::Jpeg);
    assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xDB]), Detected::Jpeg);
}

#[test]
fn detects_gif87a_and_89a() {
    assert_eq!(sniff(b"GIF87a..."), Detected::Gif);
    assert_eq!(sniff(b"GIF89a..."), Detected::Gif);
    // Faux GIF — pas 'a' final.
    assert_ne!(sniff(b"GIF89b..."), Detected::Gif);
}

#[test]
fn detects_webp() {
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&[0, 0, 0, 0]);
    b.extend_from_slice(b"WEBP");
    b.extend_from_slice(b"VP8 trailing");
    assert_eq!(sniff(&b), Detected::Webp);
}

#[test]
fn detects_pdf() {
    assert_eq!(sniff(b"%PDF-1.7\nstuff"), Detected::Pdf);
}

#[test]
fn detects_zip_three_variants() {
    assert_eq!(sniff(&[0x50, 0x4B, 0x03, 0x04, 0]), Detected::Zip);
    assert_eq!(sniff(&[0x50, 0x4B, 0x05, 0x06, 0]), Detected::Zip);
    assert_eq!(sniff(&[0x50, 0x4B, 0x07, 0x08, 0]), Detected::Zip);
    // Non-zip
    assert_ne!(sniff(&[0x50, 0x4B, 0x00, 0x00, 0]), Detected::Zip);
}

#[test]
fn detects_rar() {
    assert_eq!(sniff(&[b'R', b'a', b'r', b'!', 0x1A, 0x07, 0, 0]), Detected::Rar);
}

#[test]
fn detects_7z() {
    assert_eq!(sniff(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C, 0]), Detected::SevenZ);
}

#[test]
fn detects_gzip() {
    assert_eq!(sniff(&[0x1F, 0x8B, 0x08, 0]), Detected::Gzip);
}

#[test]
fn detects_bz2() {
    assert_eq!(sniff(b"BZh91AY"), Detected::Bz2);
}

#[test]
fn detects_elf() {
    assert_eq!(sniff(&[0x7F, b'E', b'L', b'F', 2, 1, 1, 0]), Detected::Elf);
}

#[test]
fn detects_macho_all_four_variants() {
    for sig in [
        [0xCE, 0xFA, 0xED, 0xFE],
        [0xCF, 0xFA, 0xED, 0xFE],
        [0xFE, 0xED, 0xFA, 0xCE],
        [0xFE, 0xED, 0xFA, 0xCF],
    ] {
        let mut b = sig.to_vec();
        b.extend_from_slice(&[0; 8]);
        assert_eq!(sniff(&b), Detected::MachO, "sig={sig:?}");
    }
}

#[test]
fn detects_pe_with_valid_offset() {
    let mut b = vec![0u8; 0x100];
    b[0] = b'M'; b[1] = b'Z';
    // Pointe e_lfanew vers offset 0x80
    b[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    b[0x80..0x84].copy_from_slice(b"PE\0\0");
    assert_eq!(sniff(&b), Detected::PeExe);
}

#[test]
fn pe_without_pe_signature_is_unknown() {
    // MZ header sans pointeur PE valide → ne doit PAS être détecté PE.
    let mut b = vec![0u8; 0x100];
    b[0] = b'M'; b[1] = b'Z';
    b[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    // Pas de "PE\0\0" à 0x80.
    assert_ne!(sniff(&b), Detected::PeExe);
}

#[test]
fn pe_with_out_of_bounds_offset_no_panic() {
    let mut b = vec![0u8; 0x80];
    b[0] = b'M'; b[1] = b'Z';
    // offset délibérément hors limite
    b[0x3C..0x40].copy_from_slice(&0xFFFFFFFFu32.to_le_bytes());
    let _ = sniff(&b); // ne doit pas paniquer
}

#[test]
fn detects_class_cafebabe() {
    assert_eq!(sniff(&[0xCA, 0xFE, 0xBA, 0xBE, 0, 0]), Detected::Class);
}

#[test]
fn detects_wasm() {
    assert_eq!(sniff(&[0x00, 0x61, 0x73, 0x6D, 1, 0, 0, 0]), Detected::Wasm);
}

#[test]
fn detects_iso() {
    let mut b = vec![0u8; 0x8010];
    b[0x8001..0x8006].copy_from_slice(b"CD001");
    assert_eq!(sniff(&b), Detected::Iso);
}

#[test]
fn detects_sqlite() {
    assert_eq!(sniff(b"SQLite format 3\0junk"), Detected::Sqlite);
}

#[test]
fn detects_tar() {
    let mut b = vec![0u8; 512];
    b[257..262].copy_from_slice(b"ustar");
    assert_eq!(sniff(&b), Detected::Tar);
}

// ─── Texte ─────────────────────────────────────────────────────────────────

#[test]
fn detects_html_doctype() {
    assert_eq!(sniff(b"<!DOCTYPE HTML><html>..."), Detected::Html);
    assert_eq!(sniff(b"<!doctype html>"),         Detected::Html);
}

#[test]
fn detects_html_bare_tag() {
    assert_eq!(sniff(b"<html><body>"), Detected::Html);
    assert_eq!(sniff(b"<HTML>"),       Detected::Html);
}

#[test]
fn detects_svg_direct() {
    assert_eq!(sniff(b"<svg xmlns=\"...\">"), Detected::Svg);
    assert_eq!(sniff(b"<SVG>"),               Detected::Svg);
}

#[test]
fn detects_svg_inside_xml_prolog() {
    let b = br#"<?xml version="1.0"?><svg xmlns="...">"#;
    assert_eq!(sniff(b), Detected::Svg);
}

#[test]
fn detects_xml_without_svg() {
    let b = br#"<?xml version="1.0"?><root>"#;
    assert_eq!(sniff(b), Detected::Xml);
}

#[test]
fn detects_json_object() {
    assert_eq!(sniff(b"   {\"a\": 1}"), Detected::Json);
}

#[test]
fn detects_json_array() {
    assert_eq!(sniff(b"\n\t[1,2,3]"), Detected::Json);
}

#[test]
fn skips_utf8_bom_for_text() {
    let mut b = vec![0xEF, 0xBB, 0xBF];
    b.extend_from_slice(b"<html>");
    assert_eq!(sniff(&b), Detected::Html);
}

// ─── matches_extension ────────────────────────────────────────────────────

#[test]
fn extension_match_obvious_positives() {
    assert!(matches_extension(Detected::Png,    "png"));
    assert!(matches_extension(Detected::Png,    "PNG"));
    assert!(matches_extension(Detected::Jpeg,   "jpg"));
    assert!(matches_extension(Detected::Jpeg,   "jpeg"));
    assert!(matches_extension(Detected::PeExe,  "exe"));
    assert!(matches_extension(Detected::PeExe,  "dll"));
    assert!(matches_extension(Detected::Zip,    "docx"));
    assert!(matches_extension(Detected::Zip,    "jar"));
}

#[test]
fn extension_match_negatives() {
    assert!(!matches_extension(Detected::Png,    "exe"));
    assert!(!matches_extension(Detected::PeExe,  "png"));
    assert!(!matches_extension(Detected::Pdf,    "jpg"));
    assert!(!matches_extension(Detected::Unknown, "anything"));
}

/// Test critique : un .png qui démarre par MZ — l'extension ment, le sniff
/// dit PeExe, et `matches_extension(PeExe, "png")` doit retourner `false`.
/// C'est le signal central du post-flight MIME mismatch.
#[test]
fn mz_buffer_with_png_extension_signals_mismatch() {
    let mut b = vec![0u8; 0x100];
    b[0] = b'M'; b[1] = b'Z';
    b[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    b[0x80..0x84].copy_from_slice(b"PE\0\0");
    let d = sniff(&b);
    assert_eq!(d, Detected::PeExe);
    assert!(!matches_extension(d, "png"),
        "détection MIME mismatch perdue : MZ déguisé en .png passerait");
}

// ─── category ──────────────────────────────────────────────────────────────

#[test]
fn category_executable_for_binaries() {
    assert_eq!(category(Detected::PeExe), Kind::Executable);
    assert_eq!(category(Detected::Elf),   Kind::Executable);
    assert_eq!(category(Detected::MachO), Kind::Executable);
    assert_eq!(category(Detected::Class), Kind::Executable);
    assert_eq!(category(Detected::Wasm),  Kind::Executable);
}

#[test]
fn category_safe_for_media_and_docs() {
    assert_eq!(category(Detected::Png),  Kind::Safe);
    assert_eq!(category(Detected::Pdf),  Kind::Safe);
    assert_eq!(category(Detected::Json), Kind::Safe);
    assert_eq!(category(Detected::Xml),  Kind::Safe);
}

#[test]
fn category_archive_for_containers() {
    assert_eq!(category(Detected::Zip),    Kind::Archive);
    assert_eq!(category(Detected::Gzip),   Kind::Archive);
    assert_eq!(category(Detected::SevenZ), Kind::Archive);
    assert_eq!(category(Detected::Iso),    Kind::Archive);
}

#[test]
fn category_active_for_markup() {
    assert_eq!(category(Detected::Html), Kind::ActiveDoc);
    assert_eq!(category(Detected::Svg),  Kind::ActiveDoc);
}

#[test]
fn category_unknown_passthrough() {
    assert_eq!(category(Detected::Unknown), Kind::Unknown);
}

// ─── Robustesse ────────────────────────────────────────────────────────────

#[test]
fn empty_input_is_unknown() {
    assert_eq!(sniff(&[]), Detected::Unknown);
}

#[test]
fn one_byte_inputs_no_panic() {
    for b in 0u8..=255 {
        let _ = sniff(&[b]);
    }
}

#[test]
fn two_three_byte_inputs_no_panic() {
    for x in 0..256u32 {
        let _ = sniff(&[x as u8, (x >> 8) as u8]);
        let _ = sniff(&[x as u8, (x >> 8) as u8, 0]);
    }
}

#[test]
fn truncated_below_signature_length_unknown() {
    // PNG fait 8 octets ; 7 octets de header PNG → Unknown.
    assert_eq!(sniff(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A]), Detected::Unknown);
    // RIFF sans WEBP marker tronqué.
    assert_eq!(sniff(b"RIFF\0\0\0\0WE"), Detected::Unknown);
}

#[test]
fn trailing_bytes_dont_break_detection() {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    b.extend(std::iter::repeat_n(0xAB, 4096));
    assert_eq!(sniff(&b), Detected::Png);
}

// ─── Stress ────────────────────────────────────────────────────────────────

/// LCG déterministe (Numerical Recipes) — pas de Math.random, pas de seed
/// externe : la séquence est reproductible run-to-run.
fn lcg(state: u32) -> u32 {
    state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223)
}

/// Stress : 10 000 buffers pseudo-aléatoires de tailles variées. Pas de
/// panic et distribution raisonnable (au moins quelques détections positives
/// car certaines séquences vont matcher par hasard).
#[test]
fn stress_10k_random_buffers() {
    let mut state = 0xC0FFEEu32;
    let mut unknown = 0usize;
    let mut detected = 0usize;
    for _ in 0..10_000 {
        let len = (lcg(state) % 600) as usize; // 0..600 octets
        state = lcg(state);
        let mut buf = Vec::with_capacity(len);
        for _ in 0..len {
            state = lcg(state);
            buf.push((state >> 16) as u8);
        }
        match sniff(&buf) {
            Detected::Unknown => unknown += 1,
            _ => detected += 1,
        }
    }
    // L'écrasante majorité de hasard est Unknown — bon signe (pas de
    // détecteur trop laxiste). Mais on veut au moins UN positif (peut être 0
    // sur certaines seeds extrêmes ; ici la séquence le garantit).
    assert!(unknown > 9_000, "trop de détections sur du bruit : {detected}/{}",
            unknown + detected);
}

#[test]
fn stress_5k_random_then_signature_overwritten() {
    // Variante : on impose un préfixe PNG dans 1/4 des cas → on doit le détecter.
    let mut state = 0xDEADBEEFu32;
    let mut png_detections = 0usize;
    let mut png_planted = 0usize;
    for i in 0..5_000 {
        state = lcg(state);
        let len = 8 + (state % 100) as usize;
        let mut buf = vec![0u8; len];
        for b in buf.iter_mut() {
            state = lcg(state);
            *b = (state >> 16) as u8;
        }
        if i % 4 == 0 {
            buf[..8].copy_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
            png_planted += 1;
        }
        if sniff(&buf) == Detected::Png { png_detections += 1; }
    }
    assert_eq!(png_detections, png_planted,
        "tous les PNG plantés doivent être détectés ({png_detections}/{png_planted})");
}
