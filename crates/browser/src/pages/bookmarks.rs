use crate::state::bookmarks::Bookmark;

/// Page listant les favoris — chargée sur `nyx://bookmarks`.
pub fn page_html(bm: &[Bookmark]) -> String {
    let items = if bm.is_empty() {
        "<p style='color:#7078a8;font-style:italic'>Aucun favori enregistré.</p>".into()
    } else {
        bm.iter()
            .map(|b| format!(
                "<li><a href=\"{url}\" style='color:#c8d6ff;text-decoration:none'>\
                 <span style='color:#4af2c8;font-family:monospace;margin-right:12px'>→</span>\
                 {title}</a>\
                 <span style='color:#3d4466;font-size:11px;margin-left:8px'>{url}</span></li>",
                url = b.url, title = b.title
            ))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(r#"<!doctype html><html><head><meta charset="utf-8">
<title>Favoris — Nyx</title></head>
<body style="margin:0;padding:48px;background:#050510;color:#e8ecff;
font-family:system-ui,-apple-system,sans-serif;min-height:100vh">
<h1 style="color:#4af2c8;font-weight:200;letter-spacing:.1em;margin-bottom:32px">★ Favoris</h1>
<ul style="list-style:none;padding:0;display:flex;flex-direction:column;gap:12px">
{items}
</ul>
</body></html>"#)
}
