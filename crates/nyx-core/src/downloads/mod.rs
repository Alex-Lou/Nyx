//! Store pur des téléchargements — modèle Vec d'entries, sans I/O ni GTK.
//!
//! Scope strict :
//!   - `id`     : identifiant opaque, monotone (un compteur).
//!   - `entry`  : `DownloadEntry` + `DownloadStatus` (modèle).
//!   - `store`  : CRUD + housekeeping ; jamais lue/écrit sur disque.
//!
//! Ce qu'il ne fait PAS :
//!   - pas d'I/O fichier (la couche browser fait `xdg-open` etc.) ;
//!   - pas de policy (la décision « run safe / ask danger » est dans
//!     [`crate::download_policy`]) ;
//!   - pas de bridge WebKit (à venir Tâche 5). Le store est public et le
//!     bridge appellera `add` / `mark_*` / `update_progress`.
//!
//! Voir docs/security.md §2 « Downloads — state model ».

mod entry;
mod hasher;
mod id;
pub mod quarantine;
mod staging;
mod store;

pub use entry::{DownloadEntry, DownloadStatus};
pub use hasher::{sha256_hex, Sha256Hasher};
pub use id::DownloadId;
pub use staging::{build_temp_path, fresh_random_bytes, RANDOM_BYTES};
pub use store::DownloadStore;

#[cfg(test)]
mod tests;
