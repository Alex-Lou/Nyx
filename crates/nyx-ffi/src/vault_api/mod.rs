//! Coffre SQLCipher côté Android — helpers Rust **purs** (testés sur
//! l'hôte), appelés par `jni_exports::vault` (`io.nyx.browser.NyxVault`).
//!
//! Répartition des rôles (même comportement que Windows, `vault::keymode`) :
//! • **Kotlin** garde la clé d'appareil : 32 octets aléatoires (tirés ici,
//!   [`generate_raw_key`]) enveloppés par une clé AES-256-GCM du Keystore
//!   Android, fichier `filesDir/vault.key.enc`. Il écrit/relit/supprime ce
//!   fichier aux moments imposés par l'ordre sûr ci-dessous ;
//! • **Rust** ouvre, crée et rechiffre le coffre (`filesDir/vault.db`), et
//!   vérifie chaque changement de clé en rouvrant le fichier avec la
//!   nouvelle clé ; tout échec restaure la clé précédente.
//!
//! Ordre sûr (identique à `KeyStore`) :
//! • définir un mot de passe : rechiffrer → vérifier → (Kotlin) supprimer
//!   le fichier de clé ;
//! • retirer le mot de passe : (Kotlin) écrire et relire le fichier de clé
//!   → rechiffrer → vérifier ; échec → Kotlin supprime le fichier écrit.
//!
//! Un seul coffre par application : [`Session`] est tenue dans un `Mutex`
//! global par la couche JNI. Aucune clé n'apparaît dans un message d'erreur.

mod capture;
mod data;

use std::path::Path;

use nyx_core::nyxguard::NyxGuard;
use nyx_core::state::settings::{self, AppSettings};
use vault::keymode::{self, KeyError, Startup};
use vault::{Key, Passphrase, RawKey, Vault, RAW_KEY_LEN};

pub use capture::{capture_offer, capture_save, CaptureOffer};
pub use data::*;

/// Clé du coffre où sont persistés les réglages (format `to_query`, partagé
/// avec Windows et le desktop).
pub const SETTINGS_KEY: &str = "app_settings";

/// Met des octets à zéro de façon (raisonnablement) non éliminable.
pub fn wipe(bytes: &mut [u8]) {
    bytes.iter_mut().for_each(|b| *b = 0);
    std::hint::black_box(bytes);
}

/// Nouvelle clé d'appareil (CSPRNG de l'OS). L'appelant l'efface après usage.
pub fn generate_raw_key() -> Result<Vec<u8>, String> {
    let raw = RawKey::generate().map_err(|e| format!("{e:#}"))?;
    Ok(raw.expose().to_vec())
}

/// Clé brute depuis des octets reçus de Kotlin ; `bytes` est effacé.
fn raw_key(bytes: &mut [u8]) -> Result<Key, KeyError> {
    let key = RawKey::from_slice(bytes).map_err(|e| KeyError::DeviceKey(format!("{e:#}")));
    wipe(bytes);
    key.map(Key::Raw)
}

/// Décision de démarrage (premier lancement / clé d'appareil / mot de passe).
pub fn startup(db: &Path, keyfile_exists: bool) -> &'static str {
    match keymode::detect(db.exists(), keyfile_exists) {
        Startup::FirstLaunch => "first",
        Startup::DeviceKey => "device",
        Startup::MasterPassword => "master",
    }
}

/// Contrôle d'un nouveau mot de passe principal (`""` = valide).
pub fn check_new_passphrase(passphrase: &str, confirm: &str) -> String {
    keymode::check_new_passphrase(passphrase, confirm).err().unwrap_or_default().to_string()
}

fn failed(e: anyhow::Error) -> KeyError {
    KeyError::Failed(format!("{e:#}"))
}

fn ensure_absent(db: &Path) -> Result<(), KeyError> {
    if db.exists() {
        return Err(KeyError::AlreadyExists);
    }
    if let Some(dir) = db.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| KeyError::Failed(e.to_string()))?;
    }
    Ok(())
}

// ── Ouverture / création (hors verrou : PBKDF2 peut prendre une seconde) ──

/// Ouverture silencieuse avec la clé d'appareil (déjà désenveloppée).
pub fn open_raw(db: &Path, key: &mut [u8]) -> Result<Vault, KeyError> {
    let key = raw_key(key)?;
    if !db.is_file() {
        return Err(KeyError::Failed("coffre introuvable".into()));
    }
    if !Vault::check_key(db, &key) {
        return Err(KeyError::DeviceKey("clé refusée".into()));
    }
    Vault::open_with(db, &key).map_err(failed)
}

/// Ouverture avec le mot de passe principal.
pub fn open_passphrase(db: &Path, passphrase: &str) -> Result<Vault, KeyError> {
    let key = Key::passphrase(passphrase);
    if !db.is_file() {
        return Err(KeyError::Failed("coffre introuvable".into()));
    }
    if !Vault::check_key(db, &key) {
        return Err(KeyError::WrongPassphrase);
    }
    Vault::open_with(db, &key).map_err(failed)
}

/// Premier lancement, « Plus tard » : coffre créé avec la clé d'appareil
/// que Kotlin vient d'écrire ET de relire.
pub fn create_raw(db: &Path, key: &mut [u8]) -> Result<Vault, KeyError> {
    let key = raw_key(key)?;
    ensure_absent(db)?;
    Vault::open_with(db, &key).map_err(failed)
}

/// Premier lancement avec un mot de passe principal.
pub fn create_passphrase(db: &Path, passphrase: &str, confirm: &str) -> Result<Vault, KeyError> {
    keymode::check_new_passphrase(passphrase, confirm).map_err(|e| KeyError::Failed(e.into()))?;
    ensure_absent(db)?;
    Vault::open_with(db, &Key::passphrase(passphrase)).map_err(failed)
}

// ── Session : le coffre ouvert ──────────────────────────────────────────

/// Le coffre ouvert de l'application (au plus un).
#[derive(Default)]
pub struct Session {
    vault: Option<Vault>,
    /// Clé du coffre ouvert, en mémoire seulement (jamais écrite) : les
    /// opérations de synchronisation ouvrent une connexion dédiée avec.
    key: Option<Key>,
}

impl Session {
    pub const fn new() -> Self {
        Self { vault: None, key: None }
    }

    pub fn install(&mut self, vault: Vault) {
        self.vault = Some(vault);
        self.key = None;
    }

    /// Installe le coffre et garde sa clé pour la synchronisation.
    pub fn install_keyed(&mut self, vault: Vault, key: Key) {
        self.vault = Some(vault);
        self.key = Some(key);
    }

    pub fn close(&mut self) {
        self.vault = None;
        self.key = None;
    }

    /// Coffre ouvert (lecture/écriture rapides sous le verrou de session).
    pub fn open_vault(&self) -> Option<&Vault> {
        self.vault.as_ref()
    }

    /// Chemin + clé pour une connexion dédiée (synchronisation).
    pub fn sync_target(&self) -> Option<(std::path::PathBuf, Key)> {
        Some((self.vault.as_ref()?.path().to_path_buf(), self.key.clone()?))
    }

    pub fn is_open(&self) -> bool {
        self.vault.is_some()
    }

    pub fn has_master(&self) -> bool {
        self.vault.as_ref().is_some_and(Vault::has_master_password)
    }

    fn vault(&self) -> Result<&Vault, String> {
        self.vault.as_ref().ok_or_else(|| "coffre verrouillé".to_string())
    }

    /// Vérifie `passphrase` contre le coffre ouvert (mot de passe principal actif).
    pub fn verify_master(&self, passphrase: &str) -> Result<(), KeyError> {
        let vault = self.vault().map_err(KeyError::Failed)?;
        if !vault.has_master_password() {
            return Err(KeyError::NoMasterPassword);
        }
        if !Vault::check_key(vault.path(), &Key::passphrase(passphrase)) {
            return Err(KeyError::WrongPassphrase);
        }
        Ok(())
    }

    /// Clé d'appareil → mot de passe principal. `current` : clé d'appareil
    /// actuelle (restaurée si la vérification échoue). En cas de succès,
    /// Kotlin supprime ENSUITE `vault.key.enc`.
    pub fn set_master(&mut self, new: &str, confirm: &str, current: &mut [u8]) -> Result<(), KeyError> {
        let current = raw_key(current)?;
        keymode::check_new_passphrase(new, confirm).map_err(|e| KeyError::Failed(e.into()))?;
        let vault = self.vault().map_err(KeyError::Failed)?;
        if vault.has_master_password() {
            return Err(KeyError::HasMasterPassword);
        }
        let key = Key::passphrase(new);
        vault.change_key(&key).map_err(failed)?;
        if !Vault::check_key(vault.path(), &key) {
            let _ = vault.change_key(&current);
            return Err(KeyError::Failed("vérification du nouveau mot de passe".into()));
        }
        self.key = Some(key);
        Ok(())
    }

    /// Change le mot de passe principal (`old` vérifié d'abord).
    pub fn change_master(&mut self, old: &str, new: &str, confirm: &str) -> Result<(), KeyError> {
        keymode::check_new_passphrase(new, confirm).map_err(|e| KeyError::Failed(e.into()))?;
        self.verify_master(old)?;
        let vault = self.vault().map_err(KeyError::Failed)?;
        let key = Key::passphrase(new);
        vault.change_key(&key).map_err(failed)?;
        if !Vault::check_key(vault.path(), &key) {
            let _ = vault.change_key(&Key::passphrase(old));
            return Err(KeyError::Failed("vérification du nouveau mot de passe".into()));
        }
        self.key = Some(key);
        Ok(())
    }

    /// Mot de passe principal → clé d'appareil `new_raw`, que Kotlin a DÉJÀ
    /// écrite et relue dans `vault.key.enc`. Échec → mot de passe restauré ;
    /// Kotlin supprime alors le fichier écrit.
    pub fn remove_master(&mut self, current: &str, new_raw: &mut [u8]) -> Result<(), KeyError> {
        let key = raw_key(new_raw)?;
        self.verify_master(current)?;
        let vault = self.vault().map_err(KeyError::Failed)?;
        vault.change_key(&key).map_err(failed)?;
        if !Vault::check_key(vault.path(), &key) {
            let _ = vault.change_key(&Key::Passphrase(Passphrase::new(current)));
            return Err(KeyError::Failed("vérification de la clé d'appareil".into()));
        }
        self.key = Some(key);
        Ok(())
    }

    // ── Réglages de l'application ───────────────────────────────────────

    /// Réglages persistés (défauts si absents ou coffre fermé).
    pub fn app_settings(&self) -> AppSettings {
        let shared = settings::new();
        if let Some(q) = self.vault.as_ref().and_then(|v| v.setting(SETTINGS_KEY).ok().flatten()) {
            // Bloqueur jetable : seul l'état des réglages nous intéresse ici.
            settings::apply_from_url(&format!("nyx://apply?{q}"), &shared, &NyxGuard::new());
        }
        let s = shared.borrow().clone();
        s
    }

    /// Persiste les réglages (format `to_query`).
    pub fn save_app_settings(&self, s: &AppSettings) -> Result<(), String> {
        self.vault()?.set_setting(SETTINGS_KEY, &settings::to_query(s)).map_err(|e| format!("{e:#}"))
    }

    /// Réglages affichés par l'écran Réglages d'Android (JSON).
    pub fn settings_json(&self) -> String {
        let s = self.app_settings();
        serde_json::json!({
            "engine": s.search_engine.id(),
            "home": s.home_url,
            "adblock": s.adblock_enabled,
            "savepw": s.save_passwords,
            "master": self.has_master(),
        })
        .to_string()
    }

    /// Modifie les réglages exposés sur Android, persiste et renvoie l'état
    /// complet (à appliquer au bloqueur et à la barre d'adresse). Page
    /// d'accueil vide = Nouvel onglet ; saisie nue → https ; jamais de
    /// `javascript:`/`file:`.
    pub fn update_app_settings(
        &self,
        engine: &str,
        home: &str,
        adblock: bool,
        save_passwords: bool,
    ) -> Result<AppSettings, String> {
        let mut s = self.app_settings();
        s.search_engine = settings::SearchEngine::from_id(engine);
        let home = home.trim();
        s.home_url = if home.is_empty() {
            "nyx://newtab".into()
        } else if home.contains("://") {
            home.to_string()
        } else {
            format!("https://{home}")
        };
        if !nyx_core::url::is_navigable(&s.home_url) {
            return Err("Page d'accueil refusée".into());
        }
        s.adblock_enabled = adblock;
        s.save_passwords = save_passwords;
        self.save_app_settings(&s)?;
        Ok(s)
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>, String> {
        self.vault()?.setting(key).map_err(|e| format!("{e:#}"))
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        self.vault()?.set_setting(key, value).map_err(|e| format!("{e:#}"))
    }
}

/// Longueur attendue d'une clé d'appareil (contrôle côté Kotlin).
pub const DEVICE_KEY_LEN: usize = RAW_KEY_LEN;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Dossier temporaire unique, nettoyé par le guard.
    pub struct TempDir(pub PathBuf);

    impl TempDir {
        pub fn new(name: &str) -> Self {
            let p = std::env::temp_dir().join(format!("nyx_ffi_vault_{}_{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).expect("tmp");
            Self(p)
        }
        pub fn db(&self) -> PathBuf {
            self.0.join("vault.db")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Session ouverte sur un coffre neuf à clé d'appareil.
    pub fn open_session(tmp: &TempDir) -> Session {
        let mut key = generate_raw_key().expect("rng");
        let mut s = Session::new();
        s.install(create_raw(&tmp.db(), &mut key).expect("create"));
        s
    }

    #[test]
    fn startup_states() {
        let tmp = TempDir::new("startup");
        assert_eq!(startup(&tmp.db(), false), "first");
        assert_eq!(startup(&tmp.db(), true), "first");
        let _s = open_session(&tmp);
        assert_eq!(startup(&tmp.db(), true), "device");
        assert_eq!(startup(&tmp.db(), false), "master");
    }

    #[test]
    fn raw_keys_are_wiped_and_checked() {
        let tmp = TempDir::new("raw");
        let mut key = generate_raw_key().expect("rng");
        assert_eq!(key.len(), DEVICE_KEY_LEN);
        let copy = key.clone();
        drop(create_raw(&tmp.db(), &mut key).expect("create"));
        assert!(key.iter().all(|b| *b == 0), "copie Rust effacée");

        assert!(matches!(create_raw(&tmp.db(), &mut copy.clone()), Err(KeyError::AlreadyExists)));
        assert!(open_raw(&tmp.db(), &mut copy.clone()).is_ok());
        let mut other = generate_raw_key().expect("rng");
        assert!(matches!(open_raw(&tmp.db(), &mut other), Err(KeyError::DeviceKey(_))));
        assert!(matches!(open_raw(&tmp.db(), &mut [1u8; 31]), Err(KeyError::DeviceKey(_))));
        assert!(matches!(open_passphrase(&tmp.db(), "x"), Err(KeyError::WrongPassphrase)));
        let missing = tmp.0.join("none.db");
        assert!(open_raw(&missing, &mut copy.clone()).is_err());
        assert!(!missing.exists(), "jamais créé à l'ouverture");
    }

    #[test]
    fn passphrase_creation_rules() {
        let tmp = TempDir::new("pass");
        assert_eq!(check_new_passphrase("court", "court"), "Le mot de passe doit faire au moins 8 caractères");
        assert_eq!(check_new_passphrase("long-enough", "long-enough"), "");
        assert!(create_passphrase(&tmp.db(), "abc", "abc").is_err());
        assert!(!tmp.db().exists());
        drop(create_passphrase(&tmp.db(), "mot-de-passe", "mot-de-passe").expect("create"));
        let mut s = Session::new();
        s.install(open_passphrase(&tmp.db(), "mot-de-passe").expect("open"));
        assert!(s.has_master());
    }

    #[test]
    fn master_password_round_trip() {
        let tmp = TempDir::new("master");
        let mut device = generate_raw_key().expect("rng");
        let mut s = Session::new();
        s.install(create_raw(&tmp.db(), &mut device.clone()).expect("create"));
        s.set_setting("k", "v").expect("set");
        assert!(!s.has_master());

        // Confirmation fausse : rien ne change.
        assert!(s.set_master("nouveau-mdp", "autre-mdp", &mut device.clone()).is_err());
        assert!(!s.has_master());
        // Définir.
        assert!(s.sync_target().is_none(), "clé inconnue : pas de synchronisation");
        s.set_master("nouveau-mdp", "nouveau-mdp", &mut device.clone()).expect("set master");
        assert!(s.has_master());
        assert_eq!(s.sync_target().map(|t| t.1), Some(Key::passphrase("nouveau-mdp")), "clé suivie");
        assert!(matches!(
            s.set_master("x-x-x-x-x", "x-x-x-x-x", &mut device.clone()),
            Err(KeyError::HasMasterPassword)
        ));
        s.close();
        assert!(open_raw(&tmp.db(), &mut device.clone()).is_err(), "clé d'appareil révoquée");
        s.install(open_passphrase(&tmp.db(), "nouveau-mdp").expect("open"));
        assert_eq!(s.setting("k").expect("get").as_deref(), Some("v"));

        // Changer.
        assert!(matches!(s.change_master("faux-faux", "encore-un", "encore-un"), Err(KeyError::WrongPassphrase)));
        s.change_master("nouveau-mdp", "encore-un", "encore-un").expect("change");
        assert!(matches!(open_passphrase(&tmp.db(), "nouveau-mdp"), Err(KeyError::WrongPassphrase)));

        // Retirer → nouvelle clé d'appareil.
        let mut fresh = generate_raw_key().expect("rng");
        assert!(s.remove_master("faux-faux", &mut fresh.clone()).is_err());
        assert!(s.has_master());
        s.remove_master("encore-un", &mut fresh.clone()).expect("remove");
        assert!(!s.has_master());
        let (path, key) = s.sync_target().expect("cible");
        assert!(Vault::open_with(&path, &key).is_ok(), "connexion dédiée possible avec la clé suivie");
        s.close();
        assert!(s.sync_target().is_none(), "clé oubliée à la fermeture");
        s.install(open_raw(&tmp.db(), &mut fresh).expect("open device"));
        assert_eq!(s.setting("k").expect("get").as_deref(), Some("v"));
        wipe(&mut device);
    }

    #[test]
    fn locked_session_refuses() {
        let s = Session::new();
        assert!(!s.is_open() && !s.has_master());
        assert!(s.setting("k").is_err());
        assert!(s.verify_master("x").is_err());
        assert_eq!(s.app_settings().search_engine.id(), "ddg");
    }

    #[test]
    fn app_settings_persist() {
        let tmp = TempDir::new("settings");
        let s = open_session(&tmp);
        let mut a = s.app_settings();
        assert!(a.adblock_enabled && a.save_passwords);
        a.search_engine = settings::SearchEngine::Brave;
        a.adblock_enabled = false;
        a.save_passwords = false;
        a.home_url = "https://example.org".into();
        s.save_app_settings(&a).expect("save");
        let back = s.app_settings();
        assert_eq!(back.search_engine.id(), "brave");
        assert!(!back.adblock_enabled && !back.save_passwords);
        assert_eq!(back.home_url, "https://example.org");

        let s2 = s.update_app_settings("ecosia", "exemple.fr", true, true).expect("update");
        assert_eq!(s2.home_url, "https://exemple.fr");
        assert!(s.update_app_settings("ddg", "javascript://%0Aalert(1)", true, true).is_err());
        assert!(s.update_app_settings("ddg", "file:///etc/passwd", true, true).is_err());
        assert_eq!(s.app_settings().home_url, "https://exemple.fr", "refus = rien persisté");
        s.update_app_settings("inconnu", "  ", false, true).expect("update");
        let v: serde_json::Value = serde_json::from_str(&s.settings_json()).expect("json");
        assert_eq!(v["engine"], "ddg");
        assert_eq!(v["home"], "nyx://newtab");
        assert_eq!(v["adblock"], false);
        assert_eq!(v["savepw"], true);
        assert_eq!(v["master"], false);
    }
}
