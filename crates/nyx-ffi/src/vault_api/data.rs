//! Données du coffre exposées à Kotlin : listes en JSON (serde), écritures
//! simples. Les règles (historique enregistrable, pré-remplissage) viennent
//! de `nyx-core` ; aucun mot de passe dans les listes (révélation à la
//! demande, par identifiant).

use serde::Serialize;

use nyx_core::url::{host_of, scheme_of};
use nyx_core::vault_autofill::{fill_script, Credential};
use vault::{Bookmark, Password};

use super::Session;

/// Hôte de la page Nouvel onglet (`WebViewAssetLoader`) : jamais
/// enregistrée, jamais pré-remplie.
pub const APP_ASSETS_HOST: &str = "appassets.androidplatform.net";

/// Entrées d'historique renvoyées par défaut (écran Historique).
pub const HISTORY_SHOWN: usize = 300;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct PasswordRow {
    pub id: i64,
    pub domain: String,
    pub username: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct BookmarkRow {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub folder: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct HistoryRow {
    pub id: i64,
    pub url: String,
    pub title: String,
    /// Secondes Unix.
    pub visited_at: i64,
}

fn to_json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "[]".into())
}

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

/// Page web enregistrable : http(s), pas la page Nouvel onglet interne.
pub fn recordable(url: &str) -> bool {
    matches!(scheme_of(url).as_deref(), Some("http" | "https"))
        && host_of(url).is_some_and(|h| !h.eq_ignore_ascii_case(APP_ASSETS_HOST))
}

impl Session {
    // ── Mots de passe ───────────────────────────────────────────────────

    /// Tous les identifiants (sans mot de passe), triés par domaine.
    pub fn passwords_json(&self) -> Result<String, String> {
        let vault = self.vault()?;
        let mut rows = Vec::new();
        for domain in vault.list_password_domains().map_err(err)? {
            for p in vault.passwords_for(&domain).map_err(err)? {
                rows.push(PasswordRow { id: p.id.unwrap_or(-1), domain: p.domain, username: p.username });
            }
        }
        Ok(to_json(&rows))
    }

    /// Identifiants (sans mot de passe) pour l'hôte exact `host`.
    pub fn passwords_for_json(&self, host: &str) -> Result<String, String> {
        let rows: Vec<PasswordRow> = self
            .vault()?
            .passwords_for(&host.to_ascii_lowercase())
            .map_err(err)?
            .into_iter()
            .map(|p| PasswordRow { id: p.id.unwrap_or(-1), domain: p.domain, username: p.username })
            .collect();
        Ok(to_json(&rows))
    }

    pub fn password_domains_json(&self) -> Result<String, String> {
        Ok(to_json(&self.vault()?.list_password_domains().map_err(err)?))
    }

    /// Mot de passe de l'entrée `id` du domaine `domain` (affichage / copie).
    pub fn password_reveal(&self, domain: &str, id: i64) -> Result<String, String> {
        self.vault()?
            .passwords_for(domain)
            .map_err(err)?
            .into_iter()
            .find(|p| p.id == Some(id))
            .map(|p| p.password.clone())
            .ok_or_else(|| "identifiant introuvable".to_string())
    }

    pub fn password_add(&self, domain: &str, username: &str, password: &str) -> Result<i64, String> {
        let domain = domain.trim().to_ascii_lowercase();
        if domain.is_empty() || password.is_empty() {
            return Err("domaine et mot de passe requis".into());
        }
        self.vault()?.add_password(&Password::new(domain, username, password)).map_err(err)
    }

    pub fn password_update(&self, id: i64, password: &str) -> Result<(), String> {
        if password.is_empty() {
            return Err("mot de passe vide".into());
        }
        self.vault()?.update_password(id, password).map_err(err)
    }

    pub fn password_delete(&self, id: i64) -> Result<(), String> {
        self.vault()?.delete_password(id).map_err(err)
    }

    // ── Pré-remplissage ─────────────────────────────────────────────────

    /// Script à exécuter dans le cadre principal de `page_url` (`""` :
    /// rien). Politique partagée `vault_autofill::fill_script` : https,
    /// hôte exact, un seul compte, jamais en navigation privée.
    pub fn autofill_script(&self, page_url: &str, private: bool) -> String {
        let (Ok(vault), Some(host)) = (self.vault(), host_of(page_url)) else { return String::new() };
        if private || host.eq_ignore_ascii_case(APP_ASSETS_HOST) {
            return String::new();
        }
        let creds = vault.passwords_for(&host).unwrap_or_default();
        let creds: Vec<Credential> = creds
            .iter()
            .map(|c| Credential { domain: &c.domain, username: &c.username, password: &c.password })
            .collect();
        fill_script(page_url, &creds, private).unwrap_or_default()
    }

    // ── Favoris ─────────────────────────────────────────────────────────

    pub fn bookmarks_json(&self) -> Result<String, String> {
        let rows: Vec<BookmarkRow> = self
            .vault()?
            .list_bookmarks()
            .map_err(err)?
            .into_iter()
            .map(|b| BookmarkRow { id: b.id.unwrap_or(-1), url: b.url, title: b.title, folder: b.folder })
            .collect();
        Ok(to_json(&rows))
    }

    /// Identifiant du favori d'URL exacte `url`, s'il existe.
    pub fn bookmark_find(&self, url: &str) -> Result<Option<i64>, String> {
        Ok(self.vault()?.list_bookmarks().map_err(err)?.into_iter().find(|b| b.url == url).and_then(|b| b.id))
    }

    /// Ajoute un favori (page web uniquement ; jamais en double).
    pub fn bookmark_add(&self, url: &str, title: &str) -> Result<i64, String> {
        if !recordable(url) {
            return Err("page non enregistrable".into());
        }
        if let Some(id) = self.bookmark_find(url)? {
            return Ok(id);
        }
        let title = if title.trim().is_empty() { url } else { title.trim() };
        self.vault()?.add_bookmark(&Bookmark::new(url, title)).map_err(err)
    }

    pub fn bookmark_delete(&self, id: i64) -> Result<(), String> {
        self.vault()?.delete_bookmark(id).map_err(err)
    }

    // ── Historique ──────────────────────────────────────────────────────

    /// Enregistre une navigation principale (`false` : ignorée).
    pub fn history_push(&self, url: &str, title: &str) -> Result<bool, String> {
        if !recordable(url) {
            return Ok(false);
        }
        self.vault()?.push_history(url, title).map_err(err)?;
        Ok(true)
    }

    /// Titre arrivé après le chargement : reporté sur la dernière visite.
    pub fn history_retitle(&self, url: &str, title: &str) -> Result<bool, String> {
        if !recordable(url) || title.trim().is_empty() {
            return Ok(false);
        }
        self.vault()?.retitle_history(url, title.trim()).map_err(err)
    }

    pub fn history_recent_json(&self, limit: usize) -> Result<String, String> {
        Ok(history_rows(self.vault()?.recent_history(limit.clamp(1, 5000)).map_err(err)?))
    }

    pub fn history_search_json(&self, query: &str) -> Result<String, String> {
        let vault = self.vault()?;
        let entries = if query.trim().is_empty() {
            vault.recent_history(HISTORY_SHOWN)
        } else {
            vault.search_history(query.trim())
        };
        Ok(history_rows(entries.map_err(err)?))
    }

    pub fn history_clear(&self) -> Result<(), String> {
        self.vault()?.clear_history().map_err(err)
    }
}

fn history_rows(entries: Vec<vault::HistoryEntry>) -> String {
    let rows: Vec<HistoryRow> = entries
        .into_iter()
        .map(|h| HistoryRow { id: h.id.unwrap_or(-1), url: h.url, title: h.title, visited_at: h.visited_at.timestamp() })
        .collect();
    to_json(&rows)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{open_session, TempDir};
    use super::*;

    #[test]
    fn recordable_urls() {
        assert!(recordable("https://a.org/x"));
        assert!(recordable("http://intranet.local/"));
        assert!(!recordable("https://appassets.androidplatform.net/newtab.html"));
        assert!(!recordable("about:blank"));
        assert!(!recordable("data:text/html,x"));
        assert!(!recordable(""));
    }

    #[test]
    fn passwords_crud_without_leaking() {
        let tmp = TempDir::new("pw");
        let s = open_session(&tmp);
        let id = s.password_add("Example.com", "alex", "hunter2").expect("add");
        assert!(s.password_add("", "a", "b").is_err());
        assert!(s.password_add("x.org", "a", "").is_err());
        let json = s.passwords_json().expect("list");
        assert!(!json.contains("hunter2"), "{json}");
        assert_eq!(json, format!(r#"[{{"id":{id},"domain":"example.com","username":"alex"}}]"#));
        assert_eq!(s.passwords_for_json("EXAMPLE.com").expect("for"), json);
        assert_eq!(s.password_domains_json().expect("domains"), r#"["example.com"]"#);
        assert_eq!(s.password_reveal("example.com", id).expect("reveal"), "hunter2");
        assert!(s.password_reveal("other.org", id).is_err());
        s.password_update(id, "new").expect("update");
        assert!(s.password_update(id, "").is_err());
        assert_eq!(s.password_reveal("example.com", id).expect("reveal"), "new");
        s.password_delete(id).expect("delete");
        assert_eq!(s.passwords_json().expect("list"), "[]");
    }

    #[test]
    fn autofill_only_single_exact_https() {
        let tmp = TempDir::new("fill");
        let s = open_session(&tmp);
        assert_eq!(s.autofill_script("https://example.com/login", false), "");
        s.password_add("example.com", "alex", "pw").expect("add");
        let js = s.autofill_script("https://example.com/login", false);
        assert!(js.ends_with(r#"("https://example.com","alex","pw")"#), "{js}");
        assert_eq!(s.autofill_script("https://example.com/login", true), "", "privé");
        assert_eq!(s.autofill_script("http://example.com/login", false), "");
        assert_eq!(s.autofill_script("https://sub.example.com/", false), "");
        s.password_add("example.com", "sam", "pw2").expect("add");
        assert_eq!(s.autofill_script("https://example.com/login", false), "", "deux comptes");
        assert_eq!(Session::new().autofill_script("https://example.com/", false), "", "verrouillé");
    }

    #[test]
    fn bookmarks_crud() {
        let tmp = TempDir::new("bm");
        let s = open_session(&tmp);
        assert!(s.bookmark_add("https://appassets.androidplatform.net/newtab.html", "x").is_err());
        let id = s.bookmark_add("https://rust-lang.org/", "  ").expect("add");
        assert_eq!(s.bookmark_add("https://rust-lang.org/", "Rust").expect("dup"), id, "pas de doublon");
        assert_eq!(s.bookmark_find("https://rust-lang.org/").expect("find"), Some(id));
        let json = s.bookmarks_json().expect("list");
        assert_eq!(
            json,
            format!(r#"[{{"id":{id},"url":"https://rust-lang.org/","title":"https://rust-lang.org/","folder":""}}]"#)
        );
        s.bookmark_delete(id).expect("delete");
        assert_eq!(s.bookmark_find("https://rust-lang.org/").expect("find"), None);
        assert_eq!(s.bookmarks_json().expect("list"), "[]");
    }

    #[test]
    fn history_flow() {
        let tmp = TempDir::new("hist");
        let s = open_session(&tmp);
        assert!(!s.history_push("https://appassets.androidplatform.net/newtab.html", "").expect("push"));
        assert!(s.history_push("https://a.org/", "").expect("push"));
        assert!(s.history_push("https://b.org/", "B").expect("push"));
        assert!(s.history_retitle("https://a.org/", " A ").expect("retitle"));
        assert!(!s.history_retitle("https://a.org/", "  ").expect("blank"));
        let recent: Vec<serde_json::Value> =
            serde_json::from_str(&s.history_recent_json(10).expect("recent")).expect("json");
        assert_eq!(recent.len(), 2);
        assert!(recent.iter().any(|h| h["title"] == "A" && h["url"] == "https://a.org/"));
        assert!(recent[0]["visited_at"].as_i64().is_some_and(|t| t > 1_600_000_000));
        let found: Vec<serde_json::Value> =
            serde_json::from_str(&s.history_search_json("b.org").expect("search")).expect("json");
        assert_eq!(found.len(), 1);
        assert_eq!(serde_json::from_str::<Vec<serde_json::Value>>(&s.history_search_json(" ").unwrap()).unwrap().len(), 2);
        s.history_clear().expect("clear");
        assert_eq!(s.history_recent_json(10).expect("recent"), "[]");
        assert!(Session::new().history_push("https://a.org/", "").is_err(), "verrouillé");
    }
}
