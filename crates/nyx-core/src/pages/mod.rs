//! Rendus HTML purs des pages internes partagées (desktop GTK + Windows) :
//! `nyx://bookmarks` et `nyx://history`. Aucune I/O : les données arrivent
//! déjà lues (store de favoris, entrées d'historique formatées).

pub mod bookmarks;
pub mod history;
