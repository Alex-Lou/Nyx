//! Protocoles personnalisés — routage pur (testé sur l'hôte) + adaptation
//! `http::Response` pour wry (Windows uniquement).
//!
//! • `nyx`   → pages internes de contenu (`https://nyx.localhost/…`) ;
//! • `nyxui` → le chrome (`https://nyxui.localhost/…`), enregistré sur la
//!   seule WebView du chrome.
//!
//! wry intercepte tout `https://<protocole>.*` : on ne sert donc que l'hôte
//! exact `localhost`, le reste reçoit un 404.

use std::sync::LazyLock;

use nyx_core::theme;

use crate::chrome;

const NEWTAB_TEMPLATE: &str = include_str!("../../../assets/newtab.html");

static NEWTAB: LazyLock<String> = LazyLock::new(|| theme::render_page(NEWTAB_TEMPLATE));
static CHROME_INDEX: LazyLock<String> = LazyLock::new(|| theme::render_page(chrome::INDEX_HTML));

/// CSP du chrome : uniquement ses propres fichiers (+ la palette inline).
const CHROME_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
                          img-src 'self' data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// Réponse d'un protocole interne, indépendante de la crate `http`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub status: u16,
    pub mime: &'static str,
    pub csp: Option<&'static str>,
    pub body: &'static str,
}

impl Asset {
    fn ok(mime: &'static str, body: &'static str) -> Self {
        Self { status: 200, mime, csp: None, body }
    }

    fn not_found() -> Self {
        Self { status: 404, mime: "text/plain; charset=utf-8", csp: None, body: "Introuvable" }
    }
}

/// Protocole `nyx` : phase 1, toute page interne connue = Nouvel onglet (la
/// navigation vers les autres est déjà redirigée par `nav::decide`).
pub fn internal_page(host: &str, path: &str) -> Asset {
    if host != "localhost" {
        return Asset::not_found();
    }
    match path.trim_end_matches('/') {
        "" | "/newtab" => Asset::ok("text/html; charset=utf-8", NEWTAB.as_str()),
        _ => Asset::not_found(),
    }
}

/// Protocole `nyxui` : fichiers du chrome.
pub fn chrome_asset(host: &str, path: &str) -> Asset {
    if host != "localhost" {
        return Asset::not_found();
    }
    let mut asset = match path {
        "/" | "/index.html" => Asset::ok("text/html; charset=utf-8", CHROME_INDEX.as_str()),
        "/chrome.css" => Asset::ok("text/css; charset=utf-8", chrome::CSS),
        "/chrome.js" => Asset::ok("text/javascript; charset=utf-8", chrome::JS),
        _ => return Asset::not_found(),
    };
    asset.csp = Some(CHROME_CSP);
    asset
}

/// Adaptation wry : requête → réponse HTTP.
#[cfg(windows)]
pub fn respond(
    route: fn(&str, &str) -> Asset,
    request: &wry::http::Request<Vec<u8>>,
) -> wry::http::Response<std::borrow::Cow<'static, [u8]>> {
    use std::borrow::Cow;
    use wry::http::{header, Method, Response};

    let uri = request.uri();
    let asset = if request.method() == Method::GET {
        route(uri.host().unwrap_or(""), uri.path())
    } else {
        Asset { status: 405, ..Asset::not_found() }
    };
    let mut builder = Response::builder()
        .status(asset.status)
        .header(header::CONTENT_TYPE, asset.mime)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::CACHE_CONTROL, "no-store");
    if let Some(csp) = asset.csp {
        builder = builder.header(header::CONTENT_SECURITY_POLICY, csp);
    }
    let body = Cow::Borrowed(asset.body.as_bytes());
    builder.body(body).unwrap_or_else(|_| Response::new(Cow::Borrowed(&[][..])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newtab_routes() {
        for path in ["/", "", "/newtab", "/newtab/"] {
            let a = internal_page("localhost", path);
            assert_eq!(a.status, 200, "{path}");
            assert!(a.body.contains("--nyx-aurora"), "palette injectée");
            assert!(!a.body.contains("{{TOKENS}}"));
        }
        assert_eq!(internal_page("localhost", "/settings").status, 404);
        assert_eq!(internal_page("evil.com", "/newtab").status, 404);
        assert_eq!(internal_page("localhost", "/../../etc/passwd").status, 404);
    }

    #[test]
    fn chrome_routes() {
        let index = chrome_asset("localhost", "/index.html");
        assert_eq!(index.status, 200);
        assert!(index.body.contains("--nyx-surface") && !index.body.contains("{{TOKENS}}"));
        assert!(index.csp.is_some_and(|c| c.contains("default-src 'none'")));
        assert_eq!(chrome_asset("localhost", "/chrome.js").mime, "text/javascript; charset=utf-8");
        assert_eq!(chrome_asset("localhost", "/chrome.css").status, 200);
        assert_eq!(chrome_asset("localhost", "/newtab").status, 404);
        assert_eq!(chrome_asset("localhost.evil", "/").status, 404);
    }
}
