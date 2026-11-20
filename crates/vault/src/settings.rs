use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::sync::{setting_uuid, stamp};

pub fn get(conn: &Connection, key: &str) -> Result<Option<String>> {
    let value = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1 AND deleted = 0",
            params![key],
            |row| row.get(0),
        )
        .optional()?;
    Ok(value)
}

/// Upsert. Réécrire la même valeur ne crée pas de changement à synchroniser.
/// Les clés préfixées `local.` restent propres à l'appareil.
pub fn set(conn: &Connection, device: &str, key: &str, value: &str) -> Result<()> {
    let unchanged: bool = conn
        .query_row(
            "SELECT value = ?2 AND deleted = 0 FROM settings WHERE key = ?1",
            params![key, value],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(false);
    if unchanged {
        return Ok(());
    }
    let s = stamp(conn, device)?;
    conn.execute(
        "INSERT INTO settings (key, value, uuid, updated_at, origin_device, local_hlc)
         VALUES (?1, ?2, ?3, ?4, ?5, ?4)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, uuid = excluded.uuid, deleted = 0,
           updated_at = excluded.updated_at, origin_device = excluded.origin_device,
           local_hlc = excluded.local_hlc",
        params![key, value, setting_uuid(key), s.hlc, s.device],
    )?;
    Ok(())
}
