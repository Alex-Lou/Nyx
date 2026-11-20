use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;

use crate::key::Key;

/// v1 : schéma de base historique (sans `bookmarks.folder` ni `settings`).
const V1: &str = "
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

/// v2 : table `settings` (la colonne `folder` est ajoutée conditionnellement).
const V2_SETTINGS: &str = "
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

/// Version courante du schéma (valeur de `PRAGMA user_version`).
/// v3 : métadonnées de synchronisation (uuid, HLC, pierres tombales…),
/// voir `sync.rs`.
const LATEST: i64 = 3;

pub(crate) fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT count(*) FROM pragma_table_info(?1) WHERE name = ?2",
        [table, column],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// Applique les migrations manquantes, chacune dans sa transaction.
fn migrate(conn: &Connection) -> Result<()> {
    let mut version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    while version < LATEST {
        let next = version + 1;
        let tx = conn.unchecked_transaction()?;
        match next {
            1 => tx.execute_batch(V1)?,
            2 => {
                if !has_column(&tx, "bookmarks", "folder")? {
                    tx.execute(
                        "ALTER TABLE bookmarks ADD COLUMN folder TEXT NOT NULL DEFAULT ''",
                        [],
                    )?;
                }
                tx.execute_batch(V2_SETTINGS)?;
            }
            3 => {
                // Tables de base idempotentes d'abord : un vieux coffre
                // partiel (ex. v1 sans `passwords`) est ainsi complété.
                tx.execute_batch(V1)?;
                tx.execute_batch(V2_SETTINGS)?;
                crate::sync::migrate_v3(&tx)?
            }
            _ => unreachable!("migration inconnue"),
        }
        tx.pragma_update(None, "user_version", next)?;
        tx.commit()?;
        version = next;
    }
    Ok(())
}

/// Connexion chiffrée, clé posée et vérifiée (lecture forcée), sans
/// pragmas d'optimisation ni migration.
fn keyed(path: &Path, key: &Key) -> Result<Connection> {
    let conn = Connection::open(path)?;

    // SQLCipher: doit être le tout premier pragma avant toute opération
    conn.pragma_update(None, "key", key.pragma_value().expose())?;

    // SQLCipher ne valide la clé qu'à la première lecture : on force une
    // lecture immédiate pour échouer franchement si la clé est mauvaise.
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
        .context("clé invalide ou fichier vault corrompu")?;
    Ok(conn)
}

/// `true` si le coffre EXISTANT à `path` s'ouvre avec `key` (ne crée rien).
pub fn check_key(path: &Path, key: &Key) -> bool {
    path.is_file() && keyed(path, key).is_ok()
}

/// Rechiffre tout le coffre avec `new` (`PRAGMA rekey`). Par prudence, le
/// WAL est d'abord fusionné et le journal repassé en mode classique le temps
/// de l'opération.
pub fn rekey(conn: &Connection, new: &Key) -> Result<()> {
    conn.query_row("PRAGMA journal_mode = DELETE", [], |_| Ok(()))?;
    let result = conn.pragma_update(None, "rekey", new.pragma_value().expose());
    let restored = conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()));
    result.context("changement de clé du coffre")?;
    restored?;
    Ok(())
}

pub fn open(path: &Path, key: &Key) -> Result<Connection> {
    let conn = keyed(path, key)?;

    // Optimisations standard SQLite
    conn.execute_batch("
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous  = NORMAL;
        PRAGMA cache_size   = -8000;  -- 8 MB
        PRAGMA busy_timeout = 5000;   -- la sync ouvre une 2e connexion
    ")?;

    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    migrate(&conn)?;
    crate::sync::purge_tombstones(&conn, crate::sync::TOMBSTONE_TTL_MS)?;

    Ok(conn)
}

/// Motif `LIKE` « contient » avec `%`, `_` et `\` échappés (à utiliser avec `ESCAPE '\'`).
pub fn like_pattern(q: &str) -> String {
    let mut out = String::with_capacity(q.len() + 2);
    out.push('%');
    for c in q.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}
