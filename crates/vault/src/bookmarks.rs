use anyhow::Result;
use chrono::Utc;
use std::collections::{HashMap, VecDeque};
use rusqlite::{params, Connection, Params, Row};

use crate::models::Bookmark;
use crate::sync::{random_uuid, stamp};

pub fn add(conn: &Connection, device: &str, b: &Bookmark) -> Result<i64> {
    let tags = serde_json::to_string(&b.tags)?;
    let s = stamp(conn, device)?;
    conn.execute(
        "INSERT INTO bookmarks (url, title, folder, tags, created_at, uuid, updated_at, origin_device, local_hlc)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?7)",
        params![b.url, b.title, b.folder, tags, b.created_at.to_rfc3339(), random_uuid()?, s.hlc, s.device],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list(conn: &Connection) -> Result<Vec<Bookmark>> {
    query(
        conn,
        "SELECT id, url, title, folder, tags, created_at FROM bookmarks
         WHERE deleted = 0 ORDER BY created_at DESC",
        [],
    )
}

pub fn search(conn: &Connection, query_str: &str) -> Result<Vec<Bookmark>> {
    let pattern = crate::db::like_pattern(query_str);
    query(
        conn,
        "SELECT id, url, title, folder, tags, created_at FROM bookmarks
         WHERE deleted = 0 AND (url LIKE ?1 ESCAPE '\\' OR title LIKE ?1 ESCAPE '\\')
         ORDER BY created_at DESC",
        params![pattern],
    )
}

/// Suppression = pierre tombale (contenu effacé, uuid conservé) pour que
/// les autres appareils l'apprennent à la prochaine synchronisation.
pub fn delete(conn: &Connection, device: &str, id: i64) -> Result<()> {
    let s = stamp(conn, device)?;
    tombstone(conn, id, s.hlc, s.device)
}

fn tombstone(conn: &Connection, id: i64, hlc: i64, device: &str) -> Result<()> {
    conn.execute(
        "UPDATE bookmarks SET deleted = 1, url = '', title = '', folder = '', tags = '[]', created_at = '',
           updated_at = ?2, origin_device = ?3, local_hlc = ?2
         WHERE id = ?1 AND deleted = 0",
        params![id, hlc, device],
    )?;
    Ok(())
}

/// Remplace tout le contenu de la table (sync complète du store mémoire).
///
/// Implémenté comme un diff par url pour garder l'identité (`uuid`) des
/// favoris inchangés : une ligne existante est reprise (et horodatée
/// seulement si son titre/dossier/tags changent), les nouvelles sont
/// insérées, les disparues deviennent des pierres tombales. `created_at`
/// d'une url déjà connue est préservé.
pub fn replace_all(conn: &Connection, device: &str, items: &[Bookmark]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    type Live = (i64, String, String, String);
    // url → lignes vivantes existantes (id, title, folder, tags).
    let mut prior: HashMap<String, VecDeque<Live>> = HashMap::new();
    {
        let mut stmt = tx.prepare("SELECT id, url, title, folder, tags FROM bookmarks WHERE deleted = 0 ORDER BY id")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(1)?, (r.get(0)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        })?;
        for row in rows {
            let (url, live) = row?;
            prior.entry(url).or_default().push_back(live);
        }
    }
    for b in items {
        let tags = serde_json::to_string(&b.tags)?;
        match prior.get_mut(&b.url).and_then(VecDeque::pop_front) {
            Some((id, title, folder, old_tags)) => {
                if title != b.title || folder != b.folder || old_tags != tags {
                    let s = stamp(&tx, device)?;
                    tx.execute(
                        "UPDATE bookmarks SET title = ?2, folder = ?3, tags = ?4,
                           updated_at = ?5, origin_device = ?6, local_hlc = ?5
                         WHERE id = ?1",
                        params![id, b.title, b.folder, tags, s.hlc, s.device],
                    )?;
                }
            }
            None => {
                add(&tx, device, b)?;
            }
        }
    }
    for (id, ..) in prior.into_values().flatten() {
        let s = stamp(&tx, device)?;
        tombstone(&tx, id, s.hlc, s.device)?;
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
