mod bookmarks;
mod db;
mod history;
mod models;
mod passwords;
mod settings;

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
}
