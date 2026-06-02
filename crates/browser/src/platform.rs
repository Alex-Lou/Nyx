//! Détection d'environnement. Sous WSL, deux sous-systèmes WebKit sont cassés
//! (pas de vrai GPU, pas de bus-proxy bubblewrap) → on adapte au démarrage.

/// `true` si on tourne sous WSL (noyau « microsoft »).
pub fn is_wsl() -> bool {
    std::fs::read_to_string("/proc/version")
        .map(|v| v.to_lowercase().contains("microsoft"))
        .unwrap_or(false)
}
