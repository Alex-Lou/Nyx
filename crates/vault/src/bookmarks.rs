use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection};

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
    let mut stmt = conn.prepare(
        "SELECT id, url, title, tags, created_at FROM bookmarks ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;

    let mut out = vec![];
    for row in rows {
        let (id, url, title, tags_json, created_at) = row?;
        out.push(Bookmark {
            id: Some(id),
            url,
            title,
            tags: serde_json::from_str(&tags_json).unwrap_or_default(),
            created_at: created_at.parse().unwrap_or_else(|_| Utc::now()),
        });
    }
    Ok(out)
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM bookmarks WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn search(conn: &Connection, query: &str) -> Result<Vec<Bookmark>> {
    let pattern = format!("%{}%", query);
    let mut stmt = conn.prepare(
        "SELECT id, url, title, tags, created_at FROM bookmarks
         WHERE url LIKE ?1 OR title LIKE ?1
         ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map(params![pattern], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;

    let mut out = vec![];
    for row in rows {
        let (id, url, title, tags_json, created_at) = row?;
        out.push(Bookmark {
            id: Some(id),
            url,
            title,
            tags: serde_json::from_str(&tags_json).unwrap_or_default(),
            created_at: created_at.parse().unwrap_or_else(|_| Utc::now()),
        });
    }
    Ok(out)
}
