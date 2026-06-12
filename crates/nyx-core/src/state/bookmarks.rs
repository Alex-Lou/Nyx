use std::cell::RefCell;
use std::rc::Rc;

/// Store en mémoire des favoris. Modèle **path-based** : chaque favori connaît
/// son dossier ("" = racine, "Travail/Outils" = sous-chemin). Évite la refonte
/// arbre tout en supportant dossiers + DnD (le déplacement = changer le path).
pub type Bookmarks = Rc<RefCell<Vec<Bookmark>>>;

#[derive(Clone, Default)]
pub struct Bookmark {
    pub url:    String,
    pub title:  String,
    pub folder: String,
}

pub fn new() -> Bookmarks {
    Rc::new(RefCell::new(Vec::new()))
}

/// Ajoute un favori à la racine (dédup par URL). Retourne `true` si ajouté.
pub fn add(bm: &Bookmarks, url: String, title: String) -> bool {
    add_in(bm, url, title, String::new())
}

/// Ajoute un favori dans le dossier `folder` (vide = racine). Dédup par URL.
pub fn add_in(bm: &Bookmarks, url: String, title: String, folder: String) -> bool {
    if url.is_empty() {
        return false;
    }
    let mut list = bm.borrow_mut();
    if list.iter().any(|b| b.url == url) {
        return false;
    }
    list.push(Bookmark { url, title, folder });
    true
}

pub fn remove(bm: &Bookmarks, index: usize) {
    let mut list = bm.borrow_mut();
    if index < list.len() {
        list.remove(index);
    }
}

/// Déplace un favori dans un nouveau dossier (no-op si l'index est hors borne).
pub fn move_to(bm: &Bookmarks, index: usize, folder: String) {
    if let Some(b) = bm.borrow_mut().get_mut(index) {
        b.folder = folder;
    }
}

/// Liste triée et dédupliquée des chemins de dossiers présents (hors racine).
pub fn folders(items: &[Bookmark]) -> Vec<String> {
    let mut out: Vec<String> = items.iter()
        .map(|b| b.folder.clone())
        .filter(|f| !f.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

// ── Import / Export — format Netscape (universel : Chrome/Firefox/Edge…) ────

/// Sérialise au format Netscape Bookmark, dossiers imbriqués (DL/DT/H3).
pub fn export_netscape(items: &[Bookmark]) -> String {
    let mut s = String::from(
        "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n\
         <META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">\n\
         <TITLE>Bookmarks</TITLE>\n<H1>Bookmarks</H1>\n",
    );
    write_folder(&mut s, items, "", 0);
    s
}

/// Écrit récursivement les favoris du chemin `path`, indent par profondeur.
fn write_folder(out: &mut String, items: &[Bookmark], path: &str, depth: usize) {
    let pad = "    ".repeat(depth);
    out.push_str(&format!("{pad}<DL><p>\n"));

    // Favoris directement dans ce dossier.
    let inner = "    ".repeat(depth + 1);
    for b in items.iter().filter(|b| b.folder == path) {
        out.push_str(&format!(
            "{inner}<DT><A HREF=\"{}\">{}</A>\n",
            escape(&b.url), escape(&b.title)
        ));
    }

    // Sous-dossiers : segment direct après `path`.
    let mut subs: Vec<&str> = items.iter()
        .filter_map(|b| sub_segment(&b.folder, path))
        .collect();
    subs.sort();
    subs.dedup();
    for sub in subs {
        let sub_path = if path.is_empty() { sub.to_string() } else { format!("{path}/{sub}") };
        out.push_str(&format!("{inner}<DT><H3>{}</H3>\n", escape(sub)));
        write_folder(out, items, &sub_path, depth + 1);
    }

    out.push_str(&format!("{pad}</DL><p>\n"));
}

/// Si `folder` commence par `path`, retourne le segment direct suivant.
/// Ex: sub_segment("a/b/c", "a") = Some("b"), sub_segment("a", "a") = None.
fn sub_segment<'a>(folder: &'a str, path: &str) -> Option<&'a str> {
    let rest = if path.is_empty() {
        folder
    } else {
        folder.strip_prefix(path)?.strip_prefix('/')?
    };
    if rest.is_empty() { None } else { Some(rest.split('/').next().unwrap_or(rest)) }
}

/// Parse un export Netscape (Chrome/Firefox/Edge…) avec sa hiérarchie de dossiers.
pub fn import_netscape(html: &str) -> Vec<Bookmark> {
    let mut out = Vec::new();
    let mut stack: Vec<String> = Vec::new();   // chemin courant (segments)
    let mut pending: Option<String> = None;    // dernier H3 vu, en attente de son <DL>
    let mut i = 0;

    while i < html.len() {
        // On cherche le prochain token utile : <H3>, <DL, </DL, <A
        let next = [
            find_ci(&html[i..], "<h3"),
            find_ci(&html[i..], "<dl"),
            find_ci(&html[i..], "</dl"),
            find_ci(&html[i..], "<a "),
        ];
        let Some((kind, rel)) = next.iter().enumerate()
            .filter_map(|(k, o)| o.map(|r| (k, r)))
            .min_by_key(|(_, r)| *r)
        else { break };
        let pos = i + rel;

        match kind {
            0 => { // <H3> : nom de dossier en attente.
                if let Some((name, after)) = read_tag_text(&html[pos..], "h3") {
                    pending = Some(name);
                    i = pos + after;
                } else { i = pos + 3; }
            }
            1 => { // <DL : on rentre dans un dossier (push le pending).
                if let Some(name) = pending.take() {
                    stack.push(name);
                }
                i = pos + 3;
            }
            2 => { // </DL : on sort.
                stack.pop();
                pending = None;
                i = pos + 4;
            }
            _ => { // <A> : un favori.
                if let Some((url, title, after)) = read_anchor(&html[pos..]) {
                    if !url.is_empty() {
                        out.push(Bookmark { url, title, folder: stack.join("/") });
                    }
                    i = pos + after;
                } else { i = pos + 3; }
            }
        }
    }
    out
}

/// Extrait le texte entre `<tag ...>` et `</tag>`. Retourne (texte, offset après).
fn read_tag_text(s: &str, tag: &str) -> Option<(String, usize)> {
    let gt = s.find('>')?;
    let close = format!("</{tag}");
    let end_rel = find_ci(&s[gt + 1..], &close)?;
    Some((unescape(s[gt + 1..gt + 1 + end_rel].trim()), gt + 1 + end_rel))
}

/// Extrait (url, titre, offset après) d'un `<A HREF="..."  >Titre</A>`.
fn read_anchor(s: &str) -> Option<(String, String, usize)> {
    let hrel = find_ci(s, "href=\"")?;
    let us = hrel + 6;
    let ue = s[us..].find('"')?;
    let url = unescape(&s[us..us + ue]);
    let gt = s[us + ue..].find('>')?;
    let ts = us + ue + gt + 1;
    let lt = s[ts..].find('<')?;
    let title = unescape(s[ts..ts + lt].trim());
    Some((url, title, ts + lt))
}

/// Recherche ASCII insensible à la casse (sûre UTF-8 : un octet de séquence
/// multi-octets a son bit de poids fort à 1, il ne matche jamais l'ASCII).
fn find_ci(haystack: &str, needle_lower: &str) -> Option<usize> {
    let (h, n) = (haystack.as_bytes(), needle_lower.as_bytes());
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (0..=h.len() - n.len())
        .find(|&start| (0..n.len()).all(|k| h[start + k].to_ascii_lowercase() == n[k]))
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"")
     .replace("&#39;", "'").replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_dedups() {
        let bm = new();
        assert!(add(&bm, "https://x.com".into(), "X".into()));
        assert!(!add(&bm, "https://x.com".into(), "X again".into()));
        assert!(!add(&bm, "".into(), "vide".into()));
        assert_eq!(bm.borrow().len(), 1);
    }

    #[test]
    fn add_in_folder() {
        let bm = new();
        assert!(add_in(&bm, "https://a.com".into(), "A".into(), "Travail".into()));
        assert_eq!(bm.borrow()[0].folder, "Travail");
    }

    #[test]
    fn move_to_changes_folder() {
        let bm = new();
        add(&bm, "https://a.com".into(), "A".into());
        move_to(&bm, 0, "Persos".into());
        assert_eq!(bm.borrow()[0].folder, "Persos");
        move_to(&bm, 99, "X".into()); // hors borne = no-op
    }

    #[test]
    fn folders_lists_unique_sorted() {
        let bm = new();
        add_in(&bm, "https://a.com".into(), "A".into(), "Z".into());
        add_in(&bm, "https://b.com".into(), "B".into(), "A".into());
        add_in(&bm, "https://c.com".into(), "C".into(), "Z".into());
        add(&bm, "https://d.com".into(), "D".into()); // racine
        assert_eq!(folders(&bm.borrow()), vec!["A".to_string(), "Z".to_string()]);
    }

    #[test]
    fn remove_oob_is_noop() {
        let bm = new();
        add(&bm, "https://a.com".into(), "A".into());
        remove(&bm, 9);
        remove(&bm, 0);
        assert!(bm.borrow().is_empty());
    }

    #[test]
    fn export_import_roundtrip_with_folders() {
        let items = vec![
            Bookmark { url: "https://root.com".into(), title: "Root".into(), folder: "".into() },
            Bookmark { url: "https://a.com".into(),    title: "A".into(),    folder: "Travail".into() },
            Bookmark { url: "https://b.com".into(),    title: "B".into(),    folder: "Travail/Outils".into() },
            Bookmark { url: "https://x.com".into(),    title: "X & <Y>".into(), folder: "Persos".into() },
        ];
        let html = export_netscape(&items);
        let back = import_netscape(&html);
        assert_eq!(back.len(), 4);
        let by_url: std::collections::HashMap<_, _> = back.iter().map(|b| (b.url.clone(), b)).collect();
        assert_eq!(by_url["https://root.com"].folder, "");
        assert_eq!(by_url["https://a.com"].folder, "Travail");
        assert_eq!(by_url["https://b.com"].folder, "Travail/Outils");
        assert_eq!(by_url["https://x.com"].folder, "Persos");
        assert_eq!(by_url["https://x.com"].title, "X & <Y>");
    }

    #[test]
    fn imports_real_browser_export() {
        let html = "<DL><p>\n  <DT><A HREF=\"https://example.com\" ADD_DATE=\"1700\">Example</A>\n\
                    <dt><a href=\"https://b.org\">B</a>\n</DL><p>";
        let got = import_netscape(html);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].url, "https://example.com");
        assert_eq!(got[0].folder, "");
    }

    #[test]
    fn imports_real_browser_with_folders() {
        let html = "<DL><p>\n\
                    <DT><A HREF=\"https://root.com\">Root</A>\n\
                    <DT><H3>Work</H3>\n\
                    <DL><p>\n\
                      <DT><A HREF=\"https://a.com\">A</A>\n\
                      <DT><H3>Tools</H3>\n\
                      <DL><p>\n\
                        <DT><A HREF=\"https://b.com\">B</A>\n\
                      </DL><p>\n\
                    </DL><p>\n\
                    </DL><p>";
        let got = import_netscape(html);
        assert_eq!(got.len(), 3);
        let by_url: std::collections::HashMap<_, _> = got.iter().map(|b| (b.url.clone(), b)).collect();
        assert_eq!(by_url["https://root.com"].folder, "");
        assert_eq!(by_url["https://a.com"].folder, "Work");
        assert_eq!(by_url["https://b.com"].folder, "Work/Tools");
    }

    #[test]
    fn malformed_never_panics() {
        for s in ["", "<a", "<a href=", "<A HREF=\"", "<a href=\"\">", "日本<a>x",
                  "<DL><H3>X</H3>", "<DL><DL><DL>", "</DL></DL></DL>"] {
            let _ = import_netscape(s);
        }
    }
}
