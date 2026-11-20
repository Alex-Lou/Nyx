//! Historique : quelles navigations enregistrer — pur, testé sur l'hôte.
//!
//! Même règle que le desktop : chargement principal http(s) terminé, pas
//! de doublon consécutif par onglet (reload, redirection), rien en
//! navigation privée. Le titre arrive souvent APRÈS la fin du chargement :
//! il est alors reporté sur la dernière visite (`Vault::retitle_history`).

use std::collections::HashMap;

use nyx_core::url::scheme_of;

use crate::tabs::TabId;

#[derive(Debug, Default)]
pub struct Recorder {
    /// Dernière URL enregistrée par onglet.
    last: HashMap<TabId, String>,
}

/// URL enregistrable : http(s) et pas une page interne.
pub fn recordable(url: &str) -> bool {
    matches!(scheme_of(url).as_deref(), Some("http" | "https")) && !crate::nav::is_internal(url)
}

impl Recorder {
    /// Chargement terminé de `url` dans `tab` : `true` s'il faut l'enregistrer.
    pub fn on_loaded(&mut self, tab: TabId, url: &str, private: bool) -> bool {
        if private || !recordable(url) || self.last.get(&tab).is_some_and(|l| l == url) {
            return false;
        }
        self.last.insert(tab, url.to_string());
        true
    }

    /// Nouveau titre pour la page `url` de `tab` : `true` s'il faut retitrer
    /// sa visite (enregistrée par cet onglet).
    pub fn on_title(&self, tab: TabId, url: &str) -> bool {
        self.last.get(&tab).is_some_and(|l| l == url)
    }

    pub fn forget(&mut self, tab: TabId) {
        self.last.remove(&tab);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_once_per_page() {
        let mut r = Recorder::default();
        assert!(r.on_loaded(1, "https://a.org/", false));
        assert!(!r.on_loaded(1, "https://a.org/", false), "reload");
        assert!(r.on_loaded(2, "https://a.org/", false), "autre onglet");
        assert!(r.on_loaded(1, "https://b.org/", false));
        assert!(r.on_loaded(1, "https://a.org/", false), "retour = nouvelle visite");
    }

    #[test]
    fn skips_private_and_internal() {
        let mut r = Recorder::default();
        assert!(!r.on_loaded(1, "https://a.org/", true));
        for url in ["https://nyx.localhost/newtab", "about:blank", "file:///C:/x", "data:text/html,x", ""] {
            assert!(!r.on_loaded(1, url, false), "{url}");
        }
        assert!(r.on_loaded(1, "http://intranet.local/", false));
    }

    #[test]
    fn titles_follow_last_visit() {
        let mut r = Recorder::default();
        assert!(!r.on_title(1, "https://a.org/"));
        r.on_loaded(1, "https://a.org/", false);
        assert!(r.on_title(1, "https://a.org/"));
        assert!(!r.on_title(1, "https://b.org/"));
        r.forget(1);
        assert!(!r.on_title(1, "https://a.org/"));
    }
}
