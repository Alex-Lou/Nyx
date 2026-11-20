//! Import du coffre de la capsule WSL (Nyx Linux) dans le coffre Windows.
//!
//! Le coffre source n'est JAMAIS ouvert en place : il est copié (avec son
//! WAL éventuel) dans le dossier temporaire, la copie est ouverte avec la
//! passphrase de la capsule, fusionnée, puis supprimée. Fusion par les API
//! `vault`, sans doublon : mots de passe (hôte + identifiant), favoris (URL),
//! historique (URL + date), réglages (seulement s'ils manquent).
//!
//! Testé sur l'hôte avec de vrais coffres SQLCipher.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use vault::{Key, Vault};

use crate::paths;

/// Clé des réglages applicatifs (format query-string `to_query`).
pub const SETTINGS_KEY: &str = "app_settings";
/// Plafond d'historique lu (aligné sur celui du coffre).
const HISTORY_LIMIT: usize = 5000;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Summary {
    pub passwords: usize,
    pub bookmarks: usize,
    pub history: usize,
    pub settings: bool,
    /// Entrées déjà présentes (ou invalides) laissées de côté.
    pub skipped: usize,
}

impl Summary {
    pub fn text(&self) -> String {
        format!(
            "Capsule importée : {} mots de passe, {} favoris, {} pages d'historique{} ({} déjà présents)",
            self.passwords,
            self.bookmarks,
            self.history,
            if self.settings { ", réglages" } else { "" },
            self.skipped
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Copie impossible (WSL arrêté entre-temps, droits…).
    Copy(String),
    /// La copie ne s'ouvre pas avec cette passphrase.
    BadPassphrase,
    /// Écriture dans le coffre Windows.
    Write(String),
}

impl Error {
    pub fn message(&self) -> String {
        match self {
            Self::Copy(e) => format!(
                "Lecture de la capsule impossible ({e}). Démarrez-la avec « {} » puis réessayez.",
                paths::START_CAPSULE_CMD
            ),
            Self::BadPassphrase => "Passphrase de la capsule invalide".into(),
            Self::Write(e) => format!("Import interrompu : {e}"),
        }
    }
}

/// Copie temporaire ouverte du coffre de la capsule ; fichiers supprimés à
/// la destruction.
pub struct CapsuleCopy {
    vault: Option<Vault>,
    path: PathBuf,
}

impl CapsuleCopy {
    /// Copie `source` (+ `-wal`) dans `temp_dir` puis l'ouvre avec `key`.
    pub fn open(source: &Path, temp_dir: &Path, key: &Key) -> Result<Self, Error> {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let path = paths::capsule_temp_copy(temp_dir, std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed));
        // Le guard nettoie aussi en cas d'échec ci-dessous.
        let mut copy = Self { vault: None, path };
        std::fs::copy(source, &copy.path).map_err(|e| Error::Copy(e.to_string()))?;
        let wal = paths::sidecar(source, "-wal");
        if wal.is_file() {
            std::fs::copy(&wal, paths::sidecar(&copy.path, "-wal")).map_err(|e| Error::Copy(e.to_string()))?;
        }
        // Jamais de création : une clé fausse ne doit pas « réussir ».
        if !Vault::check_key(&copy.path, key) {
            return Err(Error::BadPassphrase);
        }
        copy.vault = Some(Vault::open_with(&copy.path, key).map_err(|_| Error::BadPassphrase)?);
        Ok(copy)
    }

    pub fn vault(&self) -> &Vault {
        self.vault.as_ref().expect("CapsuleCopy::open garantit le coffre")
    }
}

impl Drop for CapsuleCopy {
    fn drop(&mut self) {
        drop(self.vault.take()); // ferme la connexion avant suppression
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(paths::sidecar(&self.path, suffix));
        }
    }
}

/// Fusionne `src` dans `dst` sans doublon. `src` n'est que lu.
pub fn merge(src: &Vault, dst: &Vault) -> Result<Summary, Error> {
    let w = |e: anyhow::Error| Error::Write(format!("{e:#}"));
    let mut s = Summary::default();

    // Mots de passe : même hôte + identifiant → on garde l'existant.
    for domain in src.list_password_domains().map_err(w)? {
        let existing = dst.passwords_for(&domain).map_err(w)?;
        for p in src.passwords_for(&domain).map_err(w)? {
            if p.password.is_empty() || existing.iter().any(|e| e.username == p.username) {
                s.skipped += 1;
                continue;
            }
            let mut copy = vault::Password::new(p.domain, p.username, p.password);
            copy.created_at = p.created_at;
            dst.add_password(&copy).map_err(w)?;
            s.passwords += 1;
        }
    }

    // Favoris : dédup par URL, schemes navigables uniquement.
    let mut known: HashSet<String> = dst.list_bookmarks().map_err(w)?.into_iter().map(|b| b.url).collect();
    let mut incoming = src.list_bookmarks().map_err(w)?;
    incoming.reverse(); // plus anciens d'abord : l'ordre d'origine est conservé
    for b in incoming {
        if !nyx_core::url::is_navigable(&b.url) || !known.insert(b.url.clone()) {
            s.skipped += 1;
            continue;
        }
        dst.add_bookmark(&vault::Bookmark { id: None, ..b }).map_err(w)?;
        s.bookmarks += 1;
    }

    // Historique : dates conservées, ré-import idempotent (url + date).
    let entries: Vec<(String, String, i64)> = src
        .recent_history(HISTORY_LIMIT)
        .map_err(w)?
        .into_iter()
        .filter(|h| crate::history::recordable(&h.url))
        .map(|h| (h.url, h.title, h.visited_at.timestamp()))
        .collect();
    s.history = dst.import_history(&entries).map_err(w)?;
    s.skipped += entries.len() - s.history;

    // Réglages : jamais d'écrasement de ceux déjà choisis sous Windows.
    if dst.setting(SETTINGS_KEY).map_err(w)?.is_none() {
        if let Some(q) = src.setting(SETTINGS_KEY).map_err(w)? {
            dst.set_setting(SETTINGS_KEY, &q).map_err(w)?;
            s.settings = true;
        }
    }
    Ok(s)
}

/// Copie + ouverture + fusion, en une étape.
pub fn import_capsule(source: &Path, temp_dir: &Path, key: &Key, dst: &Vault) -> Result<Summary, Error> {
    let copy = CapsuleCopy::open(source, temp_dir, key)?;
    merge(copy.vault(), dst)
}

/// Clé d'ouverture de la capsule : sa passphrase si saisie, sinon sa clé
/// d'appareil Linux (`vault.key` à côté du coffre, lisible par le même
/// utilisateur via `\\wsl…`). `None` : rien d'utilisable, demander la
/// passphrase. Indépendant du mode de clé du coffre Windows.
pub fn capsule_key(source: &Path, passphrase: &str) -> Option<Key> {
    if !passphrase.is_empty() {
        return Some(Key::passphrase(passphrase));
    }
    let keyfile = vault::keyfile::path_for(source);
    vault::keyfile::read(&keyfile, &vault::keyfile::OsAccountOnly).ok().map(Key::Raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault::{Bookmark, Password};

    /// Dossier temporaire unique, supprimé à la fin.
    struct Tmp(PathBuf);

    impl Tmp {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir().join(format!("nyx_win_migrate_{}_{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).expect("tmp");
            Self(p)
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn capsule(dir: &Path) -> PathBuf {
        let path = dir.join("capsule.db");
        let v = Vault::open(&path, "old-pass").expect("capsule");
        v.add_password(&Password::new("example.com", "alex", "pw1")).expect("pw");
        v.add_password(&Password::new("example.com", "sam", "pw2")).expect("pw");
        v.add_password(&Password::new("other.org", "lou", "pw3")).expect("pw");
        v.add_bookmark(&Bookmark::new("https://rust-lang.org/", "Rust").in_folder("Dev")).expect("bm");
        v.add_bookmark(&Bookmark::new("https://dup.org/", "Dup")).expect("bm");
        v.add_bookmark(&Bookmark::new("javascript:alert(1)", "bad")).expect("bm");
        v.import_history(&[
            ("https://a.org/".into(), "A".into(), 1_700_000_000),
            ("https://b.org/".into(), "B".into(), 1_700_000_100),
        ])
        .expect("hist");
        v.push_history("file:///etc/passwd", "x").expect("hist");
        v.set_setting(SETTINGS_KEY, "engine=brave&savepw=true").expect("set");
        path
    }

    #[test]
    fn imports_and_dedupes() {
        let tmp = Tmp::new("merge");
        let source = capsule(&tmp.0);
        let before = std::fs::read(&source).expect("read");
        let dst = Vault::open(&tmp.0.join("windows.db"), "new-pass").expect("dst");
        dst.add_password(&Password::new("example.com", "alex", "kept")).expect("pw");
        dst.add_bookmark(&Bookmark::new("https://dup.org/", "Mine")).expect("bm");

        let temp = tmp.0.join("t");
        std::fs::create_dir_all(&temp).expect("t");
        let s = import_capsule(&source, &temp, &Key::passphrase("old-pass"), &dst).expect("import");
        assert_eq!((s.passwords, s.bookmarks, s.history, s.settings), (2, 1, 2, true), "{s:?}");
        assert!(s.text().contains("2 mots de passe"));

        let alex = dst.passwords_for("example.com").expect("pw");
        assert_eq!(alex.iter().find(|p| p.username == "alex").map(|p| p.password.as_str()), Some("kept"));
        assert_eq!(alex.len(), 2);
        let bms = dst.list_bookmarks().expect("bm");
        assert_eq!(bms.len(), 2);
        assert!(bms.iter().any(|b| b.url == "https://rust-lang.org/" && b.folder == "Dev"));
        assert_eq!(bms.iter().find(|b| b.url == "https://dup.org/").map(|b| b.title.as_str()), Some("Mine"));
        let hist = dst.recent_history(10).expect("hist");
        assert_eq!(hist.len(), 2);
        assert_eq!(hist[0].visited_at.timestamp(), 1_700_000_100);
        assert_eq!(dst.setting(SETTINGS_KEY).expect("set").as_deref(), Some("engine=brave&savepw=true"));

        // Ré-import : rien de nouveau, réglages Windows préservés.
        dst.set_setting(SETTINGS_KEY, "engine=ecosia").expect("set");
        let again = import_capsule(&source, &temp, &Key::passphrase("old-pass"), &dst).expect("again");
        assert_eq!((again.passwords, again.bookmarks, again.history, again.settings), (0, 0, 0, false));
        assert_eq!(dst.setting(SETTINGS_KEY).expect("set").as_deref(), Some("engine=ecosia"));

        // Source intacte, copies temporaires nettoyées.
        assert_eq!(std::fs::read(&source).expect("read"), before);
        assert_eq!(std::fs::read_dir(&temp).expect("dir").count(), 0);
    }

    #[test]
    fn wrong_passphrase_and_missing_source() {
        let tmp = Tmp::new("errors");
        let source = capsule(&tmp.0);
        let dst = Vault::open(&tmp.0.join("windows.db"), "new").expect("dst");
        assert_eq!(import_capsule(&source, &tmp.0, &Key::passphrase("nope"), &dst), Err(Error::BadPassphrase));
        assert!(dst.list_bookmarks().expect("bm").is_empty());
        let missing = import_capsule(&tmp.0.join("absent.db"), &tmp.0, &Key::passphrase("old-pass"), &dst);
        assert!(matches!(missing, Err(Error::Copy(_))));
        assert!(missing.unwrap_err().message().contains("wsl -d NyxCapsule -e true"));
        // Aucun reste de copie temporaire.
        let leftovers = std::fs::read_dir(&tmp.0)
            .expect("dir")
            .filter(|e| e.as_ref().is_ok_and(|e| e.file_name().to_string_lossy().starts_with("nyx-capsule-")))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn device_key_capsule() {
        let tmp = Tmp::new("devkey");
        let ks = vault::keymode::KeyStore::new(tmp.0.join("vault.db"), vault::keyfile::OsAccountOnly);
        let source = ks.db_path().to_path_buf();
        {
            let v = ks.create_with_device_key().expect("capsule");
            v.add_bookmark(&Bookmark::new("https://k.org/", "K")).expect("bm");
        }
        // Passphrase saisie : prioritaire (et fausse ici).
        let typed = capsule_key(&source, "typed").expect("key");
        assert!(typed.is_passphrase());
        let dst = Vault::open(&tmp.0.join("windows.db"), "new").expect("dst");
        assert_eq!(import_capsule(&source, &tmp.0, &typed, &dst), Err(Error::BadPassphrase));
        // Vide : clé d'appareil de la capsule.
        let key = capsule_key(&source, "").expect("keyfile");
        let s = import_capsule(&source, &tmp.0, &key, &dst).expect("import");
        assert_eq!(s.bookmarks, 1);
        // Sans fichier de clé : rien.
        vault::keyfile::remove(ks.keyfile_path()).expect("rm");
        assert!(capsule_key(&source, "").is_none());
    }
}
