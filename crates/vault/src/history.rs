use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection, Params, Row};

use crate::models::HistoryEntry;

/// Nombre maximal de lignes d'historique conservées.
pub(crate) const MAX_ENTRIES: i64 = 5000;

pub fn push(conn: &Connection, url: &str, title: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO history (url, title, visited_at) VALUES (?1, ?2, ?3)",
        params![url, title, Utc::now().to_rfc3339()],
    )?;
    conn.execute(
        "DELETE FROM history WHERE id NOT IN
         (SELECT id FROM history ORDER BY id DESC LIMIT ?1)",
        params![MAX_ENTRIES],
    )?;
    Ok(())
}

/// Met à jour le titre de la visite la plus récente de `url` (le titre d'un
/// document arrive souvent après la fin du chargement). Retourne `true` si
/// une ligne a changé.
pub fn retitle(conn: &Connection, url: &str, title: &str) -> Result<bool> {
    let n = conn.execute(
        "UPDATE history SET title = ?2
         WHERE id = (SELECT max(id) FROM history WHERE url = ?1) AND title <> ?2",
        params![url, title],
    )?;
    Ok(n > 0)
}

/// Import en bloc `(url, titre, secondes Unix)` : une transaction, sans
/// doublon exact (url + date) pour qu'un ré-import soit idempotent, puis
/// même plafond que `push` — en gardant les plus récentes par date.
/// Retourne le nombre de lignes réellement ajoutées.
pub fn import(conn: &Connection, entries: &[(String, String, i64)]) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let mut added = 0;
    for (url, title, secs) in entries {
        let Some(at) = chrono::DateTime::<Utc>::from_timestamp(*secs, 0) else { continue };
        added += tx.execute(
            "INSERT INTO history (url, title, visited_at) SELECT ?1, ?2, ?3
             WHERE NOT EXISTS (SELECT 1 FROM history WHERE url = ?1 AND visited_at = ?3)",
            params![url, title, at.to_rfc3339()],
        )?;
    }
    tx.execute(
        "DELETE FROM history WHERE id NOT IN
         (SELECT id FROM history ORDER BY visited_at DESC, id DESC LIMIT ?1)",
        params![MAX_ENTRIES],
    )?;
    tx.commit()?;
    Ok(added)
}

pub fn list_recent(conn: &Connection, limit: usize) -> Result<Vec<HistoryEntry>> {
    query(
        conn,
        "SELECT id, url, title, visited_at FROM history
         ORDER BY visited_at DESC LIMIT ?1",
        params![limit as i64],
    )
}

pub fn search(conn: &Connection, query_str: &str) -> Result<Vec<HistoryEntry>> {
    let pattern = crate::db::like_pattern(query_str);
    query(
        conn,
        "SELECT id, url, title, visited_at FROM history
         WHERE url LIKE ?1 ESCAPE '\\' OR title LIKE ?1 ESCAPE '\\'
         ORDER BY visited_at DESC LIMIT 100",
        params![pattern],
    )
}

pub fn clear(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM history", [])?;
    Ok(())
}

fn query(conn: &Connection, sql: &str, params: impl Params) -> Result<Vec<HistoryEntry>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn from_row(row: &Row) -> rusqlite::Result<HistoryEntry> {
    Ok(HistoryEntry {
        id: Some(row.get(0)?),
        url: row.get(1)?,
        title: row.get(2)?,
        visited_at: row
            .get::<_, String>(3)?
            .parse()
            .unwrap_or_else(|_| Utc::now()),
    })
}
