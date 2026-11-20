//! Page historique — chargée sur `nyx://history` (données du vault). Rendu
//! partagé avec Windows dans `nyx_core::pages::history`. Le lien « effacer »
//! passe par `nyx://history/clear`, autorisé uniquement depuis une page
//! interne (voir `web::security`).

use nyx_core::pages::history::{self, Item, CLEAR_HREF};
use vault::HistoryEntry;

pub fn html(entries: &[HistoryEntry]) -> String {
    let items: Vec<Item> = entries
        .iter()
        .map(|e| Item {
            url: e.url.clone(),
            title: e.title.clone(),
            when: e.visited_at.format("%d/%m %H:%M").to_string(),
        })
        .collect();
    history::html(&items, CLEAR_HREF)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn titres_echappes_contre_xss() {
        let entries = vec![HistoryEntry {
            id: Some(1),
            url: "https://x.com/?a=<script>alert(1)</script>".into(),
            title: "<img onerror=x>".into(),
            visited_at: Utc::now(),
        }];
        let html = html(&entries);
        assert!(!html.contains("<script>alert"));
        assert!(!html.contains("<img onerror"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn page_vide() {
        assert!(html(&[]).contains("Aucune visite"));
    }
}
