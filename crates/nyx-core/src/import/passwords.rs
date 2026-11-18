//! Export CSV du gestionnaire de mots de passe Chrome
//! (`chrome://password-manager/settings`). Colonnes mappées par nom d'en-tête.

use super::csv;
use crate::url::{host_of, scheme_of};

#[derive(Debug, PartialEq, Eq)]
pub struct Login {
    pub host: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedLogins {
    pub logins: Vec<Login>,
    /// Lignes inutilisables (hôte ou mot de passe absent).
    pub skipped: usize,
}

/// Parse le CSV. Sans colonnes `url` et `password` dans l'en-tête → vide.
pub fn parse(input: &str) -> ParsedLogins {
    let rows = csv::parse(input);
    let mut it = rows.into_iter();
    let Some(header) = it.next() else { return ParsedLogins::default() };
    let col = |name: &str| header.iter().position(|h| h.trim().eq_ignore_ascii_case(name));
    let (Some(iu), Some(ip)) = (col("url"), col("password")) else {
        return ParsedLogins::default();
    };
    let iname = col("username");

    let mut out = ParsedLogins::default();
    for row in it {
        let get = |i: usize| row.get(i).map(String::as_str).unwrap_or("");
        let url = get(iu).trim();
        // Seulement http(s) : les entrées `android://…` n'ont pas d'hôte web.
        let web = matches!(scheme_of(url).as_deref(), Some("http" | "https"));
        let host = if web { host_of(url).map(|h| h.into_owned()) } else { None };
        let password = get(ip);
        match host {
            Some(host) if !password.is_empty() => out.logins.push(Login {
                host,
                username: iname.map(get).unwrap_or("").to_string(),
                password: password.to_string(),
            }),
            _ => out.skipped += 1,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const HDR: &str = "name,url,username,password,note\n";

    #[test]
    fn basic_row() {
        let p = parse(&format!("{HDR}gh,https://github.com/login,me,s3cret,\n"));
        assert_eq!(p.skipped, 0);
        assert_eq!(p.logins, vec![Login {
            host: "github.com".into(), username: "me".into(), password: "s3cret".into(),
        }]);
    }
    #[test]
    fn password_with_comma_quote_and_crlf() {
        let p = parse("name,url,username,password,note\r\nx,https://a.com,u,\"p,\"\"w\"\"d\",\r\n");
        assert_eq!(p.logins[0].password, "p,\"w\"d");
    }
    #[test]
    fn columns_reordered_and_bom() {
        let p = parse("\u{feff}username,password,url\nme,pw,https://B.com/x\n");
        assert_eq!(p.logins[0].host, "b.com");
        assert_eq!(p.logins[0].username, "me");
    }
    #[test]
    fn skips_missing_host_or_password() {
        let p = parse(&format!("{HDR}a,,u,pw,\nb,https://b.com,u,,\nc,android://xyz,u,pw,\n"));
        assert!(p.logins.is_empty());
        assert_eq!(p.skipped, 3);
    }
    #[test]
    fn no_username_column() {
        let p = parse("url,password\nhttps://a.com,pw\n");
        assert_eq!(p.logins[0].username, "");
    }
    #[test]
    fn bad_header_or_empty() {
        assert_eq!(parse("foo,bar\n1,2\n"), ParsedLogins::default());
        assert_eq!(parse(""), ParsedLogins::default());
    }
}
