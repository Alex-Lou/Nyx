use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection};

use crate::models::HistoryEntry;

pub fn push(conn: &Connection, url: &str, title: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO history (url, title, visited_at) VALUES (?1, ?2, ?3)",
        params![url, title, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

pub fn list_recent(conn: &Connection, limit: usize) -> Result<Vec<HistoryEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, url, title, visited_at FROM history
         ORDER BY visited_at DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;

    let mut out = vec![];
    for row in rows {
        let (id, url, title, visited_at) = row?;
        out.push(HistoryEntry {
            id: Some(id),
            url,
            title,
            visited_at: visited_at.parse().unwrap_or_else(|_| Utc::now()),
        });
    }
    Ok(out)
}

pub fn search(conn: &Connection, query: &str) -> Result<Vec<HistoryEntry>> {
    let pattern = format!("%{}%", query);
    let mut stmt = conn.prepare(
        "SELECT id, url, title, visited_at FROM history
         WHERE url LIKE ?1 OR title LIKE ?1
         ORDER BY visited_at DESC LIMIT 100",
    )?;
    let rows = stmt.query_map(params![pattern], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;

    let mut out = vec![];
    for row in rows {
        let (id, url, title, visited_at) = row?;
        out.push(HistoryEntry {
            id: Some(id),
            url,
            title,
            visited_at: visited_at.parse().unwrap_or_else(|_| Utc::now()),
        });
    }
    Ok(out)
}

pub fn clear(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM history", [])?;
    Ok(())
}
