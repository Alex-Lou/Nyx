//! Tests quarantine — round-trip, validation, hostiles, stress.

use super::*;

fn good_meta() -> QuarantineMeta {
    QuarantineMeta {
        schema:           SCHEMA_VERSION,
        sha256:           "a".repeat(64),
        size_bytes:       12345,
        sniffed:          "Png".into(),
        declared_ext:     "png".into(),
        declared_mime:    "image/png".into(),
        source_host:      "example.com".into(),
        final_host:       "example.com".into(),
        started_at_unix:  1_700_000_000,
        finished_at_unix: 1_700_000_050,
        verdict:          "Allow".into(),
        reasons:          vec!["SafeType".into()],
    }
}

// ─── Round-trip nominal ────────────────────────────────────────────────────

#[test]
fn round_trip_identity() {
    let m = good_meta();
    let s = serialize(&m).expect("serialize");
    let back = parse(&s).expect("parse");
    assert_eq!(back, m);
}

#[test]
fn round_trip_with_empty_optional_fields() {
    let mut m = good_meta();
    m.declared_ext = String::new();
    m.declared_mime = String::new();
    m.source_host = String::new();
    let s = serialize(&m).expect("serialize");
    let back = parse(&s).expect("parse");
    assert_eq!(back, m);
}

#[test]
fn round_trip_with_multiple_reasons() {
    let mut m = good_meta();
    m.reasons = vec!["SafeType".into(), "HttpDownload".into(), "OctetStreamMime".into()];
    let s = serialize(&m).expect("serialize");
    let back = parse(&s).expect("parse");
    assert_eq!(back.reasons, m.reasons);
}

#[test]
fn round_trip_empty_reasons() {
    let mut m = good_meta();
    m.reasons.clear();
    let s = serialize(&m).expect("serialize");
    let back = parse(&s).expect("parse");
    assert!(back.reasons.is_empty());
}

// ─── Format de sortie ──────────────────────────────────────────────────────

#[test]
fn output_starts_with_schema() {
    let s = serialize(&good_meta()).unwrap();
    assert!(s.starts_with("schema=1\n"), "got: {s}");
}

#[test]
fn output_uses_lf_line_endings() {
    let s = serialize(&good_meta()).unwrap();
    assert!(!s.contains("\r"), "should be LF-only");
    assert!(s.ends_with('\n'));
}

#[test]
fn output_one_field_per_line() {
    let s = serialize(&good_meta()).unwrap();
    let n_eq = s.bytes().filter(|&b| b == b'=').count();
    let n_nl = s.bytes().filter(|&b| b == b'\n').count();
    assert_eq!(n_eq, n_nl, "every '=' should map to one '\\n'");
}

// ─── Validation à l'écriture ───────────────────────────────────────────────

#[test]
fn serialize_rejects_value_with_newline() {
    let mut m = good_meta();
    m.source_host = "evil\nfake".into();
    assert_eq!(
        serialize(&m),
        Err(MetaError::InvalidValue {
            key: "source_host", why: ValueReject::ContainsForbiddenChar,
        })
    );
}

#[test]
fn serialize_rejects_value_with_nul() {
    let mut m = good_meta();
    m.sniffed = "Png\0".into();
    assert!(matches!(
        serialize(&m),
        Err(MetaError::InvalidValue { key: "sniffed", .. })
    ));
}

#[test]
fn serialize_rejects_value_with_cr() {
    let mut m = good_meta();
    m.verdict = "Allow\rspoof".into();
    assert!(matches!(
        serialize(&m),
        Err(MetaError::InvalidValue { key: "verdict", .. })
    ));
}

#[test]
fn serialize_rejects_value_with_control() {
    let mut m = good_meta();
    m.declared_mime = "image/\x07png".into();
    assert!(matches!(
        serialize(&m),
        Err(MetaError::InvalidValue { key: "declared_mime", .. })
    ));
}

#[test]
fn serialize_accepts_space_in_value() {
    let mut m = good_meta();
    m.declared_mime = "image / png with space".into();
    assert!(serialize(&m).is_ok());
}

#[test]
fn serialize_rejects_value_too_long() {
    let mut m = good_meta();
    m.sha256 = "x".repeat(MAX_VALUE_LEN + 1);
    assert!(matches!(
        serialize(&m),
        Err(MetaError::InvalidValue { key: "sha256", why: ValueReject::TooLong })
    ));
}

#[test]
fn serialize_rejects_reason_with_comma() {
    let mut m = good_meta();
    m.reasons = vec!["Safe,Type".into()];
    assert!(matches!(
        serialize(&m),
        Err(MetaError::InvalidValue { key: "reasons", why: ValueReject::InvalidReasonToken })
    ));
}

#[test]
fn serialize_rejects_reason_with_digit() {
    let mut m = good_meta();
    m.reasons = vec!["Safe1".into()];
    assert!(matches!(
        serialize(&m),
        Err(MetaError::InvalidValue { key: "reasons", .. })
    ));
}

// ─── Parsing : champs requis ───────────────────────────────────────────────

#[test]
fn parse_returns_none_when_schema_missing() {
    let mut m = good_meta();
    let s = serialize(&m).unwrap();
    let without_schema = s.replace("schema=1\n", "");
    assert_eq!(parse(&without_schema), None);
    let _ = &mut m;
}

#[test]
fn parse_returns_none_when_sha256_missing() {
    let m = good_meta();
    let s = serialize(&m).unwrap();
    let truncated = s.replace(&format!("sha256={}\n", m.sha256), "");
    assert_eq!(parse(&truncated), None);
}

#[test]
fn parse_returns_none_when_verdict_missing() {
    let m = good_meta();
    let s = serialize(&m).unwrap();
    let truncated = s.replace("verdict=Allow\n", "");
    assert_eq!(parse(&truncated), None);
}

// ─── Parsing : forward compat ──────────────────────────────────────────────

#[test]
fn parse_ignores_unknown_keys() {
    let s = "schema=1\nsha256=abc\nsize_bytes=1\nsniffed=Png\n\
             started_at_unix=0\nfinished_at_unix=0\nverdict=Allow\n\
             future_field=xyz\nanother_unknown=42\n";
    assert!(parse(s).is_some());
}

#[test]
fn parse_ignores_comments_and_blank_lines() {
    let s = "# header comment\n\
             \n\
             schema=1\n\
             # mid comment\n\
             sha256=abc\n\
             size_bytes=1\n\
             sniffed=Png\n\
             started_at_unix=0\n\
             finished_at_unix=0\n\
             verdict=Allow\n";
    assert!(parse(s).is_some());
}

#[test]
fn parse_last_duplicate_wins() {
    let s = "schema=1\nsha256=abc\nsha256=def\nsize_bytes=1\n\
             sniffed=Png\nstarted_at_unix=0\nfinished_at_unix=0\n\
             verdict=Allow\n";
    let m = parse(s).unwrap();
    assert_eq!(m.sha256, "def");
}

#[test]
fn parse_ignores_invalid_key_chars() {
    let s = "Schema=1\nsha256=x\nsize_bytes=1\nsniffed=X\n\
             started_at_unix=0\nfinished_at_unix=0\nverdict=Y\n";
    // `Schema` (capital) est rejeté → schema manquant → None.
    assert_eq!(parse(s), None);
}

#[test]
fn parse_ignores_value_with_control_chars() {
    let s = "schema=1\nsha256=ab\u{0007}cd\nsize_bytes=1\nsniffed=X\n\
             started_at_unix=0\nfinished_at_unix=0\nverdict=Y\n";
    // sha256 value contient BEL → ligne ignorée → sha256 manquant → None.
    assert_eq!(parse(s), None);
}

#[test]
fn parse_handles_equal_in_value() {
    // Le premier '=' est le séparateur ; le reste fait partie de la valeur.
    let s = "schema=1\nsha256=abc\nsize_bytes=1\nsniffed=Png\n\
             declared_mime=application/x-foo=bar\n\
             started_at_unix=0\nfinished_at_unix=0\nverdict=Allow\n";
    let m = parse(s).unwrap();
    assert_eq!(m.declared_mime, "application/x-foo=bar");
}

#[test]
fn parse_handles_reasons_with_whitespace() {
    let s = "schema=1\nsha256=x\nsize_bytes=1\nsniffed=X\n\
             started_at_unix=0\nfinished_at_unix=0\nverdict=Y\n\
             reasons=  SafeType , HttpDownload  \n";
    let m = parse(s).unwrap();
    assert_eq!(m.reasons, vec!["SafeType".to_string(), "HttpDownload".into()]);
}

#[test]
fn parse_drops_invalid_reasons_tokens() {
    let s = "schema=1\nsha256=x\nsize_bytes=1\nsniffed=X\n\
             started_at_unix=0\nfinished_at_unix=0\nverdict=Y\n\
             reasons=Safe,42,Bad-Reason,HttpDownload\n";
    let m = parse(s).unwrap();
    assert_eq!(m.reasons, vec!["Safe".to_string(), "HttpDownload".into()]);
}

// ─── Hostiles : pas de panic ───────────────────────────────────────────────

#[test]
fn parse_empty_input_returns_none() {
    assert_eq!(parse(""), None);
}

#[test]
fn parse_only_garbage_returns_none() {
    for s in ["===", "\0\n\r", "a=b\nc=d", "#####", "\0"] {
        // Pas de panic.
        let _ = parse(s);
    }
}

#[test]
fn parse_very_long_input_doesnt_panic() {
    let mut s = String::new();
    for i in 0..10_000 {
        s.push_str(&format!("unknown_{i}=value\n"));
    }
    assert_eq!(parse(&s), None); // pas de schema → None
}

#[test]
fn parse_line_too_long_is_ignored() {
    let huge = "x".repeat(MAX_VALUE_LEN + 100);
    let s = format!("schema=1\nsha256={huge}\nsize_bytes=1\nsniffed=X\n\
                     started_at_unix=0\nfinished_at_unix=0\nverdict=Y\n");
    // sha256 ligne trop longue → ignorée → manquant → None.
    assert_eq!(parse(&s), None);
}

// ─── Constants exposées ────────────────────────────────────────────────────

#[test]
fn schema_version_is_one() {
    assert_eq!(SCHEMA_VERSION, 1);
}

#[test]
fn max_value_len_is_512() {
    assert_eq!(MAX_VALUE_LEN, 512);
}

// ─── Stress ────────────────────────────────────────────────────────────────

/// 5 000 round-trips variés — pas de panic, identité préservée.
#[test]
fn stress_5k_round_trips() {
    let bases = ["example.com", "github.com", "cdn.example.org",
                 "very.long.subdomain.example.com", ""];
    let verdicts = ["Allow", "Ask", "AskDanger", "Block"];
    let sniffeds = ["Png", "PeExe", "Pdf", "Unknown", "Zip"];

    for i in 0..5_000usize {
        let m = QuarantineMeta {
            schema:           SCHEMA_VERSION,
            sha256:           format!("{:0>64x}", i),
            size_bytes:       i as u64 * 13,
            sniffed:          sniffeds[i % sniffeds.len()].into(),
            declared_ext:     "pdf".into(),
            declared_mime:    "application/pdf".into(),
            source_host:      bases[i % bases.len()].into(),
            final_host:       bases[(i + 1) % bases.len()].into(),
            started_at_unix:  1_700_000_000 + i as i64,
            finished_at_unix: 1_700_000_100 + i as i64,
            verdict:          verdicts[i % verdicts.len()].into(),
            reasons:          vec!["SafeType".into(), "HttpDownload".into()],
        };
        let s = serialize(&m).expect("serialize");
        let back = parse(&s).expect("parse");
        assert_eq!(back, m);
    }
}

#[test]
fn stress_5k_hostile_parses_no_panic() {
    let weird = [
        "", "schema", "schema=", "=1", "schema=1",
        "\0\n", "abc=def\nghi", "#comment\n",
        "schema=99999999999999999999",
        "schema=-1",
        "schema=1\nsha256=\nsize_bytes=1\nsniffed=X\nstarted_at_unix=0\nfinished_at_unix=0\nverdict=Y\n",
    ];
    for _ in 0..500 {
        for w in &weird {
            let _ = parse(w);
        }
    }
}
