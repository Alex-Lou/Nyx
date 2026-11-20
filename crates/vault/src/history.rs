use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Params, Row};

use crate::models::HistoryEntry;
use crate::sync::{history_uuid, reserve, stamp};

/// Nombre maximal de lignes d'historique conservées.
pub(crate) const MAX_ENTRIES: i64 = 5000;

/// Insère (ou ressuscite) la visite `(url, visited_at)` ; `false` si elle
/// est déjà vivante. L'uuid d'une visite est dérivé de `(url, visited_at)`.
fn upsert_visit(conn: &Connection, device: &str, url: &str, title: &str, visited_at: &str) -> Result<bool> {
    let uuid = history_uuid(url, visited_at);
    let live: bool = conn
        .query_row("SELECT deleted = 0 FROM history WHERE uuid = ?1", [&uuid], |r| r.get(0))
        .optional()?
        .unwrap_or(false);
    if live {
        return Ok(false);
    }
    let s = stamp(conn, device)?;
    conn.execute(
        "INSERT INTO history (url, title, visited_at, uuid, updated_at, origin_device, local_hlc)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?5)
         ON CONFLICT(uuid) DO UPDATE SET url = excluded.url, title = excluded.title,
           visited_at = excluded.visited_at, deleted = 0, updated_at = excluded.updated_at,
           origin_device = excluded.origin_device, local_hlc = excluded.local_hlc",
        params![url, title, visited_at, uuid, s.hlc, s.device],
    )?;
    Ok(true)
}

/// Garde les `MAX_ENTRIES` visites vivantes les plus récentes. L'élagage est
/// local (suppression physique, pas de pierre tombale) : chaque appareil
/// borne son propre historique, et comme tous élaguent selon le même ordre
/// (`visited_at`), ils convergent vers le même ensemble.
pub(crate) fn prune(conn: &Connection) -> Result<()> {
    conn.execute(
        "DELETE FROM history WHERE deleted = 0 AND id NOT IN
         (SELECT id FROM history WHERE deleted = 0 ORDER BY visited_at DESC, id DESC LIMIT ?1)",
        params![MAX_ENTRIES],
    )?;
    Ok(())
}

pub fn push(conn: &Connection, device: &str, url: &str, title: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    upsert_visit(&tx, device, url, title, &Utc::now().to_rfc3339())?;
    prune(&tx)?;
    tx.commit()?;
    Ok(())
}

/// Met à jour le titre de la visite la plus récente de `url` (le titre d'un
/// document arrive souvent après la fin du chargement). Retourne `true` si
/// une ligne a changé.
pub fn retitle(conn: &Connection, device: &str, url: &str, title: &str) -> Result<bool> {
    let id: Option<i64> = conn
        .query_row(
            "SELECT id FROM history
             WHERE id = (SELECT max(id) FROM history WHERE url = ?1 AND deleted = 0) AND title <> ?2",
            params![url, title],
            |r| r.get(0),
        )
        .optional()?;
    let Some(id) = id else { return Ok(false) };
    let s = stamp(conn, device)?;
    let n = conn.execute(
        "UPDATE history SET title = ?2, updated_at = ?3, origin_device = ?4, local_hlc = ?3 WHERE id = ?1",
        params![id, title, s.hlc, s.device],
    )?;
    Ok(n > 0)
}

/// Import en bloc `(url, titre, secondes Unix)` : une transaction, sans
/// doublon exact (url + date) pour qu'un ré-import soit idempotent, puis
/// même plafond que `push` — en gardant les plus récentes par date.
/// Retourne le nombre de lignes réellement ajoutées.
pub fn import(conn: &Connection, device: &str, entries: &[(String, String, i64)]) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let mut added = 0;
    for (url, title, secs) in entries {
        let Some(at) = chrono::DateTime::<Utc>::from_timestamp(*secs, 0) else { continue };
        added += upsert_visit(&tx, device, url, title, &at.to_rfc3339())? as usize;
    }
    prune(&tx)?;
    tx.commit()?;
    Ok(added)
}

pub fn list_recent(conn: &Connection, limit: usize) -> Result<Vec<HistoryEntry>> {
    query(
        conn,
        "SELECT id, url, title, visited_at FROM history
         WHERE deleted = 0 ORDER BY visited_at DESC LIMIT ?1",
        params![limit as i64],
    )
}

pub fn search(conn: &Connection, query_str: &str) -> Result<Vec<HistoryEntry>> {
    let pattern = crate::db::like_pattern(query_str);
    query(
        conn,
        "SELECT id, url, title, visited_at FROM history
         WHERE deleted = 0 AND (url LIKE ?1 ESCAPE '\\' OR title LIKE ?1 ESCAPE '\\')
         ORDER BY visited_at DESC LIMIT 100",
        params![pattern],
    )
}

/// Efface tout l'historique : chaque visite devient une pierre tombale (url,
/// titre et date effacés) pour que l'effacement se propage aux autres
/// appareils.
pub fn clear(conn: &Connection, device: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let ids: Vec<i64> = {
        let mut stmt = tx.prepare("SELECT id FROM history WHERE deleted = 0 ORDER BY id")?;
        let it = stmt.query_map([], |r| r.get(0))?;
        it.collect::<rusqlite::Result<_>>()?
    };
    if !ids.is_empty() {
        let base = reserve(&tx, ids.len() as u64)? as i64;
        let mut stmt = tx.prepare(
            "UPDATE history SET deleted = 1, url = '', title = '', visited_at = '',
               updated_at = ?2, origin_device = ?3, local_hlc = ?2
             WHERE id = ?1",
        )?;
        for (i, id) in ids.iter().enumerate() {
            stmt.execute(params![id, base + i as i64, device])?;
        }
    }
    tx.commit()?;
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
