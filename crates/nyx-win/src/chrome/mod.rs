//! Chrome du navigateur (onglets + barre d'outils) : petite UI HTML/CSS/JS
//! embarquée, servie par le protocole `nyxui` à sa propre WebView.

/// Hauteur logique (px CSS) du chrome : onglets 36 + barre 44.
pub const HEIGHT: f64 = 80.0;

pub const INDEX_HTML: &str = include_str!("index.html");
pub const CSS: &str = include_str!("chrome.css");
pub const JS: &str = include_str!("chrome.js");
