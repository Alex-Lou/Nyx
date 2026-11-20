//! Mode de clé du coffre — clé d'appareil (défaut) ou mot de passe principal.
//!
//! Le coffre est TOUJOURS chiffré par SQLCipher. Seule change l'origine de
//! la clé :
//! • **clé d'appareil** : 32 octets aléatoires dans `vault.key`, protégés
//!   par le compte de l'OS (droits Unix / DPAPI Windows). Ouverture
//!   silencieuse, comme Chrome ou Firefox sans mot de passe principal ;
//! • **mot de passe principal** : passphrase saisie à chaque lancement,
//!   aucun fichier de clé.
//!
//! Détection au démarrage ([`detect`]) : pas de coffre → premier lancement ;
//! coffre + `vault.key` → clé d'appareil ; coffre seul → mot de passe
//! principal (comportement historique). Rien n'est jamais supprimé
//! automatiquement : un fichier de clé illisible fait retomber sur la saisie
//! du mot de passe principal.
//!
//! Transitions ([`KeyStore`]) : chaque changement est vérifié en rouvrant le
//! coffre avec la nouvelle clé ; en cas d'échec, l'état précédent est
//! restauré. Le fichier de clé n'est supprimé qu'APRÈS un rechiffrement
//! vérifié, et écrit AVANT de rechiffrer vers une clé d'appareil.

use std::path::{Path, PathBuf};

use crate::key::{Key, Passphrase, RawKey};
use crate::keyfile::{self, Seal};
use crate::Vault;

/// Longueur minimale d'un nouveau mot de passe principal (en caractères).
pub const MIN_PASSPHRASE_CHARS: usize = 8;

/// Ce que l'application doit faire au démarrage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Startup {
    /// Aucun coffre : écran de bienvenue.
    FirstLaunch,
    /// Coffre + fichier de clé : ouverture silencieuse.
    DeviceKey,
    /// Coffre sans fichier de clé : demander le mot de passe principal.
    MasterPassword,
}

/// Décision pure à partir de la présence des deux fichiers.
pub fn detect(db_exists: bool, keyfile_exists: bool) -> Startup {
    match (db_exists, keyfile_exists) {
        (false, _) => Startup::FirstLaunch,
        (true, true) => Startup::DeviceKey,
        (true, false) => Startup::MasterPassword,
    }
}

/// Contrôle d'un nouveau mot de passe principal. Message affiché tel quel.
pub fn check_new_passphrase(passphrase: &str, confirm: &str) -> Result<(), &'static str> {
    if passphrase.trim().is_empty() {
        Err("Le mot de passe est vide")
    } else if passphrase.chars().count() < MIN_PASSPHRASE_CHARS {
        Err("Le mot de passe doit faire au moins 8 caractères")
    } else if passphrase != confirm {
        Err("Les mots de passe ne correspondent pas")
    } else {
        Ok(())
    }
}

/// Échec d'une opération de clé. Les messages ne contiennent jamais de clé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    /// Mot de passe principal incorrect.
    WrongPassphrase,
    /// Fichier de clé absent, illisible, d'un autre compte, ou ne
    /// correspondant pas au coffre.
    DeviceKey(String),
    /// Le coffre existe déjà (création refusée).
    AlreadyExists,
    /// Opération incohérente avec le mode courant.
    NoMasterPassword,
    HasMasterPassword,
    /// Autre échec (E/S, SQLCipher) ; état précédent conservé.
    Failed(String),
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongPassphrase => f.write_str("Mot de passe principal incorrect"),
            Self::DeviceKey(e) => write!(f, "La clé de cet appareil n'ouvre pas le coffre ({e})"),
            Self::AlreadyExists => f.write_str("Un coffre existe déjà"),
            Self::NoMasterPassword => f.write_str("Aucun mot de passe principal n'est défini"),
            Self::HasMasterPassword => f.write_str("Un mot de passe principal est déjà défini"),
            Self::Failed(e) => write!(f, "Opération impossible : {e}"),
        }
    }
}

impl std::error::Error for KeyError {}

fn failed(e: anyhow::Error) -> KeyError {
    KeyError::Failed(format!("{e:#}"))
}

/// Coffre + fichier de clé + protection OS du fichier.
pub struct KeyStore<S: Seal> {
    db: PathBuf,
    keyfile: PathBuf,
    seal: S,
}

impl<S: Seal> KeyStore<S> {
    /// Fichier de clé : `vault.key` à côté de `db`.
    pub fn new(db: PathBuf, seal: S) -> Self {
        let keyfile = keyfile::path_for(&db);
        Self { db, keyfile, seal }
    }

    pub fn db_path(&self) -> &Path {
        &self.db
    }

    pub fn keyfile_path(&self) -> &Path {
        &self.keyfile
    }

    pub fn startup(&self) -> Startup {
        detect(self.db.exists(), self.keyfile.exists())
    }

    // ── Création (premier lancement) ────────────────────────────────────

    /// Crée le coffre avec une nouvelle clé d'appareil (« Plus tard »).
    /// Le fichier de clé est écrit puis relu AVANT la création du coffre.
    pub fn create_with_device_key(&self) -> Result<Vault, KeyError> {
        self.ensure_absent()?;
        self.set_aside_orphan_keyfile()?;
        let raw = RawKey::generate().map_err(failed)?;
        self.write_keyfile_checked(&raw)?;
        Vault::open_with(&self.db, &Key::Raw(raw)).map_err(|e| {
            let _ = keyfile::remove(&self.keyfile);
            failed(e)
        })
    }

    /// Crée le coffre protégé par un mot de passe principal.
    pub fn create_with_master(&self, passphrase: &Passphrase) -> Result<Vault, KeyError> {
        self.ensure_absent()?;
        self.set_aside_orphan_keyfile()?;
        Vault::open_with(&self.db, &Key::Passphrase(passphrase.clone())).map_err(failed)
    }

    /// Fichier de clé sans coffre : renommé en `vault.key.orphan` (jamais
    /// supprimé), sinon il serait écrasé ou essayé à chaque démarrage.
    fn set_aside_orphan_keyfile(&self) -> Result<(), KeyError> {
        if !self.keyfile.exists() {
            return Ok(());
        }
        let mut orphan = self.keyfile.as_os_str().to_owned();
        orphan.push(".orphan");
        std::fs::rename(&self.keyfile, PathBuf::from(orphan)).map_err(|e| KeyError::Failed(e.to_string()))
    }

    fn ensure_absent(&self) -> Result<(), KeyError> {
        if self.db.exists() {
            return Err(KeyError::AlreadyExists);
        }
        if let Some(dir) = self.db.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| KeyError::Failed(e.to_string()))?;
        }
        Ok(())
    }

    // ── Ouverture ───────────────────────────────────────────────────────

    /// Ouverture silencieuse avec le fichier de clé. N'efface rien en cas
    /// d'échec : l'appelant demande alors le mot de passe principal.
    pub fn open_with_device_key(&self) -> Result<Vault, KeyError> {
        if !self.db.is_file() {
            return Err(KeyError::Failed("coffre introuvable".into()));
        }
        let raw = keyfile::read(&self.keyfile, &self.seal).map_err(|e| KeyError::DeviceKey(format!("{e:#}")))?;
        let key = Key::Raw(raw);
        if !Vault::check_key(&self.db, &key) {
            return Err(KeyError::DeviceKey("clé refusée".into()));
        }
        Vault::open_with(&self.db, &key).map_err(failed)
    }

    /// Ouverture avec le mot de passe principal.
    pub fn open_with_master(&self, passphrase: &Passphrase) -> Result<Vault, KeyError> {
        let key = Key::Passphrase(passphrase.clone());
        if !Vault::check_key(&self.db, &key) {
            return Err(KeyError::WrongPassphrase);
        }
        Vault::open_with(&self.db, &key).map_err(failed)
    }

    // ── Transitions (réglages) ──────────────────────────────────────────

    /// Clé d'appareil → mot de passe principal. Le fichier de clé n'est
    /// supprimé qu'une fois le coffre rechiffré ET rouvert avec `new`.
    pub fn set_master(&self, vault: &Vault, new: &Passphrase) -> Result<(), KeyError> {
        if vault.has_master_password() {
            return Err(KeyError::HasMasterPassword);
        }
        let key = Key::Passphrase(new.clone());
        vault.change_key(&key).map_err(failed)?;
        if !Vault::check_key(&self.db, &key) {
            // Retour à la clé d'appareil (toujours dans le fichier).
            if let Ok(raw) = keyfile::read(&self.keyfile, &self.seal) {
                let _ = vault.change_key(&Key::Raw(raw));
            }
            return Err(KeyError::Failed("vérification du nouveau mot de passe".into()));
        }
        keyfile::remove(&self.keyfile).map_err(|e| {
            KeyError::Failed(format!(
                "mot de passe défini, mais {} n'a pas pu être supprimé ({e:#}) : supprimez-le à la main",
                self.keyfile.display()
            ))
        })
    }

    /// Change le mot de passe principal (`old` vérifié d'abord).
    pub fn change_master(&self, vault: &Vault, old: &Passphrase, new: &Passphrase) -> Result<(), KeyError> {
        self.verify_master(vault, old)?;
        let key = Key::Passphrase(new.clone());
        vault.change_key(&key).map_err(failed)?;
        if !Vault::check_key(&self.db, &key) {
            let _ = vault.change_key(&Key::Passphrase(old.clone()));
            return Err(KeyError::Failed("vérification du nouveau mot de passe".into()));
        }
        Ok(())
    }

    /// Mot de passe principal → nouvelle clé d'appareil. Ordre : fichier de
    /// clé écrit et relu, PUIS rechiffrement, PUIS vérification ; tout échec
    /// restaure le mot de passe et retire le fichier écrit.
    pub fn remove_master(&self, vault: &Vault, current: &Passphrase) -> Result<(), KeyError> {
        self.verify_master(vault, current)?;
        let raw = RawKey::generate().map_err(failed)?;
        self.write_keyfile_checked(&raw)?;
        let key = Key::Raw(raw);
        if let Err(e) = vault.change_key(&key) {
            let _ = keyfile::remove(&self.keyfile);
            return Err(failed(e));
        }
        if !Vault::check_key(&self.db, &key) {
            let _ = vault.change_key(&Key::Passphrase(current.clone()));
            let _ = keyfile::remove(&self.keyfile);
            return Err(KeyError::Failed("vérification de la clé d'appareil".into()));
        }
        Ok(())
    }

    fn verify_master(&self, vault: &Vault, passphrase: &Passphrase) -> Result<(), KeyError> {
        if !vault.has_master_password() {
            return Err(KeyError::NoMasterPassword);
        }
        if !Vault::check_key(&self.db, &Key::Passphrase(passphrase.clone())) {
            return Err(KeyError::WrongPassphrase);
        }
        Ok(())
    }

    /// Écrit le fichier de clé puis le relit (le scellement OS doit
    /// fonctionner AVANT qu'on en dépende).
    fn write_keyfile_checked(&self, raw: &RawKey) -> Result<(), KeyError> {
        keyfile::write(&self.keyfile, raw, &self.seal).map_err(failed)?;
        match keyfile::read(&self.keyfile, &self.seal) {
            Ok(back) if back == *raw => Ok(()),
            _ => {
                let _ = keyfile::remove(&self.keyfile);
                Err(KeyError::Failed("relecture du fichier de clé".into()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyfile::OsAccountOnly;
    use anyhow::bail;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir().join(format!("nyx_keymode_{}_{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            Self(p)
        }
        fn store(&self) -> KeyStore<OsAccountOnly> {
            KeyStore::new(self.0.join("nyx").join("vault.db"), OsAccountOnly)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn pw(s: &str) -> Passphrase {
        Passphrase::new(s)
    }

    /// Scellement qui sait écrire mais jamais relire (DPAPI cassé…).
    struct BrokenUnseal;
    impl Seal for BrokenUnseal {
        fn seal(&self, plain: &[u8]) -> anyhow::Result<Vec<u8>> {
            Ok(plain.to_vec())
        }
        fn unseal(&self, _: &[u8]) -> anyhow::Result<Vec<u8>> {
            bail!("unseal")
        }
    }

    #[test]
    fn detection() {
        assert_eq!(detect(false, false), Startup::FirstLaunch);
        assert_eq!(detect(false, true), Startup::FirstLaunch);
        assert_eq!(detect(true, true), Startup::DeviceKey);
        assert_eq!(detect(true, false), Startup::MasterPassword);
    }

    #[test]
    fn passphrase_rules() {
        assert_eq!(check_new_passphrase("correct horse", "correct horse"), Ok(()));
        assert_eq!(check_new_passphrase("éèàçùâêî", "éèàçùâêî"), Ok(()), "8 caractères, pas 8 octets");
        assert!(check_new_passphrase("", "").is_err());
        assert!(check_new_passphrase("        ", "        ").is_err());
        assert!(check_new_passphrase("1234567", "1234567").is_err());
        assert!(check_new_passphrase("12345678", "12345679").is_err());
    }

    #[test]
    fn device_key_lifecycle() {
        let tmp = TempDir::new("device");
        let ks = tmp.store();
        assert_eq!(ks.startup(), Startup::FirstLaunch);
        {
            let v = ks.create_with_device_key().expect("create");
            assert!(!v.has_master_password());
            v.push_history("https://a.org", "A").unwrap();
        }
        assert_eq!(ks.startup(), Startup::DeviceKey);
        assert!(matches!(ks.create_with_device_key(), Err(KeyError::AlreadyExists)));
        let v = ks.open_with_device_key().expect("open");
        assert_eq!(v.recent_history(10).unwrap().len(), 1);
        // Une passphrase quelconque n'ouvre pas un coffre à clé d'appareil.
        assert_eq!(ks.open_with_master(&pw("whatever1")).err(), Some(KeyError::WrongPassphrase));
    }

    #[test]
    fn corrupted_keyfile_falls_back_without_deleting() {
        let tmp = TempDir::new("corrupt");
        let ks = tmp.store();
        drop(ks.create_with_device_key().expect("create"));
        // Fichier de clé d'un autre coffre / corrompu.
        keyfile::write(ks.keyfile_path(), &RawKey::generate().unwrap(), &OsAccountOnly).unwrap();
        assert!(matches!(ks.open_with_device_key(), Err(KeyError::DeviceKey(_))));
        std::fs::write(ks.keyfile_path(), b"garbage").unwrap();
        assert!(matches!(ks.open_with_device_key(), Err(KeyError::DeviceKey(_))));
        assert!(ks.keyfile_path().exists() && ks.db_path().exists(), "rien n'est supprimé");
    }

    #[test]
    fn master_lifecycle_and_transitions() {
        let tmp = TempDir::new("master");
        let ks = tmp.store();
        {
            let v = ks.create_with_master(&pw("premier-mdp")).expect("create");
            assert!(v.has_master_password());
            v.add_password(&crate::Password::new("x.org", "alex", "hunter2")).unwrap();
        }
        assert_eq!(ks.startup(), Startup::MasterPassword);
        assert_eq!(ks.open_with_master(&pw("mauvais-mdp")).err(), Some(KeyError::WrongPassphrase));
        let v = ks.open_with_master(&pw("premier-mdp")).expect("open");

        // Changer : l'ancien doit être juste.
        assert_eq!(ks.change_master(&v, &pw("faux-ancien"), &pw("second-mdp")), Err(KeyError::WrongPassphrase));
        ks.change_master(&v, &pw("premier-mdp"), &pw("second-mdp")).expect("change");
        assert!(!Vault::check_key(ks.db_path(), &Key::passphrase("premier-mdp")));
        assert!(Vault::check_key(ks.db_path(), &Key::passphrase("second-mdp")));

        // Retirer : clé d'appareil, fichier écrit, coffre rouvrable.
        assert_eq!(ks.set_master(&v, &pw("autre-mdp")), Err(KeyError::HasMasterPassword));
        assert_eq!(ks.remove_master(&v, &pw("premier-mdp")), Err(KeyError::WrongPassphrase));
        assert!(!ks.keyfile_path().exists());
        ks.remove_master(&v, &pw("second-mdp")).expect("remove");
        assert!(!v.has_master_password());
        assert_eq!(ks.startup(), Startup::DeviceKey);
        assert!(!Vault::check_key(ks.db_path(), &Key::passphrase("second-mdp")));
        // La connexion ouverte continue de fonctionner.
        assert_eq!(v.passwords_for("x.org").unwrap()[0].password, "hunter2");
        drop(v);
        let v = ks.open_with_device_key().expect("device");
        assert_eq!(v.passwords_for("x.org").unwrap()[0].password, "hunter2");

        // Redéfinir : fichier de clé supprimé seulement après succès.
        assert_eq!(ks.remove_master(&v, &pw("x")), Err(KeyError::NoMasterPassword));
        ks.set_master(&v, &pw("troisieme")).expect("set");
        assert!(v.has_master_password());
        assert!(!ks.keyfile_path().exists());
        assert_eq!(ks.startup(), Startup::MasterPassword);
        drop(v);
        let v = ks.open_with_master(&pw("troisieme")).expect("reopen");
        assert_eq!(v.passwords_for("x.org").unwrap()[0].password, "hunter2");
    }

    #[test]
    fn remove_master_aborts_if_keyfile_unreadable() {
        let tmp = TempDir::new("broken");
        let ks = tmp.store();
        let v = ks.create_with_master(&pw("mot-de-passe")).expect("create");
        let broken = KeyStore::new(ks.db_path().to_path_buf(), BrokenUnseal);
        assert!(matches!(broken.remove_master(&v, &pw("mot-de-passe")), Err(KeyError::Failed(_))));
        // État inchangé : mot de passe toujours valide, aucun fichier de clé.
        assert!(v.has_master_password());
        assert!(!ks.keyfile_path().exists());
        assert!(Vault::check_key(ks.db_path(), &Key::passphrase("mot-de-passe")));
    }

    #[test]
    fn create_with_master_sets_orphan_keyfile_aside() {
        let tmp = TempDir::new("orphan");
        let ks = tmp.store();
        keyfile::write(ks.keyfile_path(), &RawKey::generate().unwrap(), &OsAccountOnly).unwrap();
        assert_eq!(ks.startup(), Startup::FirstLaunch);
        drop(ks.create_with_master(&pw("mot-de-passe")).expect("create"));
        assert_eq!(ks.startup(), Startup::MasterPassword);
        assert!(ks.db_path().with_file_name("vault.key.orphan").exists(), "mis de côté, pas supprimé");
    }
}
