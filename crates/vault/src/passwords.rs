use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection};

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
    let rows = stmt.query_map(params![domain], |row| {
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
        let (id, domain, username, password, created_at) = row?;
        out.push(Password {
            id: Some(id),
            domain,
            username,
            password,
            created_at: created_at.parse().unwrap_or_else(|_| Utc::now()),
        });
    }
    Ok(out)
}

pub fn list_domains(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT DISTINCT domain FROM passwords ORDER BY domain")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
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
