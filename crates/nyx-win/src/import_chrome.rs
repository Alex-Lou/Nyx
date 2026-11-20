//! « Importer depuis Chrome » (Windows) : favoris + historique d'un profil
//! de `%LOCALAPPDATA%\Google\Chrome\User Data`, via les parseurs de
//! `nyx_core::import` et `vault::read_chrome_history` (mêmes que le desktop).
//! Les mots de passe Chrome (chiffrés DPAPI) ne sont pas lus.
//!
//! Déclenché par `nyx://import/chrome?profile=<dossier>` depuis la page
//! interne Réglages (`Verdict::ImportChrome`) ; le profil demandé doit être
//! l'un des profils détectés (aucun chemin arbitraire).

use std::path::PathBuf;

use nyx_core::import::chrome::{self, ChromeProfile};
use nyx_core::state::bookmarks::{self, Bookmarks};
use vault::Vault;

const HISTORY_LIMIT: usize = 5000;

/// Profils Chrome détectés sous `user_data` (`None` → aucun).
pub fn profiles(user_data: Option<&str>) -> Vec<ChromeProfile> {
    user_data.map_or_else(Vec::new, |d| chrome::find_profiles(&[PathBuf::from(d)]))
}

/// Nom de dossier d'un profil (`Default`, `Profile 1`…).
pub fn dir_name(p: &ChromeProfile) -> String {
    p.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Profil désigné par `nyx://import/chrome?profile=…`, parmi les détectés.
pub fn select<'a>(url: &str, found: &'a [ChromeProfile]) -> Option<&'a ChromeProfile> {
    let query = url.split_once('?')?.1;
    let name = query
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == "profile")
        .map(|(_, v)| nyx_core::url::form_decode(v))?;
    found.iter().find(|p| dir_name(p) == name)
}

/// Importe favoris (dédup URL, store mémoire) et historique (coffre).
/// Retourne le résumé affiché à l'utilisateur.
pub fn run(profile: &ChromeProfile, bm: &Bookmarks, vault: &Vault) -> String {
    let (mut added, mut history, mut errors) = (0, 0, Vec::new());
    if profile.has_bookmarks {
        match std::fs::read_to_string(profile.dir.join("Bookmarks")) {
            Ok(json) => {
                for b in chrome::parse_bookmarks_json(&json) {
                    if bookmarks::add_in(bm, b.url, b.title, b.folder) {
                        added += 1;
                    }
                }
            }
            Err(e) => errors.push(format!("favoris : {e}")),
        }
    }
    if profile.has_history {
        match vault::read_chrome_history(&profile.dir.join("History"), HISTORY_LIMIT) {
            Ok(rows) => {
                let entries: Vec<(String, String, i64)> = rows
                    .into_iter()
                    .filter(|(url, _, _)| nyx_core::url::is_navigable(url))
                    .filter_map(|(url, title, t)| Some((url, title, chrome::chrome_time_to_unix(t)?)))
                    .collect();
                match vault.import_history(&entries) {
                    Ok(n) => history = n,
                    Err(e) => errors.push(format!("historique : {e}")),
                }
            }
            Err(e) => errors.push(format!("historique : {e:#} — fermez Chrome puis réessayez")),
        }
    }
    let mut text = format!("Chrome : {added} favoris, {history} pages d'historique importés");
    for e in errors {
        text.push_str(&format!(" · ⚠ {e}"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(name: &str) -> ChromeProfile {
        ChromeProfile {
            label: format!("{name} (Chrome)"),
            dir: PathBuf::from("/x/User Data").join(name),
            has_bookmarks: true,
            has_history: false,
        }
    }

    #[test]
    fn selects_only_detected_profiles() {
        let found = [profile("Default"), profile("Profile 1")];
        assert_eq!(select("nyx://import/chrome?profile=Default", &found).map(dir_name).as_deref(), Some("Default"));
        assert_eq!(
            select("nyx://import/chrome?t=1&profile=Profile+1", &found).map(dir_name).as_deref(),
            Some("Profile 1")
        );
        for bad in [
            "nyx://import/chrome",
            "nyx://import/chrome?profile=",
            "nyx://import/chrome?profile=..%5C..%5CWindows",
            "nyx://import/chrome?profile=C%3A%5CUsers",
            "nyx://import/chrome?profile=Profile%202x",
        ] {
            assert!(select(bad, &found).is_none(), "{bad}");
        }
        assert!(profiles(None).is_empty());
    }

    #[test]
    fn imports_bookmarks_json() {
        let dir = std::env::temp_dir().join(format!("nyx_win_chrome_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let prof = dir.join("Default");
        std::fs::create_dir_all(&prof).expect("dir");
        std::fs::write(
            prof.join("Bookmarks"),
            r#"{"roots":{"bookmark_bar":{"type":"folder","children":[{"type":"url","name":"R","url":"https://rust-lang.org/"}]}}}"#,
        )
        .expect("write");
        let found = profiles(dir.to_str());
        assert_eq!(found.len(), 1);
        let vault = Vault::open(&dir.join("v.db"), "pw").expect("vault");
        let bm = bookmarks::new();
        let text = run(&found[0], &bm, &vault);
        assert!(text.starts_with("Chrome : 1 favoris"), "{text}");
        assert_eq!(bm.borrow()[0].folder, "Barre de favoris");
        assert!(run(&found[0], &bm, &vault).starts_with("Chrome : 0 favoris"), "dédup");
        drop(vault);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
