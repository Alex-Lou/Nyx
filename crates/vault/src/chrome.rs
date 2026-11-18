//! Lecture de l'historique Chrome (SQLite en clair, pas SQLCipher).

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

/// Lit `urls(url, title, last_visit_time)` du fichier `History` de Chrome :
/// les `limit` pages http(s) les plus récentes, `(url, titre, temps Chrome µs)`.
/// Le fichier est copié d'abord (Chrome le verrouille) puis ouvert en lecture
/// seule ; une base sans clé s'ouvre normalement sous SQLCipher.
pub fn read_chrome_history(history: &Path, limit: usize) -> Result<Vec<(String, String, i64)>> {
    let tmp = std::env::temp_dir().join(format!("nyx-chrome-history-{}.sqlite", std::process::id()));
    std::fs::copy(history, &tmp)
        .with_context(|| format!("copie de {} impossible (Chrome ouvert ?)", history.display()))?;
    let res = read_rows(&tmp, limit);
    let _ = std::fs::remove_file(&tmp);
    res
}

fn read_rows(path: &Path, limit: usize) -> Result<Vec<(String, String, i64)>> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt = conn.prepare(
        "SELECT url, title, last_visit_time FROM urls
         WHERE url LIKE 'http://%' OR url LIKE 'https://%'
         ORDER BY last_visit_time DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit as i64], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default(), r.get::<_, i64>(2)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_plain_sqlite_sorted_and_filtered() {
        let p = std::env::temp_dir().join(format!("nyx_chrome_hist_test_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        {
            let c = Connection::open(&p).unwrap();
            c.execute_batch(
                "CREATE TABLE urls (id INTEGER PRIMARY KEY, url TEXT, title TEXT, last_visit_time INTEGER);
                 INSERT INTO urls (url, title, last_visit_time) VALUES
                   ('https://old.com', 'Old', 100),
                   ('chrome://settings', 'S', 300),
                   ('https://new.com', NULL, 200);",
            )
            .unwrap();
        }
        let rows = read_chrome_history(&p, 10).unwrap();
        assert_eq!(rows, vec![
            ("https://new.com".to_string(), String::new(), 200),
            ("https://old.com".to_string(), "Old".to_string(), 100),
        ]);
        assert_eq!(read_chrome_history(&p, 1).unwrap().len(), 1);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn missing_file_errors() {
        assert!(read_chrome_history(Path::new("/nonexistent/History"), 5).is_err());
    }
}
