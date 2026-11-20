//! Protocoles personnalisés — routage pur (testé sur l'hôte) + adaptation
//! `http::Response` pour wry (Windows uniquement).
//!
//! • `nyx`   → pages internes de contenu (`https://nyx.localhost/…`) ;
//! • `nyxui` → le chrome (`https://nyxui.localhost/…`), enregistré sur la
//!   seule WebView du chrome.
//!
//! wry intercepte tout `https://<protocole>.*` : on ne sert donc que l'hôte
//! exact `localhost`, le reste reçoit un 404. Les pages qui lisent le coffre
//! (réglages, favoris, historique) sont des [`Route::Dynamic`] : rendues par
//! l'app sur le thread UI (protocole asynchrone de wry).

use std::borrow::Cow;
use std::sync::LazyLock;

use nyx_core::security::Page;
use nyx_core::theme;

use crate::chrome;

const NEWTAB_TEMPLATE: &str = include_str!("../../../assets/newtab.html");

static NEWTAB: LazyLock<String> = LazyLock::new(|| theme::render_page(NEWTAB_TEMPLATE));
static CHROME_INDEX: LazyLock<String> = LazyLock::new(|| theme::render_page(chrome::INDEX_HTML));

/// CSP du chrome : uniquement ses propres fichiers (+ la palette inline).
const CHROME_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
                          img-src 'self' data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// CSP des pages internes dynamiques : aucun accès réseau, scripts inline
/// des gabarits seulement, jamais intégrables dans un cadre.
pub const PAGE_CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
                            img-src data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// Réponse d'un protocole interne, indépendante de la crate `http`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub status: u16,
    pub mime: &'static str,
    pub csp: Option<&'static str>,
    pub body: Cow<'static, str>,
}

impl Asset {
    fn ok(mime: &'static str, body: &'static str) -> Self {
        Self { status: 200, mime, csp: None, body: Cow::Borrowed(body) }
    }

    /// Page interne rendue dynamiquement.
    pub fn page(html: String) -> Self {
        Self { status: 200, mime: "text/html; charset=utf-8", csp: Some(PAGE_CSP), body: Cow::Owned(html) }
    }

    pub fn not_found() -> Self {
        Self { status: 404, mime: "text/plain; charset=utf-8", csp: None, body: Cow::Borrowed("Introuvable") }
    }
}

/// Routage du protocole `nyx`.
#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    /// Réponse immédiate (Nouvel onglet, 404).
    Static(Asset),
    /// Page lisant le coffre : rendue par l'app.
    Dynamic(Page),
}

/// Protocole `nyx` : pages internes.
pub fn internal_route(host: &str, path: &str) -> Route {
    if host != "localhost" {
        return Route::Static(Asset::not_found());
    }
    match path.trim_end_matches('/') {
        "" | "/newtab" => Route::Static(Asset::ok("text/html; charset=utf-8", NEWTAB.as_str())),
        "/settings" => Route::Dynamic(Page::Settings),
        "/bookmarks" => Route::Dynamic(Page::Bookmarks),
        "/history" => Route::Dynamic(Page::History),
        _ => Route::Static(Asset::not_found()),
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

/// Requête wry → (hôte, chemin) si GET, sinon `None` (405).
#[cfg(windows)]
pub fn target(request: &wry::http::Request<Vec<u8>>) -> Option<(String, String)> {
    let uri = request.uri();
    (request.method() == wry::http::Method::GET)
        .then(|| (uri.host().unwrap_or("").to_string(), uri.path().to_string()))
}

/// Adaptation wry : `Asset` → réponse HTTP.
#[cfg(windows)]
pub fn to_response(asset: Asset) -> wry::http::Response<Cow<'static, [u8]>> {
    use wry::http::{header, Response};

    let mut builder = Response::builder()
        .status(asset.status)
        .header(header::CONTENT_TYPE, asset.mime)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::CACHE_CONTROL, "no-store");
    if let Some(csp) = asset.csp {
        builder = builder.header(header::CONTENT_SECURITY_POLICY, csp);
    }
    let body: Cow<'static, [u8]> = match asset.body {
        Cow::Borrowed(s) => Cow::Borrowed(s.as_bytes()),
        Cow::Owned(s) => Cow::Owned(s.into_bytes()),
    };
    builder.body(body).unwrap_or_else(|_| Response::new(Cow::Borrowed(&[][..])))
}

/// Réponse 405 (méthode autre que GET).
#[cfg(windows)]
pub fn method_not_allowed() -> Asset {
    Asset { status: 405, ..Asset::not_found() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(route: Route) -> Asset {
        match route {
            Route::Static(a) => a,
            Route::Dynamic(p) => panic!("dynamique : {p:?}"),
        }
    }

    #[test]
    fn newtab_routes() {
        for path in ["/", "", "/newtab", "/newtab/"] {
            let a = body(internal_route("localhost", path));
            assert_eq!(a.status, 200, "{path}");
            assert!(a.body.contains("--nyx-aurora"), "palette injectée");
            assert!(!a.body.contains("{{TOKENS}}"));
        }
        assert_eq!(body(internal_route("evil.com", "/newtab")).status, 404);
        assert_eq!(body(internal_route("localhost", "/../../etc/passwd")).status, 404);
        assert_eq!(body(internal_route("localhost", "/history/clear")).status, 404, "action ≠ page");
    }

    #[test]
    fn dynamic_routes() {
        assert_eq!(internal_route("localhost", "/settings"), Route::Dynamic(Page::Settings));
        assert_eq!(internal_route("localhost", "/bookmarks/"), Route::Dynamic(Page::Bookmarks));
        assert_eq!(internal_route("localhost", "/history"), Route::Dynamic(Page::History));
        assert_eq!(body(internal_route("localhost.evil", "/settings")).status, 404);
        let page = Asset::page("<p>x</p>".into());
        assert!(page.csp.is_some_and(|c| c.contains("default-src 'none'") && c.contains("frame-ancestors 'none'")));
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
