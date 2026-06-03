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
    // `cmd /c start "" "<path>"` — "" est le titre vide obligatoire si la
    // cible contient des espaces. Lance dans l'app par défaut.
    let mut c = Command::new("cmd");
    c.args(["/c", "start", ""]).arg(path);
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
