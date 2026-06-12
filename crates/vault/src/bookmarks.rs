use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection, Params, Row};

use crate::models::Bookmark;

pub fn add(conn: &Connection, b: &Bookmark) -> Result<i64> {
    let tags = serde_json::to_string(&b.tags)?;
    conn.execute(
        "INSERT INTO bookmarks (url, title, tags, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![b.url, b.title, tags, b.created_at.to_rfc3339()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list(conn: &Connection) -> Result<Vec<Bookmark>> {
    query(
        conn,
        "SELECT id, url, title, tags, created_at FROM bookmarks ORDER BY created_at DESC",
        [],
    )
}

pub fn search(conn: &Connection, query_str: &str) -> Result<Vec<Bookmark>> {
    let pattern = format!("%{}%", query_str);
    query(
        conn,
        "SELECT id, url, title, tags, created_at FROM bookmarks
         WHERE url LIKE ?1 OR title LIKE ?1
         ORDER BY created_at DESC",
        params![pattern],
    )
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM bookmarks WHERE id = ?1", params![id])?;
    Ok(())
}

/// Remplace tout le contenu de la table (sync complète du store mémoire —
/// les listes de favoris restent petites, la simplicité prime).
pub fn replace_all(conn: &Connection, items: &[Bookmark]) -> Result<()> {
    conn.execute("DELETE FROM bookmarks", [])?;
    for b in items {
        add(conn, b)?;
    }
    Ok(())
}

fn query(conn: &Connection, sql: &str, params: impl Params) -> Result<Vec<Bookmark>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn from_row(row: &Row) -> rusqlite::Result<Bookmark> {
    Ok(Bookmark {
        id: Some(row.get(0)?),
        url: row.get(1)?,
        title: row.get(2)?,
        tags: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default(),
        created_at: row
            .get::<_, String>(4)?
            .parse()
            .unwrap_or_else(|_| Utc::now()),
    })
}
