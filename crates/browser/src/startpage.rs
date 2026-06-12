// Page de démarrage nyx://start — bundlée, animée, zéro réseau.
// La page d'accueil reste configurable : clé `homepage` dans les settings
// du vault (panneau d'options à venir) ; défaut = nyx://start.

use gtk::gio;
use webkit2gtk::{URISchemeRequestExt, WebContext, WebContextExt};

pub const START_PAGE: &str = "nyx://start";

const START_HTML: &str = include_str!("../../../assets/start.html");

/// Enregistre le scheme nyx:// ; toute URL nyx://… sert la page de démarrage.
pub fn register(ctx: &WebContext) {
    ctx.register_uri_scheme("nyx", |request| {
        let bytes = glib::Bytes::from_static(START_HTML.as_bytes());
        let stream = gio::MemoryInputStream::from_bytes(&bytes);
        request.finish(&stream, START_HTML.len() as i64, Some("text/html"));
    });
}
