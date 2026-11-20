//! Page `nyx://history`. Le lien « Tout effacer » mène à l'action interne
//! `history/clear`, autorisée uniquement depuis une page interne
//! (`security::decide` → `Verdict::ClearHistory`).

use crate::theme;
use crate::url::html_escape as escape;

/// Une visite à afficher (date déjà formatée par l'appelant).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub url: String,
    pub title: String,
    pub when: String,
}

/// URL de l'action « Tout effacer » côté desktop (scheme `nyx://`).
pub const CLEAR_HREF: &str = "nyx://history/clear";

/// Rend la page. `clear_href` : URL de l'action d'effacement telle que le
/// moteur la voit (`nyx://history/clear` sous WebKit, origine interne
/// `https://nyx.localhost/history/clear` sous WebView2).
pub fn html(entries: &[Item], clear_href: &str) -> String {
    let items = if entries.is_empty() {
        "<p style='color:var(--nyx-dim);font-style:italic'>Aucune visite enregistrée.</p>".into()
    } else {
        entries
            .iter()
            .map(|e| {
                let title = if e.title.is_empty() { &e.url } else { &e.title };
                format!(
                    "<li><a href=\"{url}\" style='color:var(--nyx-star);text-decoration:none'>\
                     <span style='color:var(--nyx-aurora);font-family:monospace;margin-right:12px'>→</span>\
                     {title}</a>\
                     <span style='color:var(--nyx-ghost);font-size:11px;margin-left:8px'>{url} · {when}</span></li>",
                    url = escape(&e.url),
                    title = escape(title),
                    when = escape(&e.when),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        r#"<!doctype html><html><head><meta charset="utf-8">
<title>Historique — Nyx</title>{tokens}</head>
<body style="margin:0;padding:48px;background:var(--nyx-void);color:var(--nyx-text);
font-family:system-ui,-apple-system,sans-serif;min-height:100vh">
<div style="display:flex;align-items:baseline;justify-content:space-between;max-width:860px">
<h1 style="color:var(--nyx-aurora);font-weight:200;letter-spacing:.1em;margin-bottom:32px">⌛ Historique</h1>
<a href="{clear}" style="color:var(--nyx-twilight);font-size:12px;text-decoration:none">Tout effacer</a>
</div>
<ul style="list-style:none;padding:0;display:flex;flex-direction:column;gap:12px;max-width:860px">
{items}
</ul>
</body></html>"#,
        tokens = theme::ROOT,
        clear = escape(clear_href),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titres_echappes_contre_xss() {
        let entries = vec![Item {
            url: "https://x.com/?a=<script>alert(1)</script>".into(),
            title: "<img onerror=x>".into(),
            when: "01/02 03:04".into(),
        }];
        let html = html(&entries, CLEAR_HREF);
        assert!(!html.contains("<script>alert"));
        assert!(!html.contains("<img onerror"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("01/02 03:04"));
        assert!(html.contains("href=\"nyx://history/clear\""));
    }

    #[test]
    fn page_vide() {
        assert!(html(&[], CLEAR_HREF).contains("Aucune visite"));
        assert!(html(&[], "https://nyx.localhost/history/clear").contains("https://nyx.localhost/history/clear"));
    }
}
