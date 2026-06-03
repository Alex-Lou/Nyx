//! Résolution du dossier temporaire d'in-flight downloads.
//!
//! Stratégie XDG-aware. Sur Unix, le parent est créé en mode 0700 si
//! possible (defense-in-depth invariant §1 du plan).
//!
//! - Linux : `$XDG_CACHE_HOME/nyx/downloads` ou `~/.cache/nyx/downloads`
//! - macOS : `~/Library/Caches/nyx/downloads`
//! - Windows : `%LOCALAPPDATA%\nyx\downloads`
//! - Fallback dernier recours : `std::env::temp_dir()/nyx-downloads`

use std::path::{Path, PathBuf};

pub fn temp_root() -> PathBuf {
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Library").join("Caches"))
            .unwrap_or_else(std::env::temp_dir)
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME")
                .map(|h| PathBuf::from(h).join(".cache")))
            .unwrap_or_else(std::env::temp_dir)
    };
    base.join("nyx").join("downloads")
}

/// `create_dir_all` + chmod 0700 best-effort (Unix). Idempotent.
pub fn ensure(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Best-effort : on ignore l'erreur de permissions (système non-conforme,
        // FAT mount, etc.) ; la création du dir reste l'invariant dur.
        let _ = std::fs::set_permissions(
            path,
            std::fs::Permissions::from_mode(0o700),
        );
    }
    Ok(())
}
