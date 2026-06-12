use crate::pages::tokens;
use vault::HistoryEntry;

/// Page historique — chargée sur `nyx://history` (données du vault).
/// Le lien « effacer » passe par `nyx://history/clear`, autorisé uniquement
/// depuis une page interne (voir `web::security`).
pub fn html(entries: &[HistoryEntry]) -> String {
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
                    when = e.visited_at.format("%d/%m %H:%M"),
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
<a href="nyx://history/clear" style="color:var(--nyx-twilight);font-size:12px;text-decoration:none">Tout effacer</a>
</div>
<ul style="list-style:none;padding:0;display:flex;flex-direction:column;gap:12px;max-width:860px">
{items}
</ul>
</body></html>"#,
        tokens = tokens::ROOT
    )
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn titres_echappes_contre_xss() {
        let entries = vec![HistoryEntry {
            id: Some(1),
            url: "https://x.com/?a=<script>alert(1)</script>".into(),
            title: "<img onerror=x>".into(),
            visited_at: Utc::now(),
        }];
        let html = html(&entries);
        assert!(!html.contains("<script>alert"));
        assert!(!html.contains("<img onerror"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn page_vide() {
        assert!(html(&[]).contains("Aucune visite"));
    }
}
