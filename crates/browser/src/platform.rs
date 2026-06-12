//! Détection d'environnement + actions OS pour ouvrir / révéler des fichiers.
//!
//! Sous WSL, deux sous-systèmes WebKit sont cassés (pas de vrai GPU, pas de
//! bus-proxy bubblewrap) → on adapte au démarrage.
//!
//! Les helpers `open_path` / `reveal_in_file_manager` sont utilisés par la
//! shelf de téléchargements pour ouvrir un fichier ou son dossier conteneur.
//! Ils ne lancent JAMAIS de processus avec des arguments construits depuis
//! une entrée web non sanitizée : le chemin vient toujours du
//! `DownloadStore` (déjà passé par `download_policy::analyze`).

use std::path::Path;
use std::process::Command;

// ─── Environnement ─────────────────────────────────────────────────────────

/// `true` si on tourne sous WSL (noyau « microsoft »).
pub fn is_wsl() -> bool {
    std::fs::read_to_string("/proc/version")
        .map(|v| v.to_lowercase().contains("microsoft"))
        .unwrap_or(false)
}

// ─── Actions fichier ───────────────────────────────────────────────────────

/// Ouvre `path` dans l'application par défaut du système.
///
/// Retourne `Err` si le helper OS n'a pas pu être lancé (path inexistant,
/// commande absente, …). Le browser logge mais ne crash pas.
pub fn open_path(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("path absent : {}", path.display()));
    }
    let cmd = pick_open_command(path);
    spawn(cmd)
}

/// Ouvre le file manager OS en sélectionnant `path` si possible. Fallback :
/// ouvre simplement le dossier parent.
pub fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("path absent : {}", path.display()));
    }
    let cmd = pick_reveal_command(path);
    spawn(cmd)
}

// ─── Sélection de commande par plateforme ──────────────────────────────────

#[cfg(target_os = "windows")]
fn pick_open_command(path: &Path) -> Command {
    // `explorer.exe <path>` : pour un fichier, explorer délègue à ShellExecute
    // → ouverture dans l'app par défaut. Pour un dossier, idem natif. Évite
    // `cmd /c start` dont l'arg est re-parsé par cmd.exe (règles de quoting
    // différentes de CreateProcess → vecteur d'injection théorique si le
    // path contient `&|"`). Defense-in-depth : un seul exécutable côté
    // appelant, un seul argument passé à CreateProcess.
    let mut c = Command::new("explorer.exe");
    c.arg(path);
    c
}

#[cfg(target_os = "windows")]
fn pick_reveal_command(path: &Path) -> Command {
    let mut c = Command::new("explorer.exe");
    // /select, doit être collé (pas d'espace), suivi du path.
    c.arg(format!("/select,{}", path.display()));
    c
}

#[cfg(target_os = "macos")]
fn pick_open_command(path: &Path) -> Command {
    let mut c = Command::new("open");
    c.arg(path);
    c
}

#[cfg(target_os = "macos")]
fn pick_reveal_command(path: &Path) -> Command {
    let mut c = Command::new("open");
    c.arg("-R").arg(path);
    c
}

#[cfg(all(unix, not(target_os = "macos")))]
fn pick_open_command(path: &Path) -> Command {
    // Sous WSL : on délègue à explorer.exe via le pont. Plus naturel pour
    // l'utilisateur (le DL est dans /mnt/c/Users/.../Downloads typiquement).
    if is_wsl() {
        let mut c = Command::new("explorer.exe");
        c.arg(path);
        return c;
    }
    let mut c = Command::new("xdg-open");
    c.arg(path);
    c
}

#[cfg(all(unix, not(target_os = "macos")))]
fn pick_reveal_command(path: &Path) -> Command {
    if is_wsl() {
        let mut c = Command::new("explorer.exe");
        c.arg(format!("/select,{}", path.display()));
        return c;
    }
    // Linux natif : pas de way standard pour « select ». On ouvre le parent.
    // L'utilisateur peut configurer son DE (Nautilus a un D-Bus dédié) mais
    // un fallback simple est plus prévisible.
    let mut c = Command::new("xdg-open");
    c.arg(path.parent().unwrap_or(path));
    c
}

fn spawn(mut cmd: Command) -> Result<(), String> {
    // `spawn` ne bloque pas (le process file manager vit indépendamment).
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("spawn failed: {e}"))
}

// ─── Résolution du dossier de téléchargement par défaut ────────────────────

/// Renvoie `~/Downloads` (ou équivalent OS) si pas d'override en settings.
///
/// Pure best-effort : ne touche pas au disque, ne crée pas le dossier.
/// Si `HOME` (Unix) ou `USERPROFILE` (Windows) est absent, retourne `None`.
pub fn default_downloads_dir() -> Option<std::path::PathBuf> {
    let home_var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let home = std::env::var_os(home_var)?;
    Some(std::path::PathBuf::from(home).join("Downloads"))
}

/// Sous WSL, ouvre l'explorateur **Windows** (FolderBrowserDialog WinForms)
/// via PowerShell, et retourne le path sélectionné converti en chemin WSL
/// (`C:\Users\foo` → `/mnt/c/Users/foo`).
///
/// Renvoie `None` :
///   - hors WSL (laisse le caller utiliser le picker GTK),
///   - si l'utilisateur annule,
///   - si powershell.exe échoue (pas dispo, timeout, etc.).
///
/// PSARG `-NoProfile -NonInteractive -STA` : démarre vite, pas de profil
/// utilisateur, single-threaded apartment (requis pour WinForms).
/// **Bloquant** : la modale Windows s'ouvre, on attend le retour user.
pub fn wsl_pick_windows_folder(title: &str) -> Option<std::path::PathBuf> {
    if !is_wsl() { return None; }

    // PS échappe simplement les guillemets en doublant. On garde le title
    // simple côté Rust (pas de char non-ASCII problématique en pratique).
    let safe_title = title.replace('"', "\"\"").replace('\'', "''");
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms; \
         $d = New-Object System.Windows.Forms.FolderBrowserDialog; \
         $d.Description = '{safe_title}'; \
         $d.ShowNewFolderButton = $true; \
         if ($d.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {{ \
             Write-Output $d.SelectedPath \
         }}"
    );

    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-STA", "-Command", &script])
        .output().ok()?;
    if !output.status.success() { return None; }

    let win_path = String::from_utf8(output.stdout).ok()?;
    let win_path = win_path.trim();
    if win_path.is_empty() { return None; }

    Some(wsl_convert_win_path(win_path))
}

/// `C:\Users\foo\Bar` → `/mnt/c/Users/foo/Bar`. Tolère `/` ou `\\`.
fn wsl_convert_win_path(win: &str) -> std::path::PathBuf {
    let normalized = win.replace('\\', "/");
    let mut chars = normalized.chars();
    if let (Some(drive), Some(colon)) = (chars.next(), chars.next()) {
        if colon == ':' && drive.is_ascii_alphabetic() {
            let rest: String = chars.collect();
            let rest = rest.trim_start_matches('/');
            return std::path::PathBuf::from(format!(
                "/mnt/{}/{}",
                drive.to_ascii_lowercase(),
                rest,
            ));
        }
    }
    std::path::PathBuf::from(normalized)
}

#[cfg(test)]
mod tests {
    use super::wsl_convert_win_path;

    #[test]
    fn converts_drive_letter() {
        assert_eq!(wsl_convert_win_path("C:\\Users\\foo"),
                   std::path::PathBuf::from("/mnt/c/Users/foo"));
    }
    #[test]
    fn converts_forward_slash_drive() {
        assert_eq!(wsl_convert_win_path("D:/Data/x"),
                   std::path::PathBuf::from("/mnt/d/Data/x"));
    }
    #[test]
    fn passes_through_non_windows() {
        assert_eq!(wsl_convert_win_path("/home/lou/foo"),
                   std::path::PathBuf::from("/home/lou/foo"));
    }
}
