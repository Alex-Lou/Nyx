//! Page `nyx://bookmarks` : favoris groupés par dossier, glisser-déposer
//! côté JS (action interne `nyx://move?idx=…&to=…`).

use crate::state::bookmarks::{folders, Bookmark};
use crate::theme;

const TEMPLATE: &str = include_str!("../../../../assets/bookmarks.html");

/// Page listant les favoris groupés par dossier, avec DnD côté JS.
pub fn page_html(items: &[Bookmark]) -> String {
    let content = if items.is_empty() {
        "<p class=\"empty\">Aucun favori enregistré.</p>".into()
    } else {
        render_groups(items)
    };
    TEMPLATE.replace("{{TOKENS}}", theme::ROOT).replace("{{CONTENT}}", &content)
}

/// Génère un bloc `.folder` par dossier (racine en premier).
fn render_groups(items: &[Bookmark]) -> String {
    let mut out = String::new();
    render_folder(&mut out, items, "");
    for path in folders(items) {
        render_folder(&mut out, items, &path);
    }
    out
}

/// Une section : titre + zone de drop + ses favoris (avec leur index global).
fn render_folder(out: &mut String, items: &[Bookmark], path: &str) {
    let head = if path.is_empty() { "Racine".into() } else { breadcrumb(path) };
    out.push_str(&format!(
        "<section class=\"folder\">\
         <div class=\"folder-head\">{head}</div>\
         <div class=\"divider\"></div>\
         <div class=\"items\" data-folder=\"{}\">",
        attr(path)
    ));
    for (i, b) in items.iter().enumerate().filter(|(_, b)| b.folder == path) {
        let label = if b.title.is_empty() { &b.url } else { &b.title };
        out.push_str(&format!(
            "<div class=\"item\" data-idx=\"{i}\" data-folder=\"{}\">\
             <span class=\"arrow\">→</span>\
             <a class=\"label\" href=\"{}\">{}</a>\
             <span class=\"url\">{}</span></div>",
            attr(path),
            attr(&b.url),
            text(label),
            text(&b.url)
        ));
    }
    out.push_str("</div></section>");
}

/// "Travail/Outils" → "Travail <span class=crumb>/ Outils</span>".
fn breadcrumb(path: &str) -> String {
    let segs: Vec<&str> = path.split('/').collect();
    let mut out = text(segs[0]);
    for s in &segs[1..] {
        out.push_str(&format!("<span class=\"crumb\">/</span>{}", text(s)));
    }
    out
}

fn attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;")
}

fn text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bm(url: &str, title: &str, folder: &str) -> Bookmark {
        Bookmark { url: url.into(), title: title.into(), folder: folder.into() }
    }

    #[test]
    fn empty_page() {
        let html = page_html(&[]);
        assert!(html.contains("Aucun favori"));
        assert!(!html.contains("{{TOKENS}}") && !html.contains("{{CONTENT}}"));
    }

    #[test]
    fn groups_and_escapes() {
        let items = [
            bm("https://a.org/?x=\"><script>", "<b>A</b>", ""),
            bm("https://b.org", "", "Travail/Outils"),
        ];
        let html = page_html(&items);
        assert!(!html.contains("<b>A</b>") && !html.contains("\"><script>"));
        assert!(html.contains("data-idx=\"1\""));
        assert!(html.contains("Travail<span class=\"crumb\">/</span>Outils"));
    }
}
