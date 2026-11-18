//! Machine à états des onglets — pure et générique sur la vue (`T` =
//! `wry::WebView` sous Windows, `()` dans les tests).
//!
//! Invariant : `active` désigne toujours un onglet existant, ou `None` si la
//! liste est vide. Fermer un onglet rend sa vue à l'appelant, qui la *drop*
//! (libère la WebView2) : aucune référence ne survit ici.

use crate::ipc::TabView;
use crate::nav;

pub type TabId = u32;

/// Longueur max d'un titre affiché (en caractères).
const TITLE_MAX: usize = 60;

#[derive(Debug)]
pub struct Tab<T> {
    pub id: TabId,
    pub view: T,
    /// URL moteur courante (`https://nyx.localhost/…` pour les pages internes).
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

impl<T> Tab<T> {
    /// Titre lisible : titre du document, sinon URL affichable, sinon
    /// « Nouvel onglet ». Tronqué proprement (frontière de caractère).
    pub fn display_title(&self) -> String {
        let title = self.title.trim();
        let url = nav::display_url(&self.url);
        let base = if !title.is_empty() {
            title
        } else if !url.is_empty() {
            url.as_str()
        } else {
            "Nouvel onglet"
        };
        match base.char_indices().nth(TITLE_MAX) {
            Some((cut, _)) => format!("{}…", &base[..cut]),
            None => base.to_string(),
        }
    }
}

#[derive(Debug)]
pub struct Tabs<T> {
    tabs: Vec<Tab<T>>,
    active: Option<TabId>,
    next_id: TabId,
}

impl<T> Default for Tabs<T> {
    fn default() -> Self {
        Self { tabs: Vec::new(), active: None, next_id: 1 }
    }
}

impl<T> Tabs<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Identifiant que recevra le prochain onglet ouvert (les handlers de la
    /// WebView le capturent avant que la vue n'existe).
    pub fn next_id(&self) -> TabId {
        self.next_id
    }

    /// Ajoute un onglet (à droite de l'actif) sans l'activer. Le premier
    /// onglet devient actif d'office.
    pub fn open(&mut self, view: T, url: &str) -> TabId {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let tab = Tab {
            id,
            view,
            url: url.to_string(),
            title: String::new(),
            loading: true,
            can_go_back: false,
            can_go_forward: false,
        };
        let at = self.active_index().map_or(self.tabs.len(), |i| i + 1);
        self.tabs.insert(at, tab);
        if self.active.is_none() {
            self.active = Some(id);
        }
        id
    }

    /// Active `id`. Renvoie l'ancien onglet actif si l'activation a changé
    /// quelque chose (`Some(None)` s'il n'y en avait pas), `None` si `id` est
    /// inconnu ou déjà actif.
    pub fn activate(&mut self, id: TabId) -> Option<Option<TabId>> {
        if self.active == Some(id) || self.index_of(id).is_none() {
            return None;
        }
        Some(self.active.replace(id))
    }

    /// Ferme `id` et rend sa vue. Si c'était l'actif, le voisin de droite
    /// (sinon de gauche) devient actif.
    pub fn close(&mut self, id: TabId) -> Option<T> {
        let idx = self.index_of(id)?;
        let tab = self.tabs.remove(idx);
        if self.active == Some(id) {
            self.active = self
                .tabs
                .get(idx)
                .or_else(|| idx.checked_sub(1).and_then(|i| self.tabs.get(i)))
                .map(|t| t.id);
        }
        Some(tab.view)
    }

    /// Onglet suivant (ou précédent) de façon circulaire — Ctrl+Tab.
    pub fn neighbour(&self, forward: bool) -> Option<TabId> {
        let len = self.tabs.len();
        let i = self.active_index()?;
        let j = if forward { (i + 1) % len } else { (i + len - 1) % len };
        Some(self.tabs[j].id)
    }

    pub fn active_id(&self) -> Option<TabId> {
        self.active
    }

    pub fn active(&self) -> Option<&Tab<T>> {
        self.active.and_then(|id| self.get(id))
    }

    pub fn get(&self, id: TabId) -> Option<&Tab<T>> {
        self.tabs.iter().find(|t| t.id == id)
    }

    pub fn get_mut(&mut self, id: TabId) -> Option<&mut Tab<T>> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Tab<T>> {
        self.tabs.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Vue sérialisable de la barre d'onglets.
    pub fn snapshot(&self) -> Vec<TabView> {
        self.tabs
            .iter()
            .map(|t| TabView {
                id: t.id,
                title: t.display_title(),
                active: self.active == Some(t.id),
                loading: t.loading,
            })
            .collect()
    }

    fn index_of(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    fn active_index(&self) -> Option<usize> {
        self.active.and_then(|id| self.index_of(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(t: &Tabs<()>) -> Vec<TabId> {
        t.iter().map(|t| t.id).collect()
    }

    #[test]
    fn first_tab_is_active() {
        let mut t = Tabs::new();
        assert!(t.is_empty() && t.active().is_none());
        assert_eq!(t.next_id(), 1);
        let a = t.open((), nav::NEWTAB_URL);
        assert_eq!(t.active_id(), Some(a));
        let b = t.open((), "https://b.example/");
        assert_eq!(t.active_id(), Some(a), "ouvrir n'active pas");
        assert_ne!(a, b);
        assert_eq!(t.iter().count(), 2);
    }

    #[test]
    fn opens_right_of_active() {
        let mut t = Tabs::new();
        let a = t.open((), "");
        let b = t.open((), "");
        t.activate(a);
        let c = t.open((), "");
        assert_eq!(ids(&t), vec![a, c, b]);
    }

    #[test]
    fn activate_reports_changes() {
        let mut t = Tabs::new();
        let a = t.open((), "");
        let b = t.open((), "");
        assert_eq!(t.activate(a), None, "déjà actif");
        assert_eq!(t.activate(42), None, "inconnu");
        assert_eq!(t.activate(b), Some(Some(a)));
        assert_eq!(t.active_id(), Some(b));
    }

    #[test]
    fn close_selects_neighbour() {
        let mut t = Tabs::new();
        let a = t.open((), "");
        let b = t.open((), "");
        let c = t.open((), "");
        // ordre : a, c, b (insertion à droite de l'actif a)
        t.activate(c);
        assert!(t.close(c).is_some());
        assert_eq!(t.active_id(), Some(b), "voisin de droite");
        assert!(t.close(b).is_some());
        assert_eq!(t.active_id(), Some(a), "puis de gauche");
        assert!(t.close(a).is_some());
        assert_eq!(t.active_id(), None);
        assert!(t.is_empty());
        assert!(t.close(a).is_none(), "double fermeture inoffensive");
    }

    #[test]
    fn close_inactive_keeps_active() {
        let mut t = Tabs::new();
        let a = t.open((), "");
        let b = t.open((), "");
        assert!(t.close(b).is_some());
        assert_eq!(t.active_id(), Some(a));
    }

    #[test]
    fn neighbour_cycles() {
        let mut t: Tabs<()> = Tabs::new();
        assert_eq!(t.neighbour(true), None);
        let a = t.open((), "");
        assert_eq!(t.neighbour(true), Some(a));
        let b = t.open((), "");
        assert_eq!(t.neighbour(true), Some(b));
        assert_eq!(t.neighbour(false), Some(b));
        t.activate(b);
        assert_eq!(t.neighbour(true), Some(a));
    }

    #[test]
    fn ids_are_never_reused() {
        let mut t = Tabs::new();
        let a = t.open((), "");
        t.close(a);
        let b = t.open((), "");
        assert_ne!(a, b);
    }

    #[test]
    fn titles() {
        let mut t = Tabs::new();
        let a = t.open((), nav::NEWTAB_URL);
        assert_eq!(t.get(a).map(Tab::display_title).as_deref(), Some("Nouvel onglet"));
        let tab = t.get_mut(a).expect("onglet");
        tab.url = "https://rust-lang.org/".into();
        assert_eq!(tab.display_title(), "https://rust-lang.org/");
        tab.title = "  Rust  ".into();
        assert_eq!(tab.display_title(), "Rust");
        tab.title = "é".repeat(200);
        let shown = tab.display_title();
        assert_eq!(shown.chars().count(), TITLE_MAX + 1);
        assert!(shown.ends_with('…'));

        let snap = t.snapshot();
        assert_eq!(snap.len(), 1);
        assert!(snap[0].active && snap[0].loading);
    }
}
