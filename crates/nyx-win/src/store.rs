//! Pont coffre ⇄ état mémoire (favoris, réglages) — même modèle que le
//! desktop (`browser/src/state/vault_sync.rs`, `main.rs`) : favoris en
//! mémoire resynchronisés en bloc, réglages persistés au format `to_query`.

use nyx_core::nyxguard::NyxGuard;
use nyx_core::state::bookmarks::{Bookmark, Bookmarks};
use nyx_core::state::settings::{self, Settings};
use vault::Vault;

use crate::migrate::SETTINGS_KEY;

/// Favoris du coffre → store mémoire.
pub fn load_bookmarks(bm: &Bookmarks, vault: &Vault) {
    let items = vault.list_bookmarks().unwrap_or_default();
    *bm.borrow_mut() =
        items.into_iter().map(|b| Bookmark { url: b.url, title: b.title, folder: b.folder }).collect();
}

/// Store mémoire → coffre (remplacement complet ; dates de création
/// préservées par URL côté `vault`).
pub fn persist_bookmarks(bm: &Bookmarks, vault: &Vault) {
    let items: Vec<vault::Bookmark> = bm
        .borrow()
        .iter()
        .map(|b| vault::Bookmark::new(b.url.clone(), b.title.clone()).in_folder(b.folder.clone()))
        .collect();
    if let Err(e) = vault.replace_bookmarks(&items) {
        nyx_core::sec_log::emit(nyx_core::sec_log::Level::Warn, &format!("vault favoris : {e}"));
    }
}

/// Réglages du coffre → mémoire + bloqueur.
pub fn load_settings(settings: &Settings, vault: &Vault, guard: &NyxGuard) {
    if let Ok(Some(q)) = vault.setting(SETTINGS_KEY) {
        settings::apply_from_url(&format!("nyx://apply?{q}"), settings, guard);
    }
}

/// Applique `nyx://apply?…` puis persiste.
pub fn apply_settings(url: &str, settings: &Settings, vault: &Vault, guard: &NyxGuard) {
    settings::apply_from_url(url, settings, guard);
    let _ = vault.set_setting(SETTINGS_KEY, &settings::to_query(&settings.borrow()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use nyx_core::state::bookmarks;

    #[test]
    fn round_trips_through_vault() {
        let path = std::env::temp_dir().join(format!("nyx_win_store_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let vault = Vault::open(&path, "pw").expect("vault");
            let guard = NyxGuard::new();

            let bm = bookmarks::new();
            bookmarks::add_in(&bm, "https://a.org/".into(), "A".into(), "Dev".into());
            persist_bookmarks(&bm, &vault);
            let back = bookmarks::new();
            load_bookmarks(&back, &vault);
            assert_eq!(back.borrow().len(), 1);
            assert_eq!(back.borrow()[0].folder, "Dev");

            let s = settings::new();
            apply_settings("nyx://apply?engine=brave&savepw=false", &s, &vault, &guard);
            assert!(!guard.is_enabled(), "adblock absent du formulaire = coupé");
            let fresh = settings::new();
            load_settings(&fresh, &vault, &NyxGuard::new());
            assert_eq!(fresh.borrow().search_engine.id(), "brave");
            assert!(!fresh.borrow().save_passwords);
        }
        let _ = std::fs::remove_file(&path);
    }
}
