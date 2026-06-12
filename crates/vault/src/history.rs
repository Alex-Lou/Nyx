use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection, Params, Row};

use crate::models::HistoryEntry;

pub fn push(conn: &Connection, url: &str, title: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO history (url, title, visited_at) VALUES (?1, ?2, ?3)",
        params![url, title, Utc::now().to_rfc3339()],
    )?;
    Ok(())
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
    let pattern = format!("%{}%", query_str);
    query(
        conn,
        "SELECT id, url, title, visited_at FROM history
         WHERE url LIKE ?1 OR title LIKE ?1
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
