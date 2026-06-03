//! Boot scan du temp_root — purge ce qui traîne depuis > 24h.
//!
//! Invariants (cf. plan §6) :
//!   - aucun suivi de symlink (`is_symlink` skip + canonical check) ;
//!   - aucun écart hors de `temp_root` après canonicalize ;
//!   - aucune récursion (un seul `read_dir`) ;
//!   - erreurs ignorées silencieusement (best-effort).

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const TTL_SECONDS: i64 = 24 * 3600;

/// `now_unix` est passé par le caller pour rester déterministe en test
/// (et cohérent avec la pureté de nyx-core qui interdit `Instant::now`
/// dans certains modules).
pub fn scan_and_purge(temp_root: &Path, now_unix: i64) {
    let Ok(entries) = std::fs::read_dir(temp_root) else { return; };
    let Ok(canonical_root) = temp_root.canonicalize() else { return; };
    let cutoff = now_unix - TTL_SECONDS;

    for entry in entries.flatten() {
        if let Err(e) = try_purge_one(&entry, &canonical_root, cutoff) {
            // Ignored — l'invariant est best-effort, pas bloquant.
            let _ = e;
        }
    }
}

fn try_purge_one(
    entry: &std::fs::DirEntry, canonical_root: &Path, cutoff: i64,
) -> std::io::Result<()> {
    let meta = entry.metadata()?;

    // Defense #1 : on ne suit jamais un symlink.
    if meta.file_type().is_symlink() { return Ok(()); }

    // Defense #2 : le canonical de l'entrée doit rester sous canonical_root.
    // Empêche un junction point Windows / hardlink Unix de pointer ailleurs.
    let path = entry.path();
    let Ok(canonical) = path.canonicalize() else { return Ok(()); };
    if !canonical.starts_with(canonical_root) { return Ok(()); }

    // Defense #3 : on ne descend pas dans les sous-dossiers (read_dir flat).
    if !meta.is_file() { return Ok(()); }

    let modified_secs = meta.modified()?
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    if modified_secs < cutoff {
        // Best-effort. Erreur ignorée (file in use, permission, …).
        let _ = std::fs::remove_file(&path);
    }
    Ok(())
}
