use crate::pages::tokens;
use crate::state::bookmarks::{folders, Bookmark};

const TEMPLATE: &str = include_str!("../../../../assets/bookmarks.html");

/// Page listant les favoris groupés par dossier, avec DnD côté JS.
pub fn page_html(items: &[Bookmark]) -> String {
    let content = if items.is_empty() {
        "<p class=\"empty\">Aucun favori enregistré.</p>".into()
    } else {
        render_groups(items)
    };
    TEMPLATE
        .replace("{{TOKENS}}", tokens::ROOT)
        .replace("{{CONTENT}}", &content)
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
            attr(path), attr(&b.url), text(label), text(&b.url)
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

fn attr(s: &str) -> String { s.replace('&', "&amp;").replace('"', "&quot;") }
fn text(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;") }
