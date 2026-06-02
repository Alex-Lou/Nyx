//! Page de démarrage Nyx — HTML embarqué chargé via `WebView::load_html`
//! quand on ouvre un onglet vide.

const HTML: &str = include_str!("../../../assets/newtab.html");

/// URI utilisée comme base lors du `load_html` — apparaît dans la barre
/// d'adresse pour qu'on sache où on est.
pub const URI: &str = "nyx://newtab";

pub fn html() -> &'static str {
    HTML
}
