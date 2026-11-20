//! Métadonnées de synchronisation du coffre (schéma v3).
//!
//! Chaque ligne des tables synchronisées (`passwords`, `bookmarks`,
//! `history`, `settings`) porte :
//! * `uuid` — identité stable, la même sur tous les appareils ;
//! * `updated_at` — HLC de la dernière modification *logique* (voyage avec
//!   l'enregistrement, sert au « dernier écrivain gagne ») ;
//! * `deleted` — pierre tombale (le contenu est effacé, l'uuid reste) ;
//! * `origin_device` — appareil auteur de la dernière modification
//!   (départage deux HLC égales) ;
//! * `local_hlc` — HLC *locale* du dernier changement de la ligne dans CE
//!   coffre (écriture locale ou application d'un changement distant). Elle
//!   sert de curseur : un pair demande « tout ce qui a `local_hlc` > N ».
//!
//! Le protocole lui-même (réseau, chiffrement) vit dans `nyx-sync` ; ce
//! module n'expose que des opérations SQL atomiques et idempotentes.

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::hlc;

/// Durée de conservation des pierres tombales avant purge définitive.
pub const TOMBSTONE_TTL_MS: u64 = 90 * 24 * 3600 * 1000;

/// Préfixe des réglages propres à un appareil, jamais synchronisés.
pub const LOCAL_SETTING_PREFIX: &str = "local.";

/// Taille maximale (octets) des champs texte d'un enregistrement reçu.
const MAX_RECORD_BYTES: usize = 1 << 20;

// ── Tables ────────────────────────────────────────────────────────────────

/// Tables synchronisées.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Table {
    Passwords,
    Bookmarks,
    History,
    Settings,
}

impl Table {
    pub const ALL: [Table; 4] = [Table::Passwords, Table::Bookmarks, Table::History, Table::Settings];

    pub fn as_str(self) -> &'static str {
        match self {
            Table::Passwords => "passwords",
            Table::Bookmarks => "bookmarks",
            Table::History => "history",
            Table::Settings => "settings",
        }
    }
}

// ── Enregistrements ───────────────────────────────────────────────────────

/// Contenu d'un enregistrement, selon sa table. Les dates sont des chaînes
/// RFC 3339 transmises telles quelles (pas de reformatage → identité stable).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum RecordData {
    Password { domain: String, username: String, password: String, created_at: String },
    Bookmark { url: String, title: String, folder: String, tags: Vec<String>, created_at: String },
    History { url: String, title: String, visited_at: String },
    Setting { key: String, value: String },
}

impl RecordData {
    pub fn table(&self) -> Table {
        match self {
            RecordData::Password { .. } => Table::Passwords,
            RecordData::Bookmark { .. } => Table::Bookmarks,
            RecordData::History { .. } => Table::History,
            RecordData::Setting { .. } => Table::Settings,
        }
    }

    /// Contenu d'une pierre tombale : tout est effacé sauf ce qui identifie
    /// la ligne (la clé d'un réglage, non sensible).
    pub fn blanked(&self) -> RecordData {
        match self {
            RecordData::Password { .. } => RecordData::Password {
                domain: String::new(),
                username: String::new(),
                password: String::new(),
                created_at: String::new(),
            },
            RecordData::Bookmark { .. } => RecordData::Bookmark {
                url: String::new(),
                title: String::new(),
                folder: String::new(),
                tags: vec![],
                created_at: String::new(),
            },
            RecordData::History { .. } => {
                RecordData::History { url: String::new(), title: String::new(), visited_at: String::new() }
            }
            RecordData::Setting { key, .. } => RecordData::Setting { key: key.clone(), value: String::new() },
        }
    }

    fn byte_len(&self) -> usize {
        match self {
            RecordData::Password { domain, username, password, created_at } => {
                domain.len() + username.len() + password.len() + created_at.len()
            }
            RecordData::Bookmark { url, title, folder, tags, created_at } => {
                url.len() + title.len() + folder.len() + created_at.len() + tags.iter().map(String::len).sum::<usize>()
            }
            RecordData::History { url, title, visited_at } => url.len() + title.len() + visited_at.len(),
            RecordData::Setting { key, value } => key.len() + value.len(),
        }
    }
}

/// Un enregistrement synchronisable (une ligne d'une table synchronisée).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub uuid: String,
    pub updated_at: u64,
    pub deleted: bool,
    pub origin_device: String,
    pub data: RecordData,
}

impl Record {
    pub fn table(&self) -> Table {
        self.data.table()
    }

    /// Clé d'ordre total du « dernier écrivain gagne » : HLC, puis appareil.
    fn lww_key(&self) -> (u64, &str) {
        (self.updated_at, &self.origin_device)
    }
}

/// Un changement exporté : l'enregistrement et sa position (`local_hlc`)
/// dans le journal local, à reprendre comme filigrane par le pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub local_hlc: u64,
    pub record: Record,
}

/// Bilan d'une application de lot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ApplyStats {
    /// Enregistrements écrits (nouveaux ou plus récents que la copie locale).
    pub applied: usize,
    /// Déjà connus ou plus anciens que la copie locale (idempotence).
    pub skipped: usize,
    /// Refusés (malformés, HLC trop dans le futur, réglage local…).
    pub rejected: usize,
}

// ── Identifiants ──────────────────────────────────────────────────────────

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Identifiant aléatoire de 128 bits (32 caractères hexadécimaux).
pub(crate) fn random_uuid() -> Result<String> {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).map_err(|e| anyhow!("CSPRNG indisponible : {e}"))?;
    Ok(hex(&b))
}

fn derived_uuid(domain: &str, parts: &[&str]) -> String {
    let mut h = Sha256::new();
    h.update(domain.as_bytes());
    for p in parts {
        h.update([0u8]);
        h.update(p.as_bytes());
    }
    hex(&h.finalize()[..16])
}

/// Uuid déterministe d'une visite : deux appareils ayant la même visite
/// (même url, même date — ex. import Chrome des deux côtés) produisent le
/// même enregistrement → l'historique se fusionne comme un ensemble.
pub fn history_uuid(url: &str, visited_at: &str) -> String {
    derived_uuid("nyx-history-v1", &[url, visited_at])
}

/// Uuid déterministe d'un réglage : la clé EST l'identité.
pub fn setting_uuid(key: &str) -> String {
    derived_uuid("nyx-setting-v1", &[key])
}

fn valid_ident(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

// ── Horloge & identité de l'appareil ─────────────────────────────────────

/// Horodatage d'une écriture locale.
pub(crate) struct Stamp<'a> {
    pub hlc: i64,
    pub device: &'a str,
}

/// Événement local : avance l'horloge persistée et renvoie la nouvelle HLC.
/// Une seule instruction → atomique même entre plusieurs connexions.
pub(crate) fn tick(conn: &Connection) -> Result<u64> {
    let now = hlc::from_millis(hlc::now_millis()) as i64;
    let v: i64 = conn.query_row(
        "UPDATE sync_clock SET hlc = max(?1, hlc + 1) WHERE id = 1 RETURNING hlc",
        [now],
        |r| r.get(0),
    )?;
    Ok(v as u64)
}

/// Horodatage d'une nouvelle écriture locale par `device`.
pub(crate) fn stamp<'a>(conn: &Connection, device: &'a str) -> Result<Stamp<'a>> {
    Ok(Stamp { hlc: tick(conn)? as i64, device })
}

/// Réserve `n` ≥ 1 HLC consécutives ; renvoie la première.
pub(crate) fn reserve(conn: &Connection, n: u64) -> Result<u64> {
    let n = n.max(1) as i64;
    let now = hlc::from_millis(hlc::now_millis()) as i64;
    let v: i64 = conn.query_row(
        "UPDATE sync_clock SET hlc = max(?1, hlc + 1) + ?2 - 1 WHERE id = 1 RETURNING hlc - ?2 + 1",
        params![now, n],
        |r| r.get(0),
    )?;
    Ok(v as u64)
}

/// Réception d'une HLC distante : l'horloge locale passe au-dessus.
fn observe(conn: &Connection, remote: u64) -> Result<u64> {
    let now = hlc::from_millis(hlc::now_millis()) as i64;
    let v: i64 = conn.query_row(
        "UPDATE sync_clock SET hlc = max(?1, hlc + 1, ?2 + 1) WHERE id = 1 RETURNING hlc",
        params![now, remote as i64],
        |r| r.get(0),
    )?;
    Ok(v as u64)
}

pub(crate) fn device_id(conn: &Connection) -> Result<String> {
    let v: Vec<u8> = conn.query_row("SELECT value FROM sync_meta WHERE key = 'device_id'", [], |r| r.get(0))?;
    Ok(String::from_utf8(v)?)
}

// ── Migration v3 ──────────────────────────────────────────────────────────

const V3_TABLES: &str = "
CREATE TABLE IF NOT EXISTS sync_meta (
    key   TEXT PRIMARY KEY,
    value BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS sync_clock (
    id  INTEGER PRIMARY KEY CHECK (id = 1),
    hlc INTEGER NOT NULL
);
-- Filigranes : plus grande `local_hlc` DU PAIR déjà appliquée ici, par table.
CREATE TABLE IF NOT EXISTS sync_peers (
    device_id TEXT    NOT NULL,
    tbl       TEXT    NOT NULL,
    watermark INTEGER NOT NULL,
    PRIMARY KEY (device_id, tbl)
);
";

/// Ajoute les colonnes de synchronisation et remplit les lignes existantes :
/// uuid (aléatoire, ou dérivé pour l'historique et les réglages), HLC
/// distinctes et croissantes, appareil d'origine = cet appareil.
pub(crate) fn migrate_v3(conn: &Connection) -> Result<()> {
    conn.execute_batch(V3_TABLES)?;
    let device = random_uuid()?;
    conn.execute(
        "INSERT OR IGNORE INTO sync_meta (key, value) VALUES ('device_id', ?1)",
        [device.as_bytes()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO sync_clock (id, hlc) VALUES (1, ?1)",
        [hlc::from_millis(hlc::now_millis()) as i64],
    )?;
    let device = device_id(conn)?;

    for t in Table::ALL {
        let name = t.as_str();
        for (col, decl) in [
            ("uuid", "TEXT"),
            ("updated_at", "INTEGER NOT NULL DEFAULT 0"),
            ("deleted", "INTEGER NOT NULL DEFAULT 0"),
            ("origin_device", "TEXT NOT NULL DEFAULT ''"),
            ("local_hlc", "INTEGER NOT NULL DEFAULT 0"),
        ] {
            if !crate::db::has_column(conn, name, col)? {
                conn.execute(&format!("ALTER TABLE {name} ADD COLUMN {col} {decl}"), [])?;
            }
        }

        // Lignes existantes, dans l'ordre d'insertion.
        let rows: Vec<(i64, String, String)> = {
            let sql = match t {
                Table::History => "SELECT rowid, url, visited_at FROM history WHERE uuid IS NULL ORDER BY rowid",
                Table::Settings => "SELECT rowid, key, '' FROM settings WHERE uuid IS NULL ORDER BY rowid",
                Table::Passwords => "SELECT rowid, '', '' FROM passwords WHERE uuid IS NULL ORDER BY rowid",
                Table::Bookmarks => "SELECT rowid, '', '' FROM bookmarks WHERE uuid IS NULL ORDER BY rowid",
            };
            let mut stmt = conn.prepare(sql)?;
            let it = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            it.collect::<rusqlite::Result<_>>()?
        };
        if rows.is_empty() {
            continue;
        }
        let base = reserve(conn, rows.len() as u64)? as i64;
        let mut seen = std::collections::HashSet::new();
        for (i, (rowid, a, b)) in rows.iter().enumerate() {
            let uuid = match t {
                Table::History => history_uuid(a, b),
                Table::Settings => setting_uuid(a),
                _ => random_uuid()?,
            };
            if !seen.insert(uuid.clone()) {
                // Doublon exact d'une visite : on n'en garde qu'une.
                conn.execute(&format!("DELETE FROM {name} WHERE rowid = ?1"), [rowid])?;
                continue;
            }
            let h = base + i as i64;
            conn.execute(
                &format!(
                    "UPDATE {name} SET uuid = ?1, updated_at = ?2, local_hlc = ?2, origin_device = ?3
                     WHERE rowid = ?4"
                ),
                params![uuid, h, device, rowid],
            )?;
        }
    }
    for t in Table::ALL {
        let name = t.as_str();
        conn.execute_batch(&format!(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_{name}_uuid ON {name}(uuid);
             CREATE INDEX IF NOT EXISTS idx_{name}_local_hlc ON {name}(local_hlc);"
        ))?;
    }
    Ok(())
}

// ── Méta-données (clés d'appareil, profil…) ──────────────────────────────

pub(crate) fn meta_get(conn: &Connection, key: &str) -> Result<Option<Vec<u8>>> {
    Ok(conn
        .query_row("SELECT value FROM sync_meta WHERE key = ?1", [key], |r| r.get(0))
        .optional()?)
}

pub(crate) fn meta_set(conn: &Connection, key: &str, value: &[u8]) -> Result<()> {
    if key == "device_id" {
        return Err(anyhow!("device_id est immuable"));
    }
    conn.execute(
        "INSERT INTO sync_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub(crate) fn meta_delete(conn: &Connection, key: &str) -> Result<()> {
    if key == "device_id" {
        return Err(anyhow!("device_id est immuable"));
    }
    conn.execute("DELETE FROM sync_meta WHERE key = ?1", [key])?;
    Ok(())
}

// ── Filigranes ────────────────────────────────────────────────────────────

pub(crate) fn watermark(conn: &Connection, peer: &str, table: Table) -> Result<u64> {
    let v: Option<i64> = conn
        .query_row(
            "SELECT watermark FROM sync_peers WHERE device_id = ?1 AND tbl = ?2",
            params![peer, table.as_str()],
            |r| r.get(0),
        )
        .optional()?;
    Ok(v.unwrap_or(0) as u64)
}

fn set_watermark(conn: &Connection, peer: &str, table: Table, value: u64) -> Result<()> {
    // Ne recule jamais (un lot rejoué ne fait pas perdre de terrain).
    conn.execute(
        "INSERT INTO sync_peers (device_id, tbl, watermark) VALUES (?1, ?2, ?3)
         ON CONFLICT(device_id, tbl) DO UPDATE SET watermark = max(watermark, excluded.watermark)",
        params![peer, table.as_str(), value as i64],
    )?;
    Ok(())
}

pub(crate) fn forget_peer(conn: &Connection, peer: &str) -> Result<()> {
    conn.execute("DELETE FROM sync_peers WHERE device_id = ?1", [peer])?;
    Ok(())
}

// ── Export ────────────────────────────────────────────────────────────────

fn select_sql(t: Table) -> &'static str {
    match t {
        Table::Passwords => {
            "SELECT uuid, updated_at, deleted, origin_device, local_hlc, domain, username, password, created_at
             FROM passwords WHERE uuid IS NOT NULL AND "
        }
        Table::Bookmarks => {
            "SELECT uuid, updated_at, deleted, origin_device, local_hlc, url, title, folder, tags, created_at
             FROM bookmarks WHERE uuid IS NOT NULL AND "
        }
        Table::History => {
            "SELECT uuid, updated_at, deleted, origin_device, local_hlc, url, title, visited_at
             FROM history WHERE uuid IS NOT NULL AND "
        }
        Table::Settings => {
            "SELECT uuid, updated_at, deleted, origin_device, local_hlc, key, value
             FROM settings WHERE uuid IS NOT NULL AND substr(key, 1, 6) <> 'local.' AND "
        }
    }
}

fn change_from_row(t: Table, r: &Row) -> rusqlite::Result<Change> {
    let data = match t {
        Table::Passwords => RecordData::Password {
            domain: r.get(5)?,
            username: r.get(6)?,
            password: r.get(7)?,
            created_at: r.get(8)?,
        },
        Table::Bookmarks => RecordData::Bookmark {
            url: r.get(5)?,
            title: r.get(6)?,
            folder: r.get(7)?,
            tags: serde_json::from_str(&r.get::<_, String>(8)?).unwrap_or_default(),
            created_at: r.get(9)?,
        },
        Table::History => RecordData::History { url: r.get(5)?, title: r.get(6)?, visited_at: r.get(7)? },
        Table::Settings => RecordData::Setting { key: r.get(5)?, value: r.get(6)? },
    };
    Ok(Change {
        local_hlc: r.get::<_, i64>(4)? as u64,
        record: Record {
            uuid: r.get(0)?,
            updated_at: r.get::<_, i64>(1)? as u64,
            deleted: r.get::<_, i64>(2)? != 0,
            origin_device: r.get(3)?,
            data,
        },
    })
}

/// Changements de `t` postérieurs au curseur `after`, par `local_hlc`
/// croissante, au plus `limit` — sauf que toutes les lignes partageant la
/// `local_hlc` du dernier élément sont incluses, pour qu'un filigrane posé
/// sur cette valeur ne saute jamais de ligne.
pub(crate) fn changes_since(conn: &Connection, t: Table, after: u64, limit: usize) -> Result<Vec<Change>> {
    let limit = limit.clamp(1, i64::MAX as usize) as i64;
    let mut out: Vec<Change> = {
        let sql = format!("{} local_hlc > ?1 ORDER BY local_hlc, uuid LIMIT ?2", select_sql(t));
        let mut stmt = conn.prepare(&sql)?;
        let it = stmt.query_map(params![after as i64, limit], |r| change_from_row(t, r))?;
        it.collect::<rusqlite::Result<_>>()?
    };
    if out.len() as i64 == limit {
        if let Some(last) = out.last().map(|c| (c.local_hlc, c.record.uuid.clone())) {
            let sql = format!("{} local_hlc = ?1 AND uuid > ?2 ORDER BY uuid", select_sql(t));
            let mut stmt = conn.prepare(&sql)?;
            let it = stmt.query_map(params![last.0 as i64, last.1], |r| change_from_row(t, r))?;
            for c in it {
                out.push(c?);
            }
        }
    }
    Ok(out)
}

// ── Application ───────────────────────────────────────────────────────────

fn validate(r: &Record, now_ms: u64) -> bool {
    if !valid_ident(&r.uuid) || r.origin_device.len() > 64 || !hlc::is_plausible(r.updated_at, now_ms) {
        return false;
    }
    if r.updated_at > i64::MAX as u64 || r.data.byte_len() > MAX_RECORD_BYTES {
        return false;
    }
    match &r.data {
        RecordData::Setting { key, .. } => {
            !key.is_empty() && !key.starts_with(LOCAL_SETTING_PREFIX) && r.uuid == setting_uuid(key)
        }
        RecordData::History { url, visited_at, .. } => r.deleted || r.uuid == history_uuid(url, visited_at),
        _ => true,
    }
}

fn local_version(conn: &Connection, t: Table, uuid: &str) -> Result<Option<(u64, String)>> {
    Ok(conn
        .query_row(
            &format!("SELECT updated_at, origin_device FROM {} WHERE uuid = ?1", t.as_str()),
            [uuid],
            |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, String>(1)?)),
        )
        .optional()?)
}

fn write_record(conn: &Connection, r: &Record, local_hlc: u64) -> Result<()> {
    let data = if r.deleted { r.data.blanked() } else { r.data.clone() };
    let (u, d, o, l) = (&r.uuid, r.deleted as i64, &r.origin_device, local_hlc as i64);
    let at = r.updated_at as i64;
    match &data {
        RecordData::Password { domain, username, password, created_at } => conn.execute(
            "INSERT INTO passwords (uuid, domain, username, password, created_at, updated_at, deleted, origin_device, local_hlc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(uuid) DO UPDATE SET domain = excluded.domain, username = excluded.username,
               password = excluded.password, created_at = excluded.created_at, updated_at = excluded.updated_at,
               deleted = excluded.deleted, origin_device = excluded.origin_device, local_hlc = excluded.local_hlc",
            params![u, domain, username, password, created_at, at, d, o, l],
        )?,
        RecordData::Bookmark { url, title, folder, tags, created_at } => conn.execute(
            "INSERT INTO bookmarks (uuid, url, title, folder, tags, created_at, updated_at, deleted, origin_device, local_hlc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(uuid) DO UPDATE SET url = excluded.url, title = excluded.title, folder = excluded.folder,
               tags = excluded.tags, created_at = excluded.created_at, updated_at = excluded.updated_at,
               deleted = excluded.deleted, origin_device = excluded.origin_device, local_hlc = excluded.local_hlc",
            params![u, url, title, folder, serde_json::to_string(tags)?, created_at, at, d, o, l],
        )?,
        RecordData::History { url, title, visited_at } => conn.execute(
            "INSERT INTO history (uuid, url, title, visited_at, updated_at, deleted, origin_device, local_hlc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(uuid) DO UPDATE SET url = excluded.url, title = excluded.title,
               visited_at = excluded.visited_at, updated_at = excluded.updated_at, deleted = excluded.deleted,
               origin_device = excluded.origin_device, local_hlc = excluded.local_hlc",
            params![u, url, title, visited_at, at, d, o, l],
        )?,
        RecordData::Setting { key, value } => conn.execute(
            "INSERT INTO settings (key, value, uuid, updated_at, deleted, origin_device, local_hlc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, uuid = excluded.uuid,
               updated_at = excluded.updated_at, deleted = excluded.deleted,
               origin_device = excluded.origin_device, local_hlc = excluded.local_hlc",
            params![key, value, u, at, d, o, l],
        )?,
    };
    Ok(())
}

/// Applique un lot de changements distants (dernier écrivain gagne, par
/// enregistrement) dans UNE transaction ; si `cursor` est fourni
/// (`pair, table, filigrane`), le filigrane est avancé dans la même
/// transaction → une session interrompue reprend exactement où elle en
/// était, et rejouer un lot ne change rien.
pub(crate) fn apply(conn: &Connection, records: &[Record], cursor: Option<(&str, Table, u64)>) -> Result<ApplyStats> {
    let tx = conn.unchecked_transaction()?;
    let now = hlc::now_millis();
    let mut stats = ApplyStats::default();
    let mut touched_history = false;
    for r in records {
        if !validate(r, now) {
            stats.rejected += 1;
            continue;
        }
        let t = r.table();
        if let Some((at, origin)) = local_version(&tx, t, &r.uuid)? {
            if r.lww_key() <= (at, origin.as_str()) {
                stats.skipped += 1;
                continue;
            }
        }
        let local = observe(&tx, r.updated_at)?;
        write_record(&tx, r, local)?;
        touched_history |= t == Table::History;
        stats.applied += 1;
    }
    if touched_history {
        crate::history::prune(&tx)?;
    }
    if let Some((peer, t, upto)) = cursor {
        set_watermark(&tx, peer, t, upto)?;
    }
    tx.commit()?;
    Ok(stats)
}

/// Supprime définitivement les pierres tombales plus vieilles que `ttl_ms`.
pub(crate) fn purge_tombstones(conn: &Connection, ttl_ms: u64) -> Result<usize> {
    let cutoff = hlc::from_millis(hlc::now_millis().saturating_sub(ttl_ms)) as i64;
    let mut n = 0;
    for t in Table::ALL {
        n += conn.execute(
            &format!("DELETE FROM {} WHERE deleted = 1 AND updated_at < ?1", t.as_str()),
            [cutoff],
        )?;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuids_derives_stables_et_distincts() {
        assert_eq!(history_uuid("https://a", "t"), history_uuid("https://a", "t"));
        assert_ne!(history_uuid("https://a", "t"), history_uuid("https://a", "t2"));
        // Pas d'ambiguïté de concaténation.
        assert_ne!(history_uuid("ab", "c"), history_uuid("a", "bc"));
        assert_ne!(setting_uuid("x"), history_uuid("x", ""));
        assert_eq!(setting_uuid("k").len(), 32);
        assert!(valid_ident(&random_uuid().unwrap()));
    }

    #[test]
    fn validation() {
        let now = hlc::now_millis();
        let ok = Record {
            uuid: setting_uuid("k"),
            updated_at: hlc::from_millis(now),
            deleted: false,
            origin_device: "dev".into(),
            data: RecordData::Setting { key: "k".into(), value: "v".into() },
        };
        assert!(validate(&ok, now));
        let mut bad = ok.clone();
        bad.uuid = setting_uuid("autre");
        assert!(!validate(&bad, now), "uuid de réglage incohérent");
        let mut bad = ok.clone();
        bad.updated_at = hlc::from_millis(now + hlc::MAX_DRIFT_MS + 10);
        assert!(!validate(&bad, now), "HLC trop dans le futur");
        let mut bad = ok.clone();
        bad.data = RecordData::Setting { key: "local.x".into(), value: String::new() };
        bad.uuid = setting_uuid("local.x");
        assert!(!validate(&bad, now), "réglage local");
        let mut bad = ok;
        bad.uuid = "a'; DROP".into();
        assert!(!validate(&bad, now));
    }
}
