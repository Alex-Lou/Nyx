//! Construction du chemin temporaire d'un téléchargement.
//!
//! Stratégie : `<32-hex-random>_<sanitized_filename>` dans le dossier
//! `temp_root` choisi par le browser.
//!
//! - `build_temp_path` est **pur** (prend les bytes random en paramètre).
//!   Toutes les vérifications de sécurité (sanitize, hex, longueur) sont
//!   testées en déterministe sur des bytes fixés.
//! - `fresh_random_bytes` est la **seule** fonction impure de nyx-core :
//!   elle appelle `getrandom::getrandom` (syscall CSPRNG OS). Isolée ici
//!   pour que le reste reste auditable comme purement déterministe.
//!
//! Voir docs/download-bridge-plan.md §4 « Temp dir et naming ».

use std::path::{Path, PathBuf};

/// Nombre d'octets random injectés dans le suffix. 16 octets = 32 hex
/// = 128 bits d'entropie (CSPRNG OS). Décision review user : passage de
/// 64 bits → 128 bits pour zéro collision même sur 10⁹ DL.
pub const RANDOM_BYTES: usize = 16;

/// Limite défensive sur le filename sanitisé (déjà imposée par
/// `download_policy::sanitize_filename` → 200 chars, ici on couvre le
/// cas où un appelant futur oublierait de pré-sanitize).
const MAX_FILENAME_LEN: usize = 200;

/// Construit le path temporaire complet. Pur, déterministe pour des
/// `random_bytes` fixés.
///
/// Garanties :
/// - le filename produit ne contient aucun séparateur (`/` ni `\`) ;
/// - ne contient pas de NUL byte ;
/// - reste ≤ `MAX_FILENAME_LEN + 33` (32 hex + `_` + sanitized).
///
/// Si `sanitized_filename` est vide ou contient des caractères dangereux,
/// on substitue `"download"`. Pas de path-traversal possible : le
/// résultat est `temp_root.join(name)` avec `name` strictement plat.
pub fn build_temp_path(
    temp_root: &Path,
    random_bytes: &[u8; RANDOM_BYTES],
    sanitized_filename: &str,
) -> PathBuf {
    let hex = hex_lower(random_bytes);
    let safe_name = sanitize_segment(sanitized_filename);
    let combined = if safe_name.is_empty() {
        format!("{hex}_download")
    } else {
        format!("{hex}_{safe_name}")
    };
    temp_root.join(combined)
}

/// Génère 16 octets CSPRNG via `getrandom`. **Seule** impureté de
/// nyx-core. Le browser appelle ceci puis passe le résultat à
/// `build_temp_path`. Panic si l'OS refuse — c'est une condition
/// catastrophique (pas de boot sans CSPRNG).
pub fn fresh_random_bytes() -> [u8; RANDOM_BYTES] {
    let mut bytes = [0u8; RANDOM_BYTES];
    getrandom::getrandom(&mut bytes)
        .expect("getrandom failed — system CSPRNG unavailable");
    bytes
}

// ─── Helpers ───────────────────────────────────────────────────────────────

fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Retire séparateurs, NUL, control chars. Truncate à MAX_FILENAME_LEN.
/// Si vide après filtrage → "".
fn sanitize_segment(s: &str) -> String {
    let cleaned: String = s.chars()
        .filter(|c| !matches!(c, '/' | '\\' | '\0') && !c.is_control())
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() { return String::new(); }
    if trimmed.chars().count() <= MAX_FILENAME_LEN {
        trimmed.to_string()
    } else {
        trimmed.chars().take(MAX_FILENAME_LEN).collect()
    }
}

#[cfg(test)]
mod tests;
