// Sprint 3 — intégration complète avec la crate `adblock` de Brave
// Pour l'instant : liste de règles en dur pour valider le pipeline.

/// Règle : domaine (+ préfixe de chemin optionnel).
/// Le matching se fait sur le host réel de l'URL, pas en substring,
/// pour éviter les faux positifs ("notdoubleclick.net") et les
/// contournements ("https://evil.com/?x=doubleclick.net").
struct Rule {
    domain: &'static str,
    path_prefix: Option<&'static str>,
}

pub struct AdBlocker {
    rules: Vec<Rule>,
}

impl AdBlocker {
    pub fn new() -> Self {
        // TODO Sprint 3: charger les règles depuis EasyList / uBlock Origin
        // via la crate `adblock = "0.9"` et un fichier de règles embarqué.
        let rules = [
            "doubleclick.net",
            "googlesyndication.com",
            "ads.twitter.com",
            "facebook.com/tr",
            "analytics.google.com",
            "hotjar.com",
        ]
        .iter()
        .map(|pattern| match pattern.split_once('/') {
            Some((domain, path)) => Rule { domain, path_prefix: Some(path) },
            None => Rule { domain: pattern, path_prefix: None },
        })
        .collect();

        Self { rules }
    }

    /// Retourne true si l'URL doit être bloquée.
    pub fn should_block(&self, url: &str) -> bool {
        let (host, path) = crate::urls::host_and_path(url);

        self.rules.iter().any(|rule| {
            host_matches(host, rule.domain)
                && rule.path_prefix.is_none_or(|prefix| path.starts_with(prefix))
        })
    }
}

/// true si `host` est `domain` ou un sous-domaine de `domain`.
fn host_matches(host: &str, domain: &str) -> bool {
    host.strip_suffix(domain)
        .is_some_and(|rest| rest.is_empty() || rest.ends_with('.'))
}

impl Default for AdBlocker {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bloque_domaine_et_sous_domaines() {
        let b = AdBlocker::new();
        assert!(b.should_block("https://doubleclick.net/ads"));
        assert!(b.should_block("https://stats.doubleclick.net/x"));
        assert!(b.should_block("https://hotjar.com"));
    }

    #[test]
    fn ne_bloque_pas_les_faux_positifs() {
        let b = AdBlocker::new();
        assert!(!b.should_block("https://notdoubleclick.net/page"));
        assert!(!b.should_block("https://example.com/?ref=doubleclick.net"));
        assert!(!b.should_block("https://duckduckgo.com"));
    }

    #[test]
    fn regle_avec_chemin() {
        let b = AdBlocker::new();
        assert!(b.should_block("https://facebook.com/tr?id=123"));
        assert!(b.should_block("https://www.facebook.com/tr/"));
        assert!(!b.should_block("https://facebook.com/profile"));
    }
}
