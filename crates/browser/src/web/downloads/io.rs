//! I/O disque pour le bridge download : sniff, sha256, atomic move + meta.
//!
//! Toutes les fonctions retournent `io::Result<…>` ; le caller (bridge.rs)
//! transforme une erreur en `mark_failed` côté store sans paniquer.
//!
//! Aucun chmod, aucun follow-symlink, aucun bypass des règles de sécurité :
//! defense-in-depth invariants §4, §5, §12 du plan.

use std::fs::File;
use std::io::{Read, Result};
use std::path::{Path, PathBuf};

use nyx_core::downloads::Sha256Hasher;

const SNIFF_BYTES: usize = 8192;
const HASH_CHUNK: usize = 65_536;

/// Lit jusqu'à 8 Kio depuis le début du fichier (sans suivre symlink — sur
/// les plateformes qui le supportent, le ouvre OS résout déjà le lien ;
/// le canonical check est dans `cleanup.rs` pour le boot scan).
pub fn read_first_8k(path: &Path) -> Result<Vec<u8>> {
    let mut f = File::open(path)?;
    let mut buf = vec![0u8; SNIFF_BYTES];
    let n = f.read(&mut buf)?;
    buf.truncate(n);
    Ok(buf)
}

/// Calcule le SHA-256 du fichier complet en streaming par chunks de 64 Kio.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256Hasher::new();
    let mut buf = vec![0u8; HASH_CHUNK];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize_hex())
}

/// Move temp_payload → final_payload ET sidecar.
///
/// Séquence (cf. plan §5) :
///   1. Écrit `<temp>.nyxmeta` à côté du payload.
///   2. Choisit un nom final non-colliding (final_dir/name, name (1).ext, …).
///   3. `rename` payload, puis `rename` meta.
///
/// Crash entre 1 et 2 → le temp + meta sont orphelins, nettoyés au prochain
/// boot scan (24h). Crash entre 2 et 3 → payload présent, meta absent ;
/// la shelf signalera « non-quarantined » lors d'une consultation future
/// (à venir Tâche 6 — persistance).
pub fn move_with_meta(
    temp_payload: &Path,
    final_dir: &Path,
    desired_name: &str,
    meta_content: &str,
) -> Result<PathBuf> {
    std::fs::create_dir_all(final_dir)?;

    let temp_meta = with_nyxmeta_suffix(temp_payload);
    std::fs::write(&temp_meta, meta_content)?;

    let final_payload = pick_non_colliding(final_dir, desired_name);
    std::fs::rename(temp_payload, &final_payload)?;

    let final_meta = with_nyxmeta_suffix(&final_payload);
    std::fs::rename(&temp_meta, &final_meta)?;

    Ok(final_payload)
}

/// `foo.pdf` → `foo.pdf.nyxmeta`. Travaille sur l'OsString brut pour ne
/// jamais perdre l'extension (avec `with_extension` on overwriterait `.pdf`).
fn with_nyxmeta_suffix(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".nyxmeta");
    PathBuf::from(s)
}

/// Cherche un nom libre dans `dir`. Retourne le premier dispo parmi :
///   `name`, `name (1)`, `name (2)`, ... jusqu'à 1000 (~jamais atteint en
///   pratique). Au-delà : fallback `name-<pid>`.
fn pick_non_colliding(dir: &Path, desired: &str) -> PathBuf {
    let base = dir.join(desired);
    if !base.exists() { return base; }

    let (stem, ext) = split_stem_ext(desired);
    for i in 1..1000 {
        let candidate = if ext.is_empty() {
            dir.join(format!("{stem} ({i})"))
        } else {
            dir.join(format!("{stem} ({i}).{ext}"))
        };
        if !candidate.exists() { return candidate; }
    }
    dir.join(format!("{stem}-{}", std::process::id()))
}

/// `foo.tar.gz` → (`foo.tar`, `gz`). Préserve la double-ext de l'utilisateur.
fn split_stem_ext(name: &str) -> (&str, &str) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => (stem, ext),
        _ => (name, ""),
    }
}
