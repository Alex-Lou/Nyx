//! Appareils liés côté Android — helpers Rust **purs** (testés sur l'hôte),
//! appelés par `jni_exports::sync` (`io.nyx.browser.NyxSync`). Toute la
//! logique de synchronisation vit dans `nyx-sync` ; ici on ne fait
//! qu'orchestrer pour Kotlin :
//!
//! * **appairage en deux temps** ([`Pairing`]) : `pairBegin` lance
//!   `pairing::accept` sur un thread Rust et rend la main dès que le code à
//!   6 chiffres est connu (ou en cas d'échec) ; l'utilisateur compare ;
//!   `pairConfirm(oui/non)` débloque le thread (canal) et attend le
//!   résultat. Kotlin appelle les deux depuis un fil d'arrière-plan ;
//! * **synchronisation** ([`sync_now`]) : découverte mDNS + session avec
//!   chaque appareil trouvé, sur une connexion au coffre dédiée (le
//!   `Vault` de la session n'est pas `Sync` et ne doit pas rester verrouillé
//!   pendant le réseau), ouverte avec la clé gardée en mémoire ;
//! * liste / renommage / révocation sur le coffre de la session.
//!
//! [`SYNC_LOCK`] sérialise les opérations qui ouvrent une seconde
//! connexion : un changement de mot de passe principal (rechiffrement)
//! attend qu'aucune ne soit en cours.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

use nyx_sync::{pairing, Profile, SyncEngine};
use serde::{Deserialize, Serialize};
use serde_json::json;
use vault::{Key, Vault};

/// Délai d'E/S pendant l'appairage (l'humain compare les codes).
pub const PAIR_IO_TIMEOUT: Duration = Duration::from_secs(120);
/// Attente de la réponse de l'utilisateur (< délai d'E/S du PC).
pub const CONFIRM_TIMEOUT: Duration = Duration::from_secs(110);
/// Recherche mDNS d'une synchronisation.
pub const BROWSE: Duration = Duration::from_secs(3);
/// Dernière synchronisation (réglage local, même clé que Windows).
pub const LAST_SYNC_KEY: &str = "local.sync.last";

/// Une seule opération à connexion dédiée à la fois (appairage, synchro,
/// rechiffrement).
pub static SYNC_LOCK: Mutex<()> = Mutex::new(());

pub const BUSY: &str = "Synchronisation en cours, réessayez dans un instant";

/// Verrou de synchronisation, sans attendre (`None` = occupé).
pub fn try_sync_lock() -> Option<MutexGuard<'static, ()>> {
    match SYNC_LOCK.try_lock() {
        Ok(g) => Some(g),
        Err(TryLockError::Poisoned(p)) => Some(p.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

fn now_ms() -> u64 {
    nyx_sync::hlc::now_millis()
}

// ── Appairage en deux temps ─────────────────────────────────────────────

/// État d'un appairage, tel que renvoyé à Kotlin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairState {
    /// Connexion au PC en cours.
    Connecting,
    /// Canal établi : afficher `code`, attendre la réponse.
    Confirm { code: String, pc: String },
    /// Réponse transmise, fin du protocole en cours.
    Finishing,
    /// Terminé : nom du PC rejoint, ou message d'erreur.
    Done(Result<String, String>),
}

impl PairState {
    /// JSON pour Kotlin : `{"state":"confirm","code":…,"pc":…}`,
    /// `{"state":"done","pc":…}`, `{"state":"error","message":…}`, ou
    /// `{"state":"pending"}` (encore en cours à l'expiration de l'attente).
    pub fn to_json(&self) -> String {
        match self {
            Self::Connecting | Self::Finishing => json!({ "state": "pending" }),
            Self::Confirm { code, pc } => json!({ "state": "confirm", "code": code, "pc": pc }),
            Self::Done(Ok(pc)) => json!({ "state": "done", "pc": pc }),
            Self::Done(Err(message)) => json!({ "state": "error", "message": message }),
        }
        .to_string()
    }
}

/// Appairage en cours : état partagé avec le thread qui exécute le
/// protocole, réponse de l'utilisateur par canal.
pub struct Pairing {
    state: Mutex<PairState>,
    changed: Condvar,
    answer: Mutex<Option<Sender<bool>>>,
}

impl Pairing {
    /// Lance `run` sur un thread. `run` reçoit la fonction de confirmation
    /// à passer au protocole : elle publie le code, puis attend
    /// [`Pairing::answer`] (au plus `confirm_timeout`, refus sinon).
    pub fn start<R>(run: R, confirm_timeout: Duration) -> Arc<Self>
    where
        R: FnOnce(&dyn Fn(&str, &str) -> bool) -> Result<String, String> + Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<bool>();
        let me = Arc::new(Self { state: Mutex::new(PairState::Connecting), changed: Condvar::new(), answer: Mutex::new(Some(tx)) });
        let worker = me.clone();
        let spawned = std::thread::Builder::new().name("nyx-pair".into()).spawn(move || {
            let confirm = |code: &str, pc: &str| worker.await_answer(&rx, code, pc, confirm_timeout);
            let result = run(&confirm);
            worker.set(PairState::Done(result));
        });
        if spawned.is_err() {
            me.set(PairState::Done(Err("Impossible de démarrer l'appairage".into())));
        }
        me
    }

    fn lock(&self) -> MutexGuard<'_, PairState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn set(&self, s: PairState) {
        *self.lock() = s;
        self.changed.notify_all();
    }

    pub fn state(&self) -> PairState {
        self.lock().clone()
    }

    fn await_answer(&self, rx: &Receiver<bool>, code: &str, pc: &str, timeout: Duration) -> bool {
        self.set(PairState::Confirm { code: code.to_string(), pc: pc.to_string() });
        // Délai dépassé ou appairage abandonné : refus.
        let yes = rx.recv_timeout(timeout).unwrap_or_default();
        self.set(PairState::Finishing);
        yes
    }

    /// Attend (au plus `timeout`) que l'état satisfasse `done`.
    fn wait_until(&self, timeout: Duration, done: impl Fn(&PairState) -> bool) -> PairState {
        let deadline = Instant::now() + timeout;
        let mut g = self.lock();
        while !done(&g) {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else { break };
            g = self.changed.wait_timeout(g, left).map(|(g, _)| g).unwrap_or_else(|p| p.into_inner().0);
        }
        g.clone()
    }

    /// Étape 1 : attend le code (ou l'échec).
    pub fn wait_code(&self, timeout: Duration) -> PairState {
        self.wait_until(timeout, |s| matches!(s, PairState::Confirm { .. } | PairState::Done(_)))
    }

    /// Étape 2 : transmet la réponse puis attend la fin.
    pub fn answer(&self, yes: bool, timeout: Duration) -> PairState {
        if let Some(tx) = self.answer.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = tx.send(yes);
        }
        self.wait_until(timeout, |s| matches!(s, PairState::Done(_)))
    }

    /// Abandon (écran quitté) : refus si le code est affiché ; sinon le
    /// protocole échouera de lui-même (délais).
    pub fn cancel(&self) {
        if let Some(tx) = self.answer.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = tx.send(false);
        }
    }
}

/// Exécute `pairing::accept` sur une connexion dédiée au coffre.
pub fn accept_offer(db: &Path, key: &Key, offer: &str, name: &str, confirm: &dyn Fn(&str, &str) -> bool) -> Result<String, String> {
    let _busy = SYNC_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let vault = Vault::open_with(db, key).map_err(|e| format!("Coffre inaccessible : {e:#}"))?;
    let mut engine = SyncEngine::open(&vault).map_err(|e| nyx_sync::user_message(&e))?;
    engine.set_io_timeout(PAIR_IO_TIMEOUT);
    let name = if name.trim().is_empty() { "Téléphone" } else { name };
    pairing::accept(offer.trim(), &engine, name, |code, pc| confirm(code, &pc.name))
        .map(|pc| pc.name)
        .map_err(|e| nyx_sync::user_message(&e))
}

// ── Synchronisation ─────────────────────────────────────────────────────

/// Dernière synchronisation (persistée, affichée).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastSync {
    pub at_ms: u64,
    pub ok: bool,
    pub message: String,
    #[serde(default)]
    pub changed: bool,
}

impl LastSync {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// Autres appareils actifs du profil (lecture seule).
fn peers(vault: &Vault) -> usize {
    match Profile::load(vault) {
        Ok(Some(p)) => p.active().filter(|d| d.device_id != vault.device_id()).count(),
        _ => 0,
    }
}

/// « Synchroniser maintenant » : connexion dédiée, découverte, session avec
/// chaque appareil trouvé. Sans appareil lié : rien n'est créé ni envoyé.
/// `browse` = durée de la recherche mDNS.
pub fn sync_now(db: &Path, key: &Key, browse: Duration) -> LastSync {
    let Some(_busy) = try_sync_lock() else {
        return LastSync { at_ms: now_ms(), ok: false, message: BUSY.into(), changed: false };
    };
    let vault = match Vault::open_with(db, key) {
        Ok(v) => v,
        Err(e) => return LastSync { at_ms: now_ms(), ok: false, message: format!("Coffre inaccessible : {e:#}"), changed: false },
    };
    if peers(&vault) == 0 {
        return LastSync { at_ms: now_ms(), ok: false, message: "Aucun appareil lié".into(), changed: false };
    }
    let result = SyncEngine::open(&vault).and_then(|e| nyx_sync::discovery::sync_all(&e, browse));
    let last = match result {
        Ok(s) => LastSync { at_ms: now_ms(), ok: s.ok(), message: s.message(), changed: s.changed() },
        Err(e) => LastSync {
            at_ms: now_ms(),
            ok: false,
            message: format!("Découverte réseau impossible : {}", nyx_sync::user_message(&e)),
            changed: false,
        },
    };
    let _ = vault.set_setting(LAST_SYNC_KEY, &last.to_json());
    last
}

// ── Liste, nom, révocation (coffre de la session) ───────────────────────

/// `[{id, name, addedAt, revoked, me}]` ; sans profil : cet appareil seul
/// (nom vide = « Ce téléphone » côté Kotlin). Lecture seule.
pub fn devices_json(vault: &Vault) -> String {
    let me = vault.device_id();
    let devices = match Profile::load(vault) {
        Ok(Some(p)) => p.devices,
        _ => Vec::new(),
    };
    let mut rows: Vec<serde_json::Value> = devices
        .iter()
        .map(|d| {
            json!({ "id": d.device_id, "name": d.name, "addedAt": d.added_at, "revoked": d.revoked, "me": d.device_id == me })
        })
        .collect();
    if !devices.iter().any(|d| d.device_id == me) {
        rows.insert(0, json!({ "id": me, "name": "", "addedAt": 0, "revoked": false, "me": true }));
    }
    serde_json::Value::Array(rows).to_string()
}

pub fn set_device_name(vault: &Vault, name: &str) -> Result<(), String> {
    let Some(_busy) = try_sync_lock() else { return Err(BUSY.into()) };
    if name.trim().is_empty() {
        return Err("Nom vide".into());
    }
    let engine = SyncEngine::open(vault).map_err(|e| nyx_sync::user_message(&e))?;
    engine.set_device_name(name).map_err(|e| nyx_sync::user_message(&e))
}

pub fn revoke(vault: &Vault, device_id: &str) -> Result<(), String> {
    let Some(_busy) = try_sync_lock() else { return Err(BUSY.into()) };
    let engine = SyncEngine::open(vault).map_err(|e| nyx_sync::user_message(&e))?;
    engine.revoke(device_id).map_err(|e| nyx_sync::user_message(&e))
}

/// Dernière synchronisation persistée (`""` si aucune).
pub fn last_sync_json(vault: &Vault) -> String {
    vault.setting(LAST_SYNC_KEY).ok().flatten().unwrap_or_default()
}

/// Appairage en cours (au plus un).
pub static PAIRING: Mutex<Option<Arc<Pairing>>> = Mutex::new(None);

pub fn current_pairing() -> Option<Arc<Pairing>> {
    PAIRING.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// Démarre un appairage avec le coffre `db` (clé `key`, copie en mémoire
/// du thread). Remplace un appairage précédent (annulé).
pub fn begin_pairing(db: PathBuf, key: Key, offer: String, name: String) -> Arc<Pairing> {
    let mut slot = PAIRING.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(old) = slot.take() {
        old.cancel();
    }
    let p = Pairing::start(move |confirm| accept_offer(&db, &key, &offer, &name, confirm), CONFIRM_TIMEOUT);
    *slot = Some(p.clone());
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, TcpListener};

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("nyx_ffi_sync_{}_{name}.db", std::process::id()));
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", p.display()));
        }
        p
    }

    #[test]
    fn machine_a_etats_confirmee() {
        // Protocole simulé : publie le code, rend le nom du PC si accepté.
        let p = Pairing::start(
            |confirm| if confirm("123456", "PC") { Ok("PC".into()) } else { Err("Appairage annulé".into()) },
            Duration::from_secs(5),
        );
        let s = p.wait_code(Duration::from_secs(5));
        assert_eq!(s, PairState::Confirm { code: "123456".into(), pc: "PC".into() });
        assert_eq!(s.to_json(), r#"{"code":"123456","pc":"PC","state":"confirm"}"#);
        let done = p.answer(true, Duration::from_secs(5));
        assert_eq!(done, PairState::Done(Ok("PC".into())));
        assert_eq!(done.to_json(), r#"{"pc":"PC","state":"done"}"#);
        // Seconde réponse : sans effet.
        assert_eq!(p.answer(false, Duration::from_millis(10)), PairState::Done(Ok("PC".into())));
    }

    #[test]
    fn machine_a_etats_refus_annulation_delai_echec() {
        let run = |confirm: &dyn Fn(&str, &str) -> bool| {
            if confirm("000111", "PC") {
                Ok("PC".to_string())
            } else {
                Err("Appairage annulé".to_string())
            }
        };
        let p = Pairing::start(run, Duration::from_secs(5));
        p.wait_code(Duration::from_secs(5));
        assert_eq!(p.answer(false, Duration::from_secs(5)), PairState::Done(Err("Appairage annulé".into())));

        let p = Pairing::start(run, Duration::from_secs(5));
        p.wait_code(Duration::from_secs(5));
        p.cancel();
        let s = p.wait_until(Duration::from_secs(5), |s| matches!(s, PairState::Done(_)));
        assert_eq!(s, PairState::Done(Err("Appairage annulé".into())));

        // Personne ne répond : refus après le délai.
        let p = Pairing::start(run, Duration::from_millis(50));
        let s = p.answer_after_timeout();
        assert_eq!(s, PairState::Done(Err("Appairage annulé".into())));

        // Échec avant le code (offre illisible…) : l'étape 1 rend l'erreur.
        let p = Pairing::start(|_| Err("Code illisible".into()), Duration::from_secs(5));
        let s = p.wait_code(Duration::from_secs(5));
        assert_eq!(s.to_json(), r#"{"message":"Code illisible","state":"error"}"#);

        // Attente trop courte : « pending ».
        let p = Pairing::start(
            |_| {
                std::thread::sleep(Duration::from_millis(300));
                Err("x".into())
            },
            Duration::from_secs(5),
        );
        assert_eq!(p.wait_code(Duration::from_millis(10)).to_json(), r#"{"state":"pending"}"#);
    }

    impl Pairing {
        /// Test : attend la fin sans répondre.
        fn answer_after_timeout(&self) -> PairState {
            self.wait_until(Duration::from_secs(5), |s| matches!(s, PairState::Done(_)))
        }
    }

    #[test]
    fn appairage_reel_avec_un_pc_puis_liste_et_synchro() {
        let (pc_db, phone_db) = (tmp("pc"), tmp("phone"));
        let lo = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let listener = TcpListener::bind((lo, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (offer_tx, offer_rx) = mpsc::channel();
        let pc_path = pc_db.clone();
        let host = std::thread::spawn(move || {
            let v = Vault::open(&pc_path, "pc").unwrap();
            let e = SyncEngine::open(&v).unwrap();
            e.set_device_name("Bureau").unwrap();
            let mut offer = pairing::Offer::new(&e, vec![lo], port).unwrap();
            offer_tx.send(offer.encode()).unwrap();
            pairing::host(&e, &listener, &mut offer, |_, _| true).map(|d| d.name)
        });
        let offer = offer_rx.recv().unwrap();

        // Le téléphone : coffre de session + appairage sur connexion dédiée.
        let phone = Vault::open(&phone_db, "tel").unwrap();
        let before: serde_json::Value = serde_json::from_str(&devices_json(&phone)).unwrap();
        assert_eq!(before.as_array().unwrap().len(), 1);
        assert_eq!(before[0]["me"], true);
        assert!(Profile::load(&phone).unwrap().is_none(), "lecture seule : rien créé");
        // Code illisible : l'étape 1 rend un message clair.
        let bad = begin_pairing(phone_db.clone(), Key::passphrase("tel"), "nyxpair:v1:%%%".into(), "x".into());
        let s = bad.wait_code(Duration::from_secs(5));
        assert!(matches!(&s, PairState::Done(Err(m)) if m.contains("illisible")), "{s:?}");
        assert_eq!(sync_now(&phone_db, &Key::passphrase("tel"), Duration::from_millis(10)).message, "Aucun appareil lié");

        let p = begin_pairing(phone_db.clone(), Key::passphrase("tel"), format!("  {offer}\n"), "Pixel".into());
        let s = p.wait_code(Duration::from_secs(20));
        let PairState::Confirm { code, pc } = s else { panic!("{s:?}") };
        assert_eq!((code.len(), pc.as_str()), (6, "Bureau"));
        assert!(try_sync_lock().is_none(), "rechiffrement bloqué pendant l'appairage");
        assert_eq!(p.answer(true, Duration::from_secs(20)), PairState::Done(Ok("Bureau".into())));
        assert_eq!(host.join().unwrap().unwrap(), "Pixel");

        let after: serde_json::Value = serde_json::from_str(&devices_json(&phone)).unwrap();
        let names: Vec<&str> = after.as_array().unwrap().iter().map(|d| d["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"Bureau") && names.contains(&"Pixel"), "{names:?}");
        set_device_name(&phone, "Mon Pixel").unwrap();
        assert!(devices_json(&phone).contains("Mon Pixel"));
        assert!(set_device_name(&phone, "  ").is_err());
        let pc_id = after.as_array().unwrap().iter().find(|d| d["me"] == false).unwrap()["id"].as_str().unwrap().to_string();
        assert!(last_sync_json(&phone).is_empty());
        revoke(&phone, &pc_id).unwrap();
        let v: serde_json::Value = serde_json::from_str(&devices_json(&phone)).unwrap();
        assert!(v.as_array().unwrap().iter().any(|d| d["id"] == pc_id.as_str() && d["revoked"] == true));
        assert!(revoke(&phone, "inconnu").is_err());
        let me = v.as_array().unwrap().iter().find(|d| d["me"] == true).unwrap()["id"].as_str().unwrap().to_string();
        assert!(revoke(&phone, &me).is_err(), "jamais soi-même");
        drop(phone);
        for db in [pc_db, phone_db] {
            for ext in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{ext}", db.display()));
            }
        }
    }
}
