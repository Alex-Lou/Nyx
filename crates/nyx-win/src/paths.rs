//! Chemins Windows du coffre et des imports — pur (chaînes), testé sur
//! l'hôte. Les séparateurs sont toujours `\` : ces chemins ne servent que
//! sous Windows, mais leur construction se vérifie partout.

use std::path::{Path, PathBuf};

/// Distribution WSL de la capsule Nyx (Linux).
pub const CAPSULE_DISTRO: &str = "NyxCapsule";
/// Coffre de la capsule, relatif à la racine de la distribution.
const CAPSULE_VAULT: &str = r"home\nyx\.local\share\nyx\vault.db";
/// Commande qui démarre la capsule (rend `\\wsl.localhost\…` joignable).
pub const START_CAPSULE_CMD: &str = "wsl -d NyxCapsule -e true";

/// Joint des segments avec `\` (sans doubler les séparateurs).
fn win_join(base: &str, parts: &[&str]) -> String {
    let mut out = base.trim_end_matches(['\\', '/']).to_string();
    for p in parts {
        out.push('\\');
        out.push_str(p.trim_matches(['\\', '/']));
    }
    out
}

/// `%APPDATA%\Nyx\vault.db`. `None` si `APPDATA` est absent ou vide.
pub fn vault_path(appdata: Option<&str>) -> Option<String> {
    appdata.filter(|a| !a.trim().is_empty()).map(|a| win_join(a, &["Nyx", "vault.db"]))
}

/// Emplacements candidats du coffre de la capsule, par ordre de préférence
/// (`\\wsl.localhost` depuis Windows 11 / WSL récent, `\\wsl$` sinon).
pub fn capsule_vault_candidates() -> [String; 2] {
    [
        win_join(&format!(r"\\wsl.localhost\{CAPSULE_DISTRO}"), &[CAPSULE_VAULT]),
        win_join(&format!(r"\\wsl$\{CAPSULE_DISTRO}"), &[CAPSULE_VAULT]),
    ]
}

/// Racines de la distribution (pour distinguer « WSL arrêté » de « pas de
/// coffre dans la capsule »).
fn capsule_roots() -> [String; 2] {
    [format!(r"\\wsl.localhost\{CAPSULE_DISTRO}"), format!(r"\\wsl$\{CAPSULE_DISTRO}")]
}

/// État de la capsule vu depuis Windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capsule {
    /// Coffre trouvé à ce chemin.
    Found(PathBuf),
    /// Distribution joignable, mais sans coffre Nyx.
    Missing,
    /// `\\wsl.localhost\NyxCapsule` injoignable (WSL arrêté ou absent).
    Unreachable,
}

/// Sonde la capsule. `exists` est injecté (tests ; sous Windows :
/// `Path::exists`, qui peut prendre quelques secondes si WSL démarre —
/// à appeler hors du thread UI).
pub fn probe_capsule(exists: impl Fn(&Path) -> bool) -> Capsule {
    for candidate in capsule_vault_candidates() {
        let path = PathBuf::from(candidate);
        if exists(&path) {
            return Capsule::Found(path);
        }
    }
    if capsule_roots().iter().any(|r| exists(Path::new(r))) {
        Capsule::Missing
    } else {
        Capsule::Unreachable
    }
}

/// `%LOCALAPPDATA%\Google\Chrome\User Data` (profils Chrome Windows).
pub fn chrome_user_data(localappdata: Option<&str>) -> Option<String> {
    localappdata
        .filter(|a| !a.trim().is_empty())
        .map(|a| win_join(a, &["Google", "Chrome", "User Data"]))
}

/// Copie temporaire du coffre de la capsule : `<temp>\nyx-capsule-<pid>-<n>.db`.
/// Le fichier source n'est JAMAIS ouvert en écriture : on travaille sur cette
/// copie, supprimée après l'import.
pub fn capsule_temp_copy(temp: &Path, pid: u32, n: u32) -> PathBuf {
    temp.join(format!("nyx-capsule-{pid}-{n}.db"))
}

/// Fichiers annexes SQLite d'une base (`-wal`, `-shm`), à copier / nettoyer
/// avec elle : un coffre ouvert côté Linux a ses dernières écritures dans le WAL.
pub fn sidecar(db: &Path, suffix: &str) -> PathBuf {
    let mut s = db.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_in_appdata() {
        assert_eq!(
            vault_path(Some(r"C:\Users\Lou\AppData\Roaming")).as_deref(),
            Some(r"C:\Users\Lou\AppData\Roaming\Nyx\vault.db")
        );
        assert_eq!(
            vault_path(Some(r"C:\Users\Lou\AppData\Roaming\")).as_deref(),
            Some(r"C:\Users\Lou\AppData\Roaming\Nyx\vault.db")
        );
        assert_eq!(vault_path(None), None);
        assert_eq!(vault_path(Some("  ")), None);
    }

    #[test]
    fn capsule_unc_paths() {
        assert_eq!(
            capsule_vault_candidates(),
            [
                r"\\wsl.localhost\NyxCapsule\home\nyx\.local\share\nyx\vault.db".to_string(),
                r"\\wsl$\NyxCapsule\home\nyx\.local\share\nyx\vault.db".to_string(),
            ]
        );
        assert!(START_CAPSULE_CMD.contains(CAPSULE_DISTRO));
    }

    #[test]
    fn probe_states() {
        assert_eq!(probe_capsule(|_| false), Capsule::Unreachable);
        assert_eq!(probe_capsule(|p| !p.to_string_lossy().ends_with("vault.db")), Capsule::Missing);
        let second = |p: &Path| p.to_string_lossy().starts_with(r"\\wsl$");
        assert_eq!(
            probe_capsule(second),
            Capsule::Found(PathBuf::from(r"\\wsl$\NyxCapsule\home\nyx\.local\share\nyx\vault.db"))
        );
        assert!(matches!(probe_capsule(|_| true), Capsule::Found(p) if p.to_string_lossy().contains("wsl.localhost")));
    }

    #[test]
    fn chrome_dir() {
        assert_eq!(
            chrome_user_data(Some(r"C:\Users\Lou\AppData\Local")).as_deref(),
            Some(r"C:\Users\Lou\AppData\Local\Google\Chrome\User Data")
        );
        assert_eq!(chrome_user_data(None), None);
    }

    #[test]
    fn temp_copies() {
        let p = capsule_temp_copy(Path::new("/tmp"), 42, 3);
        assert!(p.ends_with("nyx-capsule-42-3.db"));
        assert!(sidecar(&p, "-wal").to_string_lossy().ends_with("nyx-capsule-42-3.db-wal"));
    }
}
