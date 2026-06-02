// Sprint 3 — intégration complète avec la crate `adblock` de Brave
// Pour l'instant : liste de domaines bloqués en dur pour valider le pipeline.

use std::collections::HashSet;

pub struct AdBlocker {
    blocked_domains: HashSet<String>,
}

impl AdBlocker {
    pub fn new() -> Self {
        // TODO Sprint 3: charger les règles depuis EasyList / uBlock Origin
        // via la crate `adblock = "0.9"` et un fichier de règles embarqué.
        let blocked_domains = [
            "doubleclick.net",
            "googlesyndication.com",
            "ads.twitter.com",
            "facebook.com/tr",
            "analytics.google.com",
            "hotjar.com",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        Self { blocked_domains }
    }

    /// Retourne true si l'URL doit être bloquée.
    pub fn should_block(&self, url: &str) -> bool {
        self.blocked_domains
            .iter()
            .any(|domain| url.contains(domain.as_str()))
    }
}

impl Default for AdBlocker {
    fn default() -> Self { Self::new() }
}
