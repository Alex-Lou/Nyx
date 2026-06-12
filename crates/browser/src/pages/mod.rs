//! Pages internes Nyx (`nyx://newtab`, `nyx://settings`, `nyx://bookmarks`).
//! Génèrent du HTML embarqué ; aucune logique d'état ici.

pub mod bookmarks;
pub mod history;
pub mod newtab;
pub mod settings;
pub mod tokens;

use std::path::Path;

const ASSETS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");

/// Base URI `file://` du dossier assets — passée à `load_html` pour résoudre
/// les ressources relatives des pages internes. Sprint 6 : chemin d'install.
pub fn assets_base_uri() -> String {
    let p = Path::new(ASSETS_DIR)
        .canonicalize()
        .unwrap_or_else(|_| Path::new(ASSETS_DIR).to_path_buf());
    format!("file://{}/", p.display())
}
