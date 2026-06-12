use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;

const SCHEMA: &str = "
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS bookmarks (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    url        TEXT    NOT NULL,
    title      TEXT    NOT NULL,
    tags       TEXT    NOT NULL DEFAULT '[]',  -- JSON array
    created_at TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS passwords (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    domain     TEXT    NOT NULL,
    username   TEXT    NOT NULL,
    password   TEXT    NOT NULL,               -- stored as-is (vault chiffre tout le fichier)
    created_at TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS history (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    url        TEXT    NOT NULL,
    title      TEXT    NOT NULL,
    visited_at TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_history_visited ON history(visited_at DESC);
CREATE INDEX IF NOT EXISTS idx_passwords_domain ON passwords(domain);
";

pub fn open(path: &Path, passphrase: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;

    // SQLCipher: doit être le tout premier pragma avant toute opération
    conn.pragma_update(None, "key", passphrase)?;

    // SQLCipher ne valide la clé qu'à la première lecture : on force une
    // lecture immédiate pour échouer franchement si la passphrase est mauvaise.
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
        .context("passphrase invalide ou fichier vault corrompu")?;

    // Optimisations standard SQLite
    conn.execute_batch("
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous  = NORMAL;
        PRAGMA cache_size   = -8000;  -- 8 MB
    ")?;

    conn.execute_batch(SCHEMA)?;

    Ok(conn)
}
