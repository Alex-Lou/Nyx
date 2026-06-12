//! Réexporte les modules d'état depuis nyx-core + handles GTK locaux.

pub mod downloads;
pub mod downloads_temp;
pub mod vault_sync;

pub use nyx_core::state::bookmarks;
pub use nyx_core::state::settings;
