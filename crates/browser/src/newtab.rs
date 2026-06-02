use std::path::Path;

const HTML: &str = include_str!("../../../assets/newtab.html");

/// Chemin vers le dossier assets à compile-time — utilisé comme base_uri
/// pour que les chemins relatifs (vidéos, futures icônes) se résolvent
/// via file://. Sprint 6 remplacera cette const par un chemin d'installation.
const ASSETS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");

pub fn html() -> &'static str { HTML }

/// URI de base à passer à `WebView::load_html` pour résoudre les ressources
/// relatives (bg-waves.mp4, bg-nebula.mp4).
pub fn base_uri() -> String {
    // Normalise le chemin pour WSL / Linux : assure qu'il commence par /
    let p = Path::new(ASSETS_DIR)
        .canonicalize()
        .unwrap_or_else(|_| Path::new(ASSETS_DIR).to_path_buf());
    format!("file://{}/", p.display())
}
