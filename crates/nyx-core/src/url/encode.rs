//! Encodage `application/x-www-form-urlencoded` et échappement HTML —
//! source unique (remplace les `urlencode`/`urldecode` partiels de
//! settings, web/mod et l'`escape` des pages internes).

/// Encode un composant de query : non-réservés RFC 3986 tels quels,
/// espace → `+`, tout le reste en `%XX` (UTF-8 octet par octet).
pub fn form_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Décode un composant de query : `+` → espace, `%XX` → octet. Les octets
/// sont réassemblés PUIS décodés en UTF-8 (lossy) : `%C3%A9` → `é`.
/// Séquence `%` invalide gardée littéralement.
pub fn form_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match (hex(b[i + 1]), hex(b[i + 2])) {
                    (Some(h), Some(l)) => { out.push(h << 4 | l); i += 2; }
                    _ => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(c: u8) -> Option<u8> {
    (c as char).to_digit(16).map(|d| d as u8)
}

/// Échappe un texte pour insertion dans du HTML (contenu ou attribut entre
/// guillemets doubles ou simples).
pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_special_chars() {
        for s in ["c++", "100%", "a=b&c?d#e", "été à Liège", "https://x.com/?q=1+2", "", "%", "%zz"] {
            assert_eq!(form_decode(&form_encode(s)), s, "{s}");
        }
    }

    #[test]
    fn decode_utf8_and_plus() {
        assert_eq!(form_decode("%C3%A9t%C3%A9+ok"), "été ok");
        assert_eq!(form_decode("50%"), "50%");
        assert_eq!(form_decode("%4"), "%4");
        assert_eq!(form_decode("%G1"), "%G1");
    }

    #[test]
    fn html_escape_all_dangerous() {
        assert_eq!(html_escape(r#"<a href="x" onclick='y'>&"#), "&lt;a href=&quot;x&quot; onclick=&#39;y&#39;&gt;&amp;");
    }
}
