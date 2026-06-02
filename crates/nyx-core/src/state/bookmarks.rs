use std::cell::RefCell;
use std::rc::Rc;

/// Store en mémoire (Sprint 2 : persistance dans le vault chiffré).
/// Dossiers + drag-and-drop : itération suivante (modèle hiérarchique).
pub type Bookmarks = Rc<RefCell<Vec<Bookmark>>>;

#[derive(Clone)]
pub struct Bookmark {
    pub url:   String,
    pub title: String,
}

pub fn new() -> Bookmarks {
    Rc::new(RefCell::new(Vec::new()))
}

/// Ajoute un favori (ignore les doublons d'URL). Retourne `true` si ajouté.
pub fn add(bm: &Bookmarks, url: String, title: String) -> bool {
    if url.is_empty() {
        return false;
    }
    let mut list = bm.borrow_mut();
    if list.iter().any(|b| b.url == url) {
        return false;
    }
    list.push(Bookmark { url, title });
    true
}

pub fn remove(bm: &Bookmarks, index: usize) {
    let mut list = bm.borrow_mut();
    if index < list.len() {
        list.remove(index);
    }
}

// ── Import / Export — format Netscape (universel : Chrome/Firefox/Edge…) ────

/// Sérialise au format Netscape Bookmark — importable par n'importe quel navigateur.
pub fn export_netscape(items: &[Bookmark]) -> String {
    let mut s = String::from(
        "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n\
         <META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">\n\
         <TITLE>Bookmarks</TITLE>\n<H1>Bookmarks</H1>\n<DL><p>\n",
    );
    for b in items {
        s.push_str(&format!(
            "    <DT><A HREF=\"{}\">{}</A>\n",
            escape(&b.url),
            escape(&b.title)
        ));
    }
    s.push_str("</DL><p>\n");
    s
}

/// Parse un export Netscape de n'importe quel navigateur → liste de favoris.
pub fn import_netscape(html: &str) -> Vec<Bookmark> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(rel) = find_ci(&html[i..], "<a ") {
        let a = i + rel + 3;
        let Some(hrel) = find_ci(&html[a..], "href=\"") else { i = a; continue; };
        let us = a + hrel + 6;
        let Some(ue) = html[us..].find('"') else { break; };
        let url = unescape(&html[us..us + ue]);
        let Some(gt) = html[us + ue..].find('>') else { break; };
        let ts = us + ue + gt + 1;
        let Some(lt) = html[ts..].find('<') else { break; };
        let title = unescape(html[ts..ts + lt].trim());
        if !url.is_empty() {
            out.push(Bookmark { url, title });
        }
        i = ts + lt;
    }
    out
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
    // &amp; en dernier pour ne pas ré-interpréter les entités déjà décodées.
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_dedups() {
        let bm = new();
        assert!(add(&bm, "https://x.com".into(), "X".into()));
        assert!(!add(&bm, "https://x.com".into(), "X again".into())); // doublon
        assert!(!add(&bm, "".into(), "vide".into()));
        assert_eq!(bm.borrow().len(), 1);
    }

    #[test]
    fn remove_oob_is_noop() {
        let bm = new();
        add(&bm, "https://a.com".into(), "A".into());
        remove(&bm, 9); // hors borne → no-op, pas de panic
        remove(&bm, 0);
        assert!(bm.borrow().is_empty());
    }

    #[test]
    fn export_import_roundtrip() {
        let items = vec![
            Bookmark { url: "https://rust-lang.org".into(), title: "Rust".into() },
            Bookmark { url: "https://a.com/?x=1&y=2".into(), title: "A & B <test>".into() },
        ];
        let html = export_netscape(&items);
        let back = import_netscape(&html);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].url, "https://rust-lang.org");
        assert_eq!(back[1].url, "https://a.com/?x=1&y=2");
        assert_eq!(back[1].title, "A & B <test>"); // entités correctement décodées
    }

    #[test]
    fn imports_real_browser_export() {
        // Forme produite par Chrome/Firefox (casse + attributs variés).
        let html = "<DL><p>\n  <DT><A HREF=\"https://example.com\" ADD_DATE=\"1700\">Example</A>\n\
                    <dt><a href=\"https://b.org\">B</a>\n</DL><p>";
        let got = import_netscape(html);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].url, "https://example.com");
        assert_eq!(got[1].url, "https://b.org");
    }

    #[test]
    fn malformed_never_panics() {
        for s in ["", "<a", "<a href=", "<A HREF=\"", "<a href=\"\">", "日本<a>x"] {
            let _ = import_netscape(s);
        }
    }
}
