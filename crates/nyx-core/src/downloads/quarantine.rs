//! Sidecar de quarantaine — format `key=value` strict, jamais JSON.
//!
//! Le browser écrit `<final>.nyxmeta` à côté de chaque fichier téléchargé
//! validé. Le format est volontairement **plus restrictif que JSON** :
//!
//! - un `key=value\n` par ligne, séparateur = le premier `=` rencontré ;
//! - clés `[a-z0-9_]+` strict (premier char non-digit pour rester lisible) ;
//! - valeurs UTF-8 sans NUL/CR/LF/control (sauf espace), ≤ 512 octets ;
//! - lignes vides et `#...` ignorées (commentaires futurs) ;
//! - clés inconnues à la lecture : ignorées (forward compat) ;
//! - clés requises manquantes : `parse` retourne `None`.
//!
//! Surface d'attaque < JSON manuel ET < serde_json (pas de Unicode
//! escapes, pas de profondeur, pas de duplicate keys interprétés).
//! Le format reste évolutif : ajout de champs = nouvelle clé sans casser
//! les anciennes lectures.
//!
//! Voir docs/download-bridge-plan.md §5.

/// Version actuelle. Incrémenter dès qu'un champ change de sémantique
/// (l'ajout simple de nouvelles clés ne nécessite PAS de bump).
pub const SCHEMA_VERSION: u32 = 1;

/// Borne dure sur la longueur d'une valeur. Au-delà → rejet à
/// l'écriture, ignoré à la lecture. 512 octets couvre largement les
/// noms / mime / hosts attendus.
pub const MAX_VALUE_LEN: usize = 512;

/// Quarantine metadata — sérialisable vers `.nyxmeta` sidecar.
///
/// Champs jamais inclus (defense-in-depth, cf. plan §5) :
///   - URL complète avec path / query / fragment
///   - cookies, headers, tokens
///   - full path absolu (le sidecar vit à côté du fichier — basename
///     déductible du nom du sibling)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarantineMeta {
    pub schema:           u32,
    pub sha256:           String,
    pub size_bytes:       u64,
    pub sniffed:          String,
    pub declared_ext:     String,
    pub declared_mime:    String,
    pub source_host:      String,
    pub final_host:       String,
    pub started_at_unix:  i64,
    pub finished_at_unix: i64,
    pub verdict:          String,
    pub reasons:          Vec<String>,
}

/// Erreurs renvoyées par [`serialize`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaError {
    /// Une clé interne ne matche pas `[a-z_]+`. Bug du code, jamais
    /// dépendant d'une donnée externe (les clés sont en dur).
    InvalidKey(String),
    /// Une valeur contient un char interdit ou dépasse `MAX_VALUE_LEN`.
    InvalidValue { key: &'static str, why: ValueReject },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueReject {
    TooLong,
    ContainsForbiddenChar, // NUL, CR, LF, ou control char non-espace
    InvalidReasonToken,    // pour les champs `reasons`, un token ne matche pas [A-Za-z]+
}

// ─── API publique ──────────────────────────────────────────────────────────

/// Sérialise vers un buffer texte prêt à écrire dans `.nyxmeta`.
///
/// Retourne `Err` si une valeur est invalide (la struct fournie est
/// corrompue par construction — bug à corriger en amont, pas un input
/// externe). Side-effect-free.
pub fn serialize(meta: &QuarantineMeta) -> Result<String, MetaError> {
    let mut out = String::with_capacity(512);
    write_u32  (&mut out, "schema",           meta.schema)?;
    write_str  (&mut out, "sha256",           &meta.sha256)?;
    write_u64  (&mut out, "size_bytes",       meta.size_bytes)?;
    write_str  (&mut out, "sniffed",          &meta.sniffed)?;
    write_str  (&mut out, "declared_ext",     &meta.declared_ext)?;
    write_str  (&mut out, "declared_mime",    &meta.declared_mime)?;
    write_str  (&mut out, "source_host",      &meta.source_host)?;
    write_str  (&mut out, "final_host",       &meta.final_host)?;
    write_i64  (&mut out, "started_at_unix",  meta.started_at_unix)?;
    write_i64  (&mut out, "finished_at_unix", meta.finished_at_unix)?;
    write_str  (&mut out, "verdict",          &meta.verdict)?;
    write_reasons(&mut out, &meta.reasons)?;
    Ok(out)
}

/// Parse un buffer texte. `None` si un champ requis manque ou si la
/// schema_version est absente / invalide.
///
/// Ne panique JAMAIS sur input hostile. Tout champ unconnu est ignoré
/// (forward compat). Doublons : la dernière occurrence gagne.
pub fn parse(input: &str) -> Option<QuarantineMeta> {
    let mut schema:           Option<u32>    = None;
    let mut sha256:           Option<String> = None;
    let mut size_bytes:       Option<u64>    = None;
    let mut sniffed:          Option<String> = None;
    let mut declared_ext:     Option<String> = None;
    let mut declared_mime:    Option<String> = None;
    let mut source_host:      Option<String> = None;
    let mut final_host:       Option<String> = None;
    let mut started_at_unix:  Option<i64>    = None;
    let mut finished_at_unix: Option<i64>    = None;
    let mut verdict:          Option<String> = None;
    let mut reasons: Vec<String> = Vec::new();

    for raw_line in input.lines() {
        let line = raw_line.trim_start();
        if line.is_empty() || line.starts_with('#') { continue; }
        let Some((key, value)) = line.split_once('=') else { continue; };
        let key = key.trim();
        let value = value.trim();
        if !is_valid_key(key) || !is_valid_value(value) { continue; }

        match key {
            "schema"           => schema           = value.parse().ok(),
            "sha256"           => sha256           = Some(value.to_string()),
            "size_bytes"       => size_bytes       = value.parse().ok(),
            "sniffed"          => sniffed          = Some(value.to_string()),
            "declared_ext"     => declared_ext     = Some(value.to_string()),
            "declared_mime"    => declared_mime    = Some(value.to_string()),
            "source_host"      => source_host      = Some(value.to_string()),
            "final_host"       => final_host       = Some(value.to_string()),
            "started_at_unix"  => started_at_unix  = value.parse().ok(),
            "finished_at_unix" => finished_at_unix = value.parse().ok(),
            "verdict"          => verdict          = Some(value.to_string()),
            "reasons"          => reasons          = parse_reasons(value),
            _ => {} // forward compat
        }
    }

    Some(QuarantineMeta {
        schema:           schema?,
        sha256:           sha256?,
        size_bytes:       size_bytes?,
        sniffed:          sniffed?,
        declared_ext:     declared_ext.unwrap_or_default(),
        declared_mime:    declared_mime.unwrap_or_default(),
        source_host:      source_host.unwrap_or_default(),
        final_host:       final_host.unwrap_or_default(),
        started_at_unix:  started_at_unix?,
        finished_at_unix: finished_at_unix?,
        verdict:          verdict?,
        reasons,
    })
}

// ─── Writers internes ──────────────────────────────────────────────────────

fn write_str(out: &mut String, key: &'static str, value: &str) -> Result<(), MetaError> {
    if !is_valid_key(key) { return Err(MetaError::InvalidKey(key.into())); }
    if value.len() > MAX_VALUE_LEN {
        return Err(MetaError::InvalidValue { key, why: ValueReject::TooLong });
    }
    if !value_chars_ok(value) {
        return Err(MetaError::InvalidValue { key, why: ValueReject::ContainsForbiddenChar });
    }
    out.push_str(key); out.push('='); out.push_str(value); out.push('\n');
    Ok(())
}

fn write_u32(out: &mut String, key: &'static str, v: u32) -> Result<(), MetaError> {
    write_str(out, key, &v.to_string())
}
fn write_u64(out: &mut String, key: &'static str, v: u64) -> Result<(), MetaError> {
    write_str(out, key, &v.to_string())
}
fn write_i64(out: &mut String, key: &'static str, v: i64) -> Result<(), MetaError> {
    write_str(out, key, &v.to_string())
}

fn write_reasons(out: &mut String, reasons: &[String]) -> Result<(), MetaError> {
    for r in reasons {
        if !is_valid_reason(r) {
            return Err(MetaError::InvalidValue {
                key: "reasons", why: ValueReject::InvalidReasonToken,
            });
        }
    }
    let joined = reasons.join(",");
    write_str(out, "reasons", &joined)
}

// ─── Validators ────────────────────────────────────────────────────────────

fn is_valid_key(s: &str) -> bool {
    // [a-z_][a-z0-9_]* — premier char non-digit (lisibilité), reste OK
    // avec digits (clés réelles : `sha256`, `size_bytes`, `started_at_unix`).
    let mut bytes = s.bytes();
    let Some(first) = bytes.next() else { return false; };
    if !(first == b'_' || first.is_ascii_lowercase()) { return false; }
    bytes.all(|b| b == b'_' || b.is_ascii_lowercase() || b.is_ascii_digit())
}

fn is_valid_value(s: &str) -> bool {
    s.len() <= MAX_VALUE_LEN && value_chars_ok(s)
}

fn value_chars_ok(s: &str) -> bool {
    !s.chars().any(|c| c != ' ' && c.is_control())
}

/// Reasons : alphabétique uniquement, sans virgule. Aligne sur les noms
/// de variantes Rust (`SafeType`, `DangerousOrigin`, …).
fn is_valid_reason(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphabetic())
}

fn parse_reasons(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|t| is_valid_reason(t))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests;
