//! Bridge WebKit pour les téléchargements — frontière publique unique.
//!
//! Architecture (Single Responsibility) :
//!   - `bridge` : signal handlers WebKit (started / decide-dest / progress
//!     / finished / failed) + pipeline pre-flight / post-flight.
//!   - `io` : disque (sniff 8K, sha256 streaming, atomic move + meta).
//!   - `cleanup` : boot scan + purge 24h (no symlink follow).
//!   - `confirm` : dialog Ask / AskDanger.
//!
//! Tous les modules sont privés : la surface publique est [`install`].

mod bridge;
mod cleanup;
mod confirm;
mod io;

use std::path::PathBuf;
use webkit2gtk::WebContext;

use crate::state::downloads::DownloadsHandle;
use crate::state::settings::Settings;
use crate::ui::toast::ToastHandle;

/// Câble le bridge sur un `WebContext`. Idempotent par contexte (chaque
/// nouvel onglet créant son propre contexte appelle install une fois).
///
/// Effet de bord : crée `temp_root` si absent (mode 0700 Unix) et lance
/// un boot scan qui purge tout fichier > 24h du run précédent.
pub fn install(
    ctx: &WebContext, store: DownloadsHandle, settings: Settings,
    temp_root: PathBuf, toaster: ToastHandle,
) {
    bridge::wire(ctx, store, settings, temp_root, toaster);
}
