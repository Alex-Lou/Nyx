use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection, Row};

use crate::models::Password;

pub fn add(conn: &Connection, p: &Password) -> Result<i64> {
    conn.execute(
        "INSERT INTO passwords (domain, username, password, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![p.domain, p.username, p.password, p.created_at.to_rfc3339()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn find_by_domain(conn: &Connection, domain: &str) -> Result<Vec<Password>> {
    let mut stmt = conn.prepare(
        "SELECT id, domain, username, password, created_at
         FROM passwords WHERE domain = ?1",
    )?;
    let rows = stmt.query_map(params![domain], from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn list_domains(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT DISTINCT domain FROM passwords ORDER BY domain")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM passwords WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn update_password(conn: &Connection, id: i64, new_password: &str) -> Result<()> {
    conn.execute(
        "UPDATE passwords SET password = ?1 WHERE id = ?2",
        params![new_password, id],
    )?;
    Ok(())
}

fn from_row(row: &Row) -> rusqlite::Result<Password> {
    Ok(Password {
        id: Some(row.get(0)?),
        domain: row.get(1)?,
        username: row.get(2)?,
        password: row.get(3)?,
        created_at: row
            .get::<_, String>(4)?
            .parse()
            .unwrap_or_else(|_| Utc::now()),
    })
}
