mod bookmarks;
mod db;
mod history;
mod models;
mod passwords;

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
}
