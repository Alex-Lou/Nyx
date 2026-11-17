use anyhow::Result;
use chrono::Utc;
use std::collections::HashMap;
use rusqlite::{params, Connection, Params, Row};

use crate::models::Bookmark;

pub fn add(conn: &Connection, b: &Bookmark) -> Result<i64> {
    let tags = serde_json::to_string(&b.tags)?;
    conn.execute(
        "INSERT INTO bookmarks (url, title, folder, tags, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![b.url, b.title, b.folder, tags, b.created_at.to_rfc3339()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list(conn: &Connection) -> Result<Vec<Bookmark>> {
    query(
        conn,
        "SELECT id, url, title, folder, tags, created_at FROM bookmarks ORDER BY created_at DESC",
        [],
    )
}

pub fn search(conn: &Connection, query_str: &str) -> Result<Vec<Bookmark>> {
    let pattern = crate::db::like_pattern(query_str);
    query(
        conn,
        "SELECT id, url, title, folder, tags, created_at FROM bookmarks
         WHERE url LIKE ?1 ESCAPE '\\' OR title LIKE ?1 ESCAPE '\\'
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
    let tx = conn.unchecked_transaction()?;
    // url → created_at des lignes existantes (préservé à travers la resync).
    let mut prior: HashMap<String, String> = HashMap::new();
    {
        let mut stmt = tx.prepare("SELECT url, created_at FROM bookmarks ORDER BY id DESC")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (url, created) = row?;
            prior.insert(url, created);
        }
    }
    tx.execute("DELETE FROM bookmarks", [])?;
    for b in items {
        let tags = serde_json::to_string(&b.tags)?;
        let created = prior
            .get(&b.url)
            .cloned()
            .unwrap_or_else(|| b.created_at.to_rfc3339());
        tx.execute(
            "INSERT INTO bookmarks (url, title, folder, tags, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![b.url, b.title, b.folder, tags, created],
        )?;
    }
    tx.commit()?;
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
        folder: row.get(3)?,
        tags: serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or_default(),
        created_at: row
            .get::<_, String>(5)?
            .parse()
            .unwrap_or_else(|_| Utc::now()),
    })
}
