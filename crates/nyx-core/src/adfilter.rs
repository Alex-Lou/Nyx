//! Filtre publicitaire production — moteur `adblock` de Brave nourri par
//! les listes EasyList + EasyPrivacy (gzippées, ~82 000 règles).
//!
//! Source unique partagée par le desktop (`browser::web::adfilter`, listes
//! embarquées via `include_bytes!`) et Android (`nyx-ffi`, listes lues dans
//! les assets de l'APK puis passées en octets).
//!
//! **Thread-safety** : la crate `adblock` est compilée SANS sa feature
//! `single-thread` (cf. Cargo.toml), donc `Engine: Send + Sync` — ce qui
//! permet à Android de partager un seul moteur entre les threads réseau de
//! `WebViewClient.shouldInterceptRequest`. Aucune crate du workspace ne doit
//! réactiver les features par défaut d'`adblock` (l'unification de features
//! rendrait le moteur `!Send`).

use std::io::Read;

use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::Engine;

/// Moteur EasyList/EasyPrivacy prêt à interroger.
pub struct AdFilter {
    engine: Engine,
}

impl AdFilter {
    /// Construit le moteur depuis des listes au format texte ABP.
    pub fn from_lists<'a>(lists: impl IntoIterator<Item = &'a str>) -> Self {
        let mut set = FilterSet::new(false);
        for list in lists {
            set.add_filters(list.lines(), ParseOptions::default());
        }
        Self { engine: Engine::from_filter_set(set, true) }
    }

    /// Construit le moteur depuis des listes gzippées (format des fichiers
    /// `assets/filterlists/*.txt.gz`). Une liste corrompue est ignorée —
    /// jamais de panique : un filtre vide vaut mieux qu'un navigateur mort.
    pub fn from_gz_lists<'a>(lists: impl IntoIterator<Item = &'a [u8]>) -> Self {
        let texts: Vec<String> = lists.into_iter().map(gunzip).collect();
        Self::from_lists(texts.iter().map(String::as_str))
    }

    /// `true` si la requête `url` (chargée par la page `source_url`, de type
    /// `resource_type` au sens adblock : `script`, `image`, `stylesheet`,
    /// `xmlhttprequest`, `subdocument`, `document`, `other`…) matche les
    /// règles. `source_url` vide = requête de premier niveau.
    pub fn should_block(&self, url: &str, source_url: &str, resource_type: &str) -> bool {
        let source = if source_url.is_empty() { url } else { source_url };
        Request::new(url, source, resource_type)
            .map(|r| self.engine.check_network_request(&r).matched)
            .unwrap_or(false)
    }
}

/// Déduit le type de ressource adblock d'une requête dont le moteur web ne
/// donne pas le type (Android `WebResourceRequest`) : en-tête `Accept` puis
/// extension du chemin. Pur, sans état.
pub fn infer_resource_type(url: &str, accept: &str) -> &'static str {
    let accept = accept.to_ascii_lowercase();
    if accept.starts_with("text/html") || accept.contains("application/xhtml") {
        return "subdocument";
    }
    if accept.starts_with("image/") {
        return "image";
    }
    if accept.starts_with("text/css") {
        return "stylesheet";
    }
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let ext = path.rsplit_once('/').map_or(path.as_str(), |(_, f)| f)
        .rsplit_once('.')
        .map_or("", |(_, e)| e);
    match ext {
        "js" | "mjs" => "script",
        "css" => "stylesheet",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "svg" | "ico" | "bmp" => "image",
        "woff" | "woff2" | "ttf" | "otf" | "eot" => "font",
        "mp4" | "webm" | "mp3" | "ogg" | "m3u8" | "m4a" => "media",
        "htm" | "html" => "subdocument",
        _ if accept.contains("json") => "xmlhttprequest",
        _ => "other",
    }
}

fn gunzip(data: &[u8]) -> String {
    let mut out = String::new();
    if flate2::read::GzDecoder::new(data).read_to_string(&mut out).is_err() {
        out.clear();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small() -> AdFilter {
        AdFilter::from_lists([
            "||doubleclick.net^\n/pagead/ads?\n||tracker.example^$third-party",
            "||pixel.example^$image",
        ])
    }

    #[test]
    fn engine_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AdFilter>();
    }

    #[test]
    fn blocks_listed() {
        let f = small();
        assert!(f.should_block("https://ad.doubleclick.net/x.js", "https://news.example/", "script"));
        assert!(f.should_block("https://a.example/pagead/ads?c=1", "", "script"));
        assert!(!f.should_block("https://notdoubleclick.net/", "", "document"));
        assert!(!f.should_block("https://example.com/?ref=doubleclick.net", "", "document"));
    }

    #[test]
    fn third_party_and_type_options() {
        let f = small();
        assert!(f.should_block("https://tracker.example/t.js", "https://site.example/", "script"));
        assert!(!f.should_block("https://tracker.example/t.js", "https://tracker.example/", "script"));
        assert!(f.should_block("https://pixel.example/p.gif", "https://site.example/", "image"));
        assert!(!f.should_block("https://pixel.example/p.js", "https://site.example/", "script"));
    }

    #[test]
    fn garbage_never_panics() {
        let f = AdFilter::from_gz_lists([b"not gzip".as_slice(), &[]]);
        assert!(!f.should_block("https://doubleclick.net/", "", "script"));
        assert!(!f.should_block("not a url", "also not", "???"));
    }

    #[test]
    fn infer_types() {
        assert_eq!(infer_resource_type("https://x/a.js?v=1", "*/*"), "script");
        assert_eq!(infer_resource_type("https://x/a", "image/webp,*/*"), "image");
        assert_eq!(infer_resource_type("https://x/a", "text/css,*/*;q=0.1"), "stylesheet");
        assert_eq!(infer_resource_type("https://x/frame", "text/html,application/xhtml+xml"), "subdocument");
        assert_eq!(infer_resource_type("https://x/F.WOFF2", ""), "font");
        assert_eq!(infer_resource_type("https://x/api", "application/json"), "xmlhttprequest");
        assert_eq!(infer_resource_type("https://x.com/", ""), "other");
        assert_eq!(infer_resource_type("", ""), "other");
    }
}
