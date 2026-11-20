use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection, Row};

use crate::models::Password;
use crate::sync::{random_uuid, stamp};

pub fn add(conn: &Connection, device: &str, p: &Password) -> Result<i64> {
    let s = stamp(conn, device)?;
    conn.execute(
        "INSERT INTO passwords (domain, username, password, created_at, uuid, updated_at, origin_device, local_hlc)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?6)",
        params![p.domain, p.username, p.password, p.created_at.to_rfc3339(), random_uuid()?, s.hlc, s.device],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn find_by_domain(conn: &Connection, domain: &str) -> Result<Vec<Password>> {
    let mut stmt = conn.prepare(
        "SELECT id, domain, username, password, created_at
         FROM passwords WHERE domain = ?1 AND deleted = 0",
    )?;
    let rows = stmt.query_map(params![domain], from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn list_domains(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT DISTINCT domain FROM passwords WHERE deleted = 0 ORDER BY domain")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Suppression = pierre tombale : identifiants et secret effacés, uuid
/// conservé pour propager la suppression.
pub fn delete(conn: &Connection, device: &str, id: i64) -> Result<()> {
    let s = stamp(conn, device)?;
    conn.execute(
        "UPDATE passwords SET deleted = 1, domain = '', username = '', password = '', created_at = '',
           updated_at = ?2, origin_device = ?3, local_hlc = ?2
         WHERE id = ?1 AND deleted = 0",
        params![id, s.hlc, s.device],
    )?;
    Ok(())
}

pub fn update_password(conn: &Connection, device: &str, id: i64, new_password: &str) -> Result<()> {
    let s = stamp(conn, device)?;
    conn.execute(
        "UPDATE passwords SET password = ?1, updated_at = ?3, origin_device = ?4, local_hlc = ?3
         WHERE id = ?2 AND deleted = 0",
        params![new_password, id, s.hlc, s.device],
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
