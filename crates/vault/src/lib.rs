mod bookmarks;
mod chrome;
mod db;
mod history;
mod models;
mod passwords;
mod settings;

pub use chrome::read_chrome_history;
pub use models::{Bookmark, HistoryEntry, Password};

use anyhow::Result;
use rusqlite::Connection;
use std::path::{Path, PathBuf};

pub struct Vault {
    conn: Connection,
    path: PathBuf,
}

impl Vault {
    /// Ouvre (ou crée) le vault chiffré au chemin donné.
    /// La passphrase est utilisée par SQLCipher pour dériver la clé AES-256.
    pub fn open(path: &Path, passphrase: &str) -> Result<Self> {
        let conn = db::open(path, passphrase)?;
        Ok(Self { conn, path: path.to_owned() })
    }

    pub fn path(&self) -> &Path { &self.path }

    // ── Bookmarks ──────────────────────────────────────────────────────────

    pub fn add_bookmark(&self, b: &Bookmark) -> Result<i64> {
        bookmarks::add(&self.conn, b)
    }

    pub fn list_bookmarks(&self) -> Result<Vec<Bookmark>> {
        bookmarks::list(&self.conn)
    }

    pub fn search_bookmarks(&self, query: &str) -> Result<Vec<Bookmark>> {
        bookmarks::search(&self.conn, query)
    }

    pub fn delete_bookmark(&self, id: i64) -> Result<()> {
        bookmarks::delete(&self.conn, id)
    }

    /// Remplace tous les bookmarks d'un coup (sync du store mémoire).
    pub fn replace_bookmarks(&self, items: &[Bookmark]) -> Result<()> {
        bookmarks::replace_all(&self.conn, items)
    }

    // ── Passwords ──────────────────────────────────────────────────────────

    pub fn add_password(&self, p: &Password) -> Result<i64> {
        passwords::add(&self.conn, p)
    }

    pub fn passwords_for(&self, domain: &str) -> Result<Vec<Password>> {
        passwords::find_by_domain(&self.conn, domain)
    }

    pub fn list_password_domains(&self) -> Result<Vec<String>> {
        passwords::list_domains(&self.conn)
    }

    pub fn update_password(&self, id: i64, new_password: &str) -> Result<()> {
        passwords::update_password(&self.conn, id, new_password)
    }

    pub fn delete_password(&self, id: i64) -> Result<()> {
        passwords::delete(&self.conn, id)
    }

    // ── History ────────────────────────────────────────────────────────────

    pub fn push_history(&self, url: &str, title: &str) -> Result<()> {
        history::push(&self.conn, url, title)
    }

    /// Import en bloc d'historique `(url, titre, secondes Unix)`.
    pub fn import_history(&self, entries: &[(String, String, i64)]) -> Result<usize> {
        history::import(&self.conn, entries)
    }

    pub fn recent_history(&self, limit: usize) -> Result<Vec<HistoryEntry>> {
        history::list_recent(&self.conn, limit)
    }

    pub fn search_history(&self, query: &str) -> Result<Vec<HistoryEntry>> {
        history::search(&self.conn, query)
    }

    pub fn clear_history(&self) -> Result<()> {
        history::clear(&self.conn)
    }

    // ── Settings ───────────────────────────────────────────────────────────

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        settings::get(&self.conn, key)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        settings::set(&self.conn, key, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vault temporaire avec un chemin unique, nettoyé par le guard.
    struct TempVault {
        path: PathBuf,
    }

    impl TempVault {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("nyx_vault_test_{}_{}.db", std::process::id(), name));
            let _ = std::fs::remove_file(&path);
            Self { path }
        }

        fn open(&self, passphrase: &str) -> Result<Vault> {
            Vault::open(&self.path, passphrase)
        }
    }

    impl Drop for TempVault {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    #[test]
    fn bookmarks_round_trip() -> Result<()> {
        let tmp = TempVault::new("bookmarks");
        let vault = tmp.open("secret")?;

        let mut b = Bookmark::new("https://rust-lang.org", "Rust");
        b.tags = vec!["dev".into()];
        let id = vault.add_bookmark(&b)?;

        let all = vault.list_bookmarks()?;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].url, "https://rust-lang.org");
        assert_eq!(all[0].tags, vec!["dev".to_string()]);

        assert_eq!(vault.search_bookmarks("rust")?.len(), 1);
        assert_eq!(vault.search_bookmarks("nope")?.len(), 0);

        vault.delete_bookmark(id)?;
        assert!(vault.list_bookmarks()?.is_empty());
        Ok(())
    }

    #[test]
    fn passwords_round_trip() -> Result<()> {
        let tmp = TempVault::new("passwords");
        let vault = tmp.open("secret")?;

        let id = vault.add_password(&Password::new("example.com", "alex", "hunter2"))?;
        let found = vault.passwords_for("example.com")?;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].password, "hunter2");
        assert_eq!(vault.list_password_domains()?, vec!["example.com"]);

        vault.update_password(id, "correct horse")?;
        assert_eq!(vault.passwords_for("example.com")?[0].password, "correct horse");

        vault.delete_password(id)?;
        assert!(vault.passwords_for("example.com")?.is_empty());
        Ok(())
    }

    #[test]
    fn import_history_keeps_dates_dedups_and_bounds() -> Result<()> {
        let tmp = TempVault::new("import_history");
        let vault = tmp.open("pw")?;
        let e = |u: &str, t: i64| (u.to_string(), String::new(), t);
        assert_eq!(vault.import_history(&[e("https://a.org", 1_600_000_000), e("https://b.org", 1_700_000_000)])?, 2);
        // Ré-import : idempotent.
        assert_eq!(vault.import_history(&[e("https://a.org", 1_600_000_000)])?, 0);
        let recent = vault.recent_history(10)?;
        assert_eq!(recent[0].url, "https://b.org");
        assert_eq!(recent[0].visited_at.timestamp(), 1_700_000_000);
        // Plafond : les plus récentes gagnent.
        let many: Vec<_> = (0..history::MAX_ENTRIES + 10).map(|i| e(&format!("https://x.org/{i}"), 1_800_000_000 + i)).collect();
        vault.import_history(&many)?;
        let all = vault.recent_history(history::MAX_ENTRIES as usize + 100)?;
        assert_eq!(all.len(), history::MAX_ENTRIES as usize);
        assert!(all.iter().all(|h| h.url != "https://a.org"));
        Ok(())
    }

    #[test]
    fn history_round_trip() -> Result<()> {
        let tmp = TempVault::new("history");
        let vault = tmp.open("secret")?;

        vault.push_history("https://a.org", "A")?;
        vault.push_history("https://b.org", "B")?;
        assert_eq!(vault.recent_history(10)?.len(), 2);
        assert_eq!(vault.search_history("a.org")?.len(), 1);

        vault.clear_history()?;
        assert!(vault.recent_history(10)?.is_empty());
        Ok(())
    }

    #[test]
    fn settings_round_trip() -> Result<()> {
        let tmp = TempVault::new("settings");
        let vault = tmp.open("secret")?;

        assert_eq!(vault.setting("adblock_whitelist")?, None);
        vault.set_setting("adblock_whitelist", r#"["example.com"]"#)?;
        assert_eq!(
            vault.setting("adblock_whitelist")?.as_deref(),
            Some(r#"["example.com"]"#)
        );
        // Upsert : la nouvelle valeur remplace l'ancienne
        vault.set_setting("adblock_whitelist", "[]")?;
        assert_eq!(vault.setting("adblock_whitelist")?.as_deref(), Some("[]"));
        Ok(())
    }

    #[test]
    fn mauvaise_passphrase_rejetee() -> Result<()> {
        let tmp = TempVault::new("badpass");
        {
            let vault = tmp.open("bonne-passphrase")?;
            vault.push_history("https://a.org", "A")?;
        }
        assert!(tmp.open("mauvaise-passphrase").is_err());
        // La bonne passphrase fonctionne toujours et les données persistent
        let vault = tmp.open("bonne-passphrase")?;
        assert_eq!(vault.recent_history(10)?.len(), 1);
        Ok(())
    }

    #[test]
    fn migration_ancien_schema_sans_folder() -> Result<()> {
        let tmp = TempVault::new("oldschema");
        {
            let conn = Connection::open(&tmp.path)?;
            conn.pragma_update(None, "key", "secret")?;
            conn.execute_batch(
                "CREATE TABLE bookmarks (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    url TEXT NOT NULL, title TEXT NOT NULL,
                    tags TEXT NOT NULL DEFAULT '[]', created_at TEXT NOT NULL);
                 INSERT INTO bookmarks (url, title, created_at)
                    VALUES ('https://old.org', 'Old', '2020-01-01T00:00:00+00:00');
                 PRAGMA user_version = 1;",
            )?;
        }
        let vault = tmp.open("secret")?;
        vault.add_bookmark(&Bookmark::new("https://new.org", "New").in_folder("f"))?;
        let all = vault.list_bookmarks()?;
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|b| b.url == "https://old.org" && b.folder.is_empty()));
        assert!(all.iter().any(|b| b.url == "https://new.org" && b.folder == "f"));
        Ok(())
    }

    #[test]
    fn replace_all_preserve_created_at() -> Result<()> {
        let tmp = TempVault::new("replace");
        let vault = tmp.open("secret")?;
        let mut first = Bookmark::new("https://a.org", "A");
        first.created_at = "2021-05-05T10:00:00Z".parse()?;
        vault.add_bookmark(&first)?;

        let items = vec![Bookmark::new("https://a.org", "A2"), Bookmark::new("https://b.org", "B")];
        let b_created = items[1].created_at;
        vault.replace_bookmarks(&items)?;

        let all = vault.list_bookmarks()?;
        assert_eq!(all.len(), 2);
        let a = all.iter().find(|b| b.url == "https://a.org").unwrap();
        assert_eq!(a.title, "A2");
        assert_eq!(a.created_at, first.created_at);
        let b = all.iter().find(|b| b.url == "https://b.org").unwrap();
        assert_eq!(b.created_at, b_created);
        Ok(())
    }

    #[test]
    fn recherche_like_echappee() -> Result<()> {
        let tmp = TempVault::new("like");
        let vault = tmp.open("secret")?;
        vault.add_bookmark(&Bookmark::new("https://a.org", "100% sûr"))?;
        vault.add_bookmark(&Bookmark::new("https://b.org", "snake_case"))?;
        vault.add_bookmark(&Bookmark::new("https://c.org", "plain"))?;
        assert_eq!(vault.search_bookmarks("%")?.len(), 1);
        assert_eq!(vault.search_bookmarks("_")?.len(), 1);
        assert_eq!(vault.search_bookmarks("e_c")?.len(), 1);
        assert_eq!(vault.search_bookmarks("a\\b")?.len(), 0);

        vault.push_history("https://h.org/a_b", "x")?;
        vault.push_history("https://h.org/axb", "y")?;
        assert_eq!(vault.search_history("a_b")?.len(), 1);
        assert_eq!(vault.search_history("%")?.len(), 0);
        Ok(())
    }

    #[test]
    fn historique_borne() -> Result<()> {
        let tmp = TempVault::new("prune");
        let vault = tmp.open("secret")?;
        let cap = history::MAX_ENTRIES as usize;
        // Insertion en masse directe, puis un push déclenche l'élagage.
        vault.conn.execute(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < ?1 + 10)
             INSERT INTO history (url, title, visited_at)
             SELECT 'https://x.org/' || i, 't', '2020-01-01T00:00:00Z' FROM n",
            [cap as i64],
        )?;
        vault.push_history("https://last.org", "last")?;
        let all = vault.recent_history(cap + 100)?;
        assert_eq!(all.len(), cap);
        assert_eq!(all[0].url, "https://last.org");
        Ok(())
    }
}
