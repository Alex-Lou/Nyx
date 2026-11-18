//! Données Chrome : favoris (`Bookmarks` JSON), timestamps et profils.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::state::bookmarks::Bookmark;

/// Secondes entre 1601-01-01 (epoch Chrome/WebKit) et 1970-01-01.
const CHROME_EPOCH_OFFSET_SECS: i64 = 11_644_473_600;

/// Temps Chrome (µs depuis 1601 UTC) → secondes Unix. `None` si 0/invalide.
pub fn chrome_time_to_unix(micros: i64) -> Option<i64> {
    if micros <= 0 {
        return None;
    }
    let secs = micros / 1_000_000 - CHROME_EPOCH_OFFSET_SECS;
    (secs > 0).then_some(secs)
}

/// Aplatit le fichier `Bookmarks` de Chrome. Le dossier Nyx est le chemin
/// complet depuis la racine ("Barre de favoris/Dev"). URLs non navigables
/// (`javascript:`, `chrome://`…) ignorées.
pub fn parse_bookmarks_json(json: &str) -> Vec<Bookmark> {
    let Ok(root) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    let mut out = Vec::new();
    for (key, label) in [
        ("bookmark_bar", "Barre de favoris"),
        ("other", "Autres favoris"),
        ("synced", "Favoris sur mobile"),
    ] {
        if let Some(node) = root.pointer(&format!("/roots/{key}")) {
            walk(node, label, &mut out);
        }
    }
    out
}

fn walk(node: &Value, folder: &str, out: &mut Vec<Bookmark>) {
    let Some(children) = node.get("children").and_then(Value::as_array) else { return };
    for child in children {
        let name = child.get("name").and_then(Value::as_str).unwrap_or("");
        match child.get("type").and_then(Value::as_str) {
            Some("url") => {
                let url = child.get("url").and_then(Value::as_str).unwrap_or("");
                if crate::url::is_navigable(url) {
                    let title = if name.is_empty() { url } else { name };
                    out.push(Bookmark {
                        url: url.to_string(),
                        title: title.to_string(),
                        folder: folder.to_string(),
                    });
                }
            }
            Some("folder") => {
                // `/` est le séparateur de chemin Nyx : on l'évite dans un nom.
                let seg = name.replace('/', "-");
                let sub = if seg.is_empty() { folder.to_string() } else { format!("{folder}/{seg}") };
                walk(child, &sub, out);
            }
            _ => {}
        }
    }
}

/// Un profil Chrome/Chromium détecté sur disque.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromeProfile {
    /// Libellé affiché : "Default (Chrome Windows : alice)".
    pub label: String,
    pub dir: PathBuf,
    pub has_bookmarks: bool,
    pub has_history: bool,
}

/// Dossiers de base candidats : Linux natif (`~/.config/…`) et Windows via
/// WSL (`<users_root>/*/AppData/Local/Google/Chrome/User Data`).
pub fn candidate_bases(home: Option<&Path>, users_root: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(h) = home {
        v.push(h.join(".config/google-chrome"));
        v.push(h.join(".config/chromium"));
    }
    if let Ok(rd) = std::fs::read_dir(users_root) {
        let mut users: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        users.sort();
        v.extend(users.into_iter().map(|u| u.join("AppData/Local/Google/Chrome/User Data")));
    }
    v
}

/// Profils (sous-dossiers contenant `Bookmarks` ou `History`) de chaque base.
pub fn find_profiles(bases: &[PathBuf]) -> Vec<ChromeProfile> {
    let mut out = Vec::new();
    for base in bases {
        let Ok(rd) = std::fs::read_dir(base) else { continue };
        let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        dirs.sort();
        for dir in dirs {
            let (has_bookmarks, has_history) = (dir.join("Bookmarks").is_file(), dir.join("History").is_file());
            if !has_bookmarks && !has_history {
                continue;
            }
            let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            out.push(ChromeProfile {
                label: format!("{name} ({})", base_label(base)),
                dir,
                has_bookmarks,
                has_history,
            });
        }
    }
    out
}

fn base_label(base: &Path) -> String {
    let s = base.to_string_lossy();
    if let Some(rest) = s.split("/Users/").nth(1) {
        let user = rest.split('/').next().unwrap_or("");
        return format!("Chrome Windows : {user}");
    }
    if s.contains("chromium") { "Chromium".into() } else { "Chrome".into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome_time() {
        // 2020-01-01T00:00:00Z = 1577836800 s Unix.
        assert_eq!(chrome_time_to_unix((1_577_836_800 + CHROME_EPOCH_OFFSET_SECS) * 1_000_000), Some(1_577_836_800));
        assert_eq!(chrome_time_to_unix(0), None);
        assert_eq!(chrome_time_to_unix(-5), None);
        assert_eq!(chrome_time_to_unix(1_000_000), None); // avant 1970
    }

    const JSON: &str = r#"{"roots":{
      "bookmark_bar":{"type":"folder","name":"Bookmarks bar","children":[
        {"type":"url","name":"Rust","url":"https://rust-lang.org/"},
        {"type":"folder","name":"Dev","children":[
          {"type":"url","name":"","url":"https://docs.rs/"},
          {"type":"folder","name":"A/B","children":[{"type":"url","name":"x","url":"http://x.org"}]}
        ]},
        {"type":"url","name":"bad","url":"javascript:alert(1)"}
      ]},
      "other":{"type":"folder","name":"Other","children":[{"type":"url","name":"o","url":"https://o.com"}]},
      "synced":{"type":"folder","name":"Mobile","children":[]}
    }}"#;

    #[test]
    fn bookmarks_flatten() {
        let b = parse_bookmarks_json(JSON);
        let got: Vec<(&str, &str, &str)> =
            b.iter().map(|b| (b.folder.as_str(), b.title.as_str(), b.url.as_str())).collect();
        assert_eq!(got, vec![
            ("Barre de favoris", "Rust", "https://rust-lang.org/"),
            ("Barre de favoris/Dev", "https://docs.rs/", "https://docs.rs/"),
            ("Barre de favoris/Dev/A-B", "x", "http://x.org"),
            ("Autres favoris", "o", "https://o.com"),
        ]);
    }

    #[test]
    fn bookmarks_garbage() {
        assert!(parse_bookmarks_json("nope").is_empty());
        assert!(parse_bookmarks_json("{}").is_empty());
    }

    #[test]
    fn profiles_discovery() {
        let tmp = std::env::temp_dir().join(format!("nyx_import_prof_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let ud = tmp.join("Users/alice/AppData/Local/Google/Chrome/User Data");
        for (d, f) in [("Default", "Bookmarks"), ("Profile 1", "History"), ("Cache", "x")] {
            std::fs::create_dir_all(ud.join(d)).unwrap();
            std::fs::write(ud.join(d).join(f), "").unwrap();
        }
        let home = tmp.join("home");
        let bases = candidate_bases(Some(&home), &tmp.join("Users"));
        assert!(bases.contains(&ud));
        assert!(bases.contains(&home.join(".config/chromium")));
        let p = find_profiles(&bases);
        let names: Vec<&str> = p.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(names, ["Default (Chrome Windows : alice)", "Profile 1 (Chrome Windows : alice)"]);
        assert!(p[0].has_bookmarks && !p[0].has_history);
        assert!(!p[1].has_bookmarks && p[1].has_history);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn missing_dirs_are_ok() {
        assert!(candidate_bases(None, Path::new("/nonexistent-nyx")).is_empty());
        assert!(find_profiles(&[PathBuf::from("/nonexistent-nyx")]).is_empty());
    }
}
