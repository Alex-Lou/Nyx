//! Pages internes Nyx (`nyx://newtab`, `nyx://settings`, `nyx://bookmarks`).
//! Génèrent du HTML embarqué ; aucune logique d'état ici.

pub mod bookmarks;
pub mod history;
pub mod newtab;
pub mod settings;
pub mod tokens;

use std::path::Path;
use std::sync::OnceLock;

const ASSETS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");

/// Base URI `file://` du dossier assets — passée à `load_html` pour résoudre
/// les ressources relatives des pages internes. Sprint 6 : chemin d'install.
/// Calculée une fois (appelée à chaque navigation par la frontière de
/// confiance) et percent-encodée comme les URI que WebKit renvoie.
pub fn assets_base_uri() -> &'static str {
    static URI: OnceLock<String> = OnceLock::new();
    URI.get_or_init(|| {
        let p = Path::new(ASSETS_DIR).canonicalize()
            .unwrap_or_else(|_| Path::new(ASSETS_DIR).to_path_buf());
        let uri = gtk::glib::filename_to_uri(&p, None)
            .map(|u| u.to_string())
            .unwrap_or_else(|_| format!("file://{}", p.display()));
        format!("{}/", uri.trim_end_matches('/'))
    })
}
