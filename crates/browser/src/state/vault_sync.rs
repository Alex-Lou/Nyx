//! Pont store mémoire ↔ vault chiffré (le TODO « Sprint 2 : persistance »
//! de `nyx_core::state::bookmarks`). Vit côté browser : nyx-core reste pur,
//! sans dépendance au stockage.

use std::rc::Rc;

use vault::Vault;

use crate::state::bookmarks::{Bookmark, Bookmarks};

/// Charge les favoris depuis le vault (au déverrouillage).
pub fn load_bookmarks(bm: &Bookmarks, vault: &Rc<Vault>) {
    let items = vault.list_bookmarks().unwrap_or_default();
    *bm.borrow_mut() = items
        .into_iter()
        .map(|b| Bookmark { url: b.url, title: b.title, folder: b.folder })
        .collect();
}

/// Resynchronise le vault avec le store mémoire (après add/remove/move/
/// import — remplacement complet : les listes restent petites, la
/// simplicité prime).
pub fn persist_bookmarks(bm: &Bookmarks, vault: &Rc<Vault>) {
    let items: Vec<vault::Bookmark> = bm
        .borrow()
        .iter()
        .map(|b| {
            vault::Bookmark::new(b.url.clone(), b.title.clone()).in_folder(b.folder.clone())
        })
        .collect();
    let _ = vault.replace_bookmarks(&items);
}
