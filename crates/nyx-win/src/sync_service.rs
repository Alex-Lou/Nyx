//! Service de synchronisation « Appareils liés » — un thread dédié, sans
//! dépendance Windows (compilé et testé partout, avec de vrais coffres).
//!
//! `vault::Vault` n'est pas `Sync` : le thread ouvre SA connexion au coffre,
//! avec la même clé que l'UI (clé d'appareil ou mot de passe principal,
//! gardés en mémoire seulement, jamais écrits). Il possède :
//! * l'écoute TCP sur le premier port libre de `nyx_sync::SERVICE_PORTS`
//!   (47420…47429) et l'annonce mDNS ;
//! * la boucle `serve_stream` (connexions entrantes des autres appareils) ;
//! * une synchronisation sortante (`discovery::sync_all`) au démarrage,
//!   toutes les 5 minutes et à la demande ;
//! * l'appairage côté PC (`pairing::host_until`) : l'offre (QR), puis le
//!   code à 6 chiffres, sont envoyés à l'UI par [`Event`] ; la réponse de
//!   l'utilisateur revient par [`Cmd::PairAnswer`]. Le thread UI n'attend
//!   jamais.
//!
//! Arrêt : drapeau vérifié toutes les 200 ms (et pendant l'attente du
//! téléphone) ; [`Service::stop`] attend la fin au plus le délai donné puis
//! détache le thread (une session en cours est bornée par ses délais d'E/S).

use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use nyx_sync::discovery::{self, Advertiser};
use nyx_sync::{hlc, pairing, SyncEngine, SyncSummary};
use serde::{Deserialize, Serialize};
use vault::{Key, Vault};

use crate::qr;

/// Synchronisation sortante périodique.
pub const PERIOD: Duration = Duration::from_secs(5 * 60);
/// Durée de la recherche mDNS d'une synchronisation sortante.
pub const BROWSE: Duration = Duration::from_secs(3);
/// Délai d'E/S pendant l'appairage : l'humain compare les codes.
pub const PAIR_IO_TIMEOUT: Duration = Duration::from_secs(120);
/// Attente de la réponse de l'utilisateur (< délai d'E/S du téléphone).
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(110);
const TICK: Duration = Duration::from_millis(200);
/// Dernière synchronisation (réglage local : ne quitte jamais l'appareil).
pub const LAST_SYNC_KEY: &str = "local.sync.last";

/// Paramètres du service. `key` : mémoire seulement.
pub struct Config {
    pub db: PathBuf,
    pub key: Key,
    pub bind: IpAddr,
    pub ports: Vec<u16>,
    /// Adresses publiées dans le QR (`None` : `pairing::lan_addrs`).
    pub offer_addrs: Option<Vec<IpAddr>>,
    /// Annonce + découverte mDNS (coupé dans les tests).
    pub mdns: bool,
    pub period: Duration,
    pub browse: Duration,
}

impl Config {
    pub fn new(db: PathBuf, key: Key) -> Self {
        Self {
            db,
            key,
            bind: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            ports: nyx_sync::SERVICE_PORTS.collect(),
            offer_addrs: None,
            mdns: true,
            period: PERIOD,
            browse: BROWSE,
        }
    }
}

/// Commande de l'UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    SyncNow,
    /// Afficher une offre d'appairage (QR) et attendre le téléphone.
    Pair,
    /// Réponse à « Les codes correspondent ? ».
    PairAnswer(bool),
    PairCancel,
    Rename(String),
    Revoke(String),
}

/// Dernière synchronisation (persistée dans `local.sync.last`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastSync {
    pub at_ms: u64,
    pub ok: bool,
    pub message: String,
}

/// Un appareil du profil, tel qu'affiché.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRow {
    pub id: String,
    pub name: String,
    pub added_at_ms: u64,
    pub revoked: bool,
    pub this_device: bool,
}

/// Ce que le service signale à l'UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Écoute réseau (port retenu, ou pourquoi aucun).
    Listening { port: Option<u16>, error: Option<String> },
    Devices(Vec<DeviceRow>),
    /// Dernière synchronisation connue au démarrage.
    LastSync(LastSync),
    /// Offre à afficher : QR (SVG), chaîne à saisir, expiration (ms epoch).
    PairOffer { offer: String, qr_svg: String, expires_at_ms: u64 },
    /// Un téléphone est connecté : code à comparer + son nom.
    PairCode { code: String, device: String },
    /// Fin de l'appairage : nom de l'appareil ajouté, ou message d'erreur.
    PairDone(Result<String, String>),
    /// Synchronisation terminée ; `changed` = données reçues → relire l'état.
    Synced { last: LastSync, changed: bool },
    /// Message ponctuel (renommage, révocation…).
    Notice(String),
    /// Le service n'a pas pu démarrer (coffre).
    Failed(String),
}

/// Poignée du thread de synchronisation (côté UI).
pub struct Service {
    tx: Sender<Cmd>,
    stop: Arc<AtomicBool>,
    pair_cancel: Arc<AtomicBool>,
    done: Receiver<()>,
    thread: Option<JoinHandle<()>>,
}

impl Service {
    /// Démarre le thread. `emit` est appelé depuis ce thread : il ne doit
    /// que poster l'événement à l'UI (proxy de la boucle d'événements).
    pub fn start<E>(cfg: Config, emit: E) -> std::io::Result<Self>
    where
        E: Fn(Event) + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        let (done_tx, done) = mpsc::channel::<()>();
        let stop = Arc::new(AtomicBool::new(false));
        let pair_cancel = Arc::new(AtomicBool::new(false));
        let worker = Worker { cfg, rx, stop: stop.clone(), pair_cancel: pair_cancel.clone(), emit };
        let thread = std::thread::Builder::new().name("nyx-sync".into()).spawn(move || {
            let _done = done_tx; // relâché à la sortie : signale la fin
            worker.run();
        })?;
        Ok(Self { tx, stop, pair_cancel, done, thread: Some(thread) })
    }

    pub fn send(&self, cmd: Cmd) {
        match cmd {
            Cmd::Pair => self.pair_cancel.store(false, Ordering::SeqCst),
            Cmd::PairCancel => self.pair_cancel.store(true, Ordering::SeqCst),
            _ => {}
        }
        let _ = self.tx.send(cmd);
    }

    /// Arrête le service ; `true` si le thread a fini dans le délai (sinon
    /// il est détaché et se terminera seul).
    pub fn stop(mut self, wait: Duration) -> bool {
        self.shutdown(wait)
    }

    fn shutdown(&mut self, wait: Duration) -> bool {
        self.stop.store(true, Ordering::SeqCst);
        self.pair_cancel.store(true, Ordering::SeqCst);
        let _ = self.tx.send(Cmd::PairCancel);
        let finished = matches!(self.done.recv_timeout(wait), Err(RecvTimeoutError::Disconnected));
        if let Some(t) = self.thread.take() {
            if finished {
                let _ = t.join();
            }
        }
        finished
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.shutdown(Duration::from_secs(2));
        }
    }
}

fn now_ms() -> u64 {
    hlc::now_millis()
}

/// Lignes affichables des appareils du profil.
pub fn rows(engine: &SyncEngine) -> Vec<DeviceRow> {
    let me = engine.device_id();
    engine
        .devices()
        .unwrap_or_default()
        .into_iter()
        .map(|d| DeviceRow {
            this_device: d.device_id == me,
            id: d.device_id,
            name: d.name,
            added_at_ms: d.added_at,
            revoked: d.revoked,
        })
        .collect()
}

pub fn read_last(vault: &Vault) -> Option<LastSync> {
    serde_json::from_str(&vault.setting(LAST_SYNC_KEY).ok()??).ok()
}

fn save_last(vault: &Vault, last: &LastSync) {
    if let Ok(json) = serde_json::to_string(last) {
        let _ = vault.set_setting(LAST_SYNC_KEY, &json);
    }
}

/// État du thread (emprunte sa connexion au coffre).
struct State<'v> {
    vault: &'v Vault,
    engine: SyncEngine<'v>,
    listener: Option<TcpListener>,
    port: u16,
    ad: Option<Advertiser>,
}

struct Worker<E> {
    cfg: Config,
    rx: Receiver<Cmd>,
    stop: Arc<AtomicBool>,
    pair_cancel: Arc<AtomicBool>,
    emit: E,
}

impl<E: Fn(Event)> Worker<E> {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    fn run(&self) {
        let vault = match Vault::open_with(&self.cfg.db, &self.cfg.key) {
            Ok(v) => v,
            Err(e) => return (self.emit)(Event::Failed(format!("Coffre inaccessible pour la synchronisation : {e:#}"))),
        };
        let engine = match SyncEngine::open(&vault) {
            Ok(e) => e,
            Err(e) => return (self.emit)(Event::Failed(format!("Synchronisation indisponible : {e:#}"))),
        };
        if let Some(last) = read_last(&vault) {
            (self.emit)(Event::LastSync(last));
        }
        (self.emit)(Event::Devices(rows(&engine)));

        let mut st = State { vault: &vault, engine, listener: None, port: 0, ad: None };
        match nyx_sync::bind_first(self.cfg.bind, self.cfg.ports.iter().copied())
            .and_then(|l| l.set_nonblocking(true).map(|()| l))
            .and_then(|l| l.local_addr().map(|a| (l, a.port())))
        {
            Ok((l, port)) => {
                st.listener = Some(l);
                st.port = port;
                self.advertise(&mut st);
                (self.emit)(Event::Listening { port: Some(port), error: None });
            }
            Err(e) => (self.emit)(Event::Listening {
                port: None,
                error: Some(format!("Aucun port libre entre 47420 et 47429 ({e})")),
            }),
        }

        let mut next = Instant::now();
        while !self.stopped() {
            if let Some(stream) = st.listener.as_ref().and_then(|l| match l.accept() {
                Ok((s, _)) => Some(s),
                Err(e) if e.kind() == ErrorKind::WouldBlock => None,
                Err(_) => None,
            }) {
                self.serve(&st, stream);
                continue;
            }
            if Instant::now() >= next {
                self.sync_round(&st, false);
                next = Instant::now() + self.cfg.period;
            }
            match self.rx.recv_timeout(TICK) {
                Ok(cmd) => self.handle(&mut st, cmd),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        // Annonce mDNS retirée (drop), connexion au coffre fermée.
        drop(st);
    }

    fn advertise(&self, st: &mut State) {
        st.ad = None;
        if self.cfg.mdns && st.listener.is_some() {
            st.ad = Advertiser::start(&st.engine, st.port).ok();
        }
    }

    fn serve(&self, st: &State, stream: TcpStream) {
        // Échecs entrants (appareil inconnu, connexion coupée) : silencieux.
        let Ok(r) = nyx_sync::serve_stream(&st.engine, stream) else { return };
        let last = LastSync {
            at_ms: now_ms(),
            ok: true,
            message: format!("Synchronisé avec {} ({} reçus, {} envoyés)", r.peer.name, r.received.applied, r.sent),
        };
        save_last(st.vault, &last);
        (self.emit)(Event::Synced { last, changed: r.received.applied > 0 });
        (self.emit)(Event::Devices(rows(&st.engine)));
    }

    /// Synchronisation sortante. Périodique (`manual` faux) : silencieuse
    /// s'il n'y a rien à dire (aucun appareil lié ou trouvé).
    fn sync_round(&self, st: &State, manual: bool) {
        let summary = if self.cfg.mdns {
            discovery::sync_all(&st.engine, self.cfg.browse)
        } else {
            Ok(SyncSummary {
                known: st.engine.profile().map(|p| p.active().count().saturating_sub(1)).unwrap_or(0),
                ..SyncSummary::default()
            })
        };
        let (last, changed) = match summary {
            Ok(s) => {
                if !manual && s.reports.is_empty() && s.failures.is_empty() {
                    return;
                }
                (LastSync { at_ms: now_ms(), ok: s.ok(), message: s.message() }, s.changed())
            }
            Err(e) => {
                if !manual {
                    return;
                }
                (LastSync { at_ms: now_ms(), ok: false, message: format!("Découverte réseau impossible : {e:#}") }, false)
            }
        };
        save_last(st.vault, &last);
        (self.emit)(Event::Synced { last, changed });
        (self.emit)(Event::Devices(rows(&st.engine)));
    }

    fn handle(&self, st: &mut State, cmd: Cmd) {
        match cmd {
            Cmd::SyncNow => self.sync_round(st, true),
            Cmd::Pair => {
                let result = self.pair(st);
                (self.emit)(Event::PairDone(result));
                (self.emit)(Event::Devices(rows(&st.engine)));
            }
            Cmd::Rename(name) => {
                let msg = match st.engine.set_device_name(&name) {
                    Ok(()) => "Nom de l'appareil enregistré".to_string(),
                    Err(e) => nyx_sync::user_message(&e),
                };
                (self.emit)(Event::Devices(rows(&st.engine)));
                (self.emit)(Event::Notice(msg));
            }
            Cmd::Revoke(id) => {
                let msg = match st.engine.revoke(&id) {
                    Ok(()) => {
                        // Secret de profil changé : nouvelle étiquette mDNS.
                        self.advertise(st);
                        "Appareil retiré. S'il a été perdu ou volé, changez vos mots de passe importants.".to_string()
                    }
                    Err(e) => nyx_sync::user_message(&e),
                };
                (self.emit)(Event::Devices(rows(&st.engine)));
                (self.emit)(Event::Notice(msg));
            }
            Cmd::PairAnswer(_) | Cmd::PairCancel => {}
        }
    }

    fn pair(&self, st: &mut State) -> Result<String, String> {
        let Some(listener) = st.listener.as_ref() else {
            return Err("Service de synchronisation indisponible (aucun port réseau libre)".into());
        };
        let addrs: Vec<IpAddr> = match &self.cfg.offer_addrs {
            Some(a) => a.clone(),
            None => pairing::lan_addrs().into_iter().filter(|ip| ip.is_ipv4() == self.cfg.bind.is_ipv4()).collect(),
        };
        if addrs.is_empty() {
            return Err("Aucune adresse sur le réseau local : connectez ce PC au Wi-Fi ou au réseau filaire".into());
        }
        let mut offer = pairing::Offer::new(&st.engine, addrs, st.port).map_err(|e| nyx_sync::user_message(&e))?;
        let encoded = offer.encode();
        let qr_svg = qr::svg(&encoded).ok_or("QR code impossible à générer")?;
        (self.emit)(Event::PairOffer { offer: encoded, qr_svg, expires_at_ms: offer.info().expires_at * 1000 });

        st.engine.set_io_timeout(PAIR_IO_TIMEOUT);
        let result = pairing::host_until(
            &st.engine,
            listener,
            &mut offer,
            || self.stopped() || self.pair_cancel.load(Ordering::SeqCst),
            |code, dev| self.confirm(code, &dev.name),
        );
        let _ = listener.set_nonblocking(true);
        st.engine.set_io_timeout(nyx_sync::DEFAULT_IO_TIMEOUT);
        result.map(|d| d.name).map_err(|e| nyx_sync::user_message(&e))
    }

    /// Code affiché, attente de la réponse de l'utilisateur (sur ce thread).
    fn confirm(&self, code: &str, device: &str) -> bool {
        (self.emit)(Event::PairCode { code: code.to_string(), device: device.to_string() });
        let deadline = Instant::now() + CONFIRM_TIMEOUT;
        loop {
            if self.stopped() || self.pair_cancel.load(Ordering::SeqCst) {
                return false;
            }
            let Some(left) = deadline.checked_duration_since(Instant::now()) else { return false };
            match self.rx.recv_timeout(left.min(TICK)) {
                Ok(Cmd::PairAnswer(yes)) => return yes,
                Ok(Cmd::PairCancel) | Err(RecvTimeoutError::Disconnected) => return false,
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    const LOCAL: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

    struct Tmp(PathBuf);

    impl Tmp {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir().join(format!("nyx_win_sync_{}_{name}.db", std::process::id()));
            let _ = std::fs::remove_file(&p);
            Self(p)
        }
        fn vault(&self) -> Vault {
            Vault::open(&self.0, "pw").expect("vault")
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            for ext in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{ext}", self.0.display()));
            }
        }
    }

    fn config(t: &Tmp) -> Config {
        Config {
            bind: LOCAL,
            ports: vec![0],
            offer_addrs: Some(vec![LOCAL]),
            mdns: false,
            ..Config::new(t.0.clone(), Key::passphrase("pw"))
        }
    }

    fn start(t: &Tmp) -> (Service, Receiver<Event>) {
        drop(t.vault()); // coffre créé avant le service
        let (tx, rx) = mpsc::channel();
        let svc = Service::start(config(t), move |e| {
            let _ = tx.send(e);
        })
        .expect("start");
        (svc, rx)
    }

    fn wait_for<T>(rx: &Receiver<Event>, mut pick: impl FnMut(Event) -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let left = deadline.checked_duration_since(Instant::now()).expect("événement attendu");
            if let Some(v) = pick(rx.recv_timeout(left).expect("événement")) {
                return v;
            }
        }
    }

    fn port(rx: &Receiver<Event>) -> u16 {
        wait_for(rx, |e| match e {
            Event::Listening { port, .. } => Some(port.expect("port")),
            _ => None,
        })
    }

    #[test]
    fn appairage_confirme_puis_synchro_entrante() {
        let (pc, phone) = (Tmp::new("pair_pc"), Tmp::new("pair_phone"));
        let (svc, rx) = start(&pc);
        let port = port(&rx);
        svc.send(Cmd::Pair);
        let (offer, svg) = wait_for(&rx, |e| match e {
            Event::PairOffer { offer, qr_svg, expires_at_ms } => {
                assert!(expires_at_ms > now_ms());
                Some((offer, qr_svg))
            }
            _ => None,
        });
        assert!(offer.starts_with("nyxpair:v1:") && svg.starts_with("<?xml"));

        let phone_path = phone.0.clone();
        let guest = std::thread::spawn(move || {
            let v = Vault::open(&phone_path, "pw").unwrap();
            let e = SyncEngine::open(&v).unwrap();
            let mut seen = String::new();
            let pc = pairing::accept(&offer, &e, "Pixel", |code, _| {
                seen = code.to_string();
                true
            })
            .map(|d| d.name);
            (pc, seen)
        });
        let (code, device) = wait_for(&rx, |e| match e {
            Event::PairCode { code, device } => Some((code, device)),
            _ => None,
        });
        assert_eq!(device, "Pixel");
        svc.send(Cmd::PairAnswer(true));
        let done = wait_for(&rx, |e| match e {
            Event::PairDone(r) => Some(r),
            _ => None,
        });
        assert_eq!(done, Ok("Pixel".to_string()));
        let (pc_name, guest_code) = guest.join().unwrap();
        assert_eq!(pc_name.expect("accept"), "Nyx");
        assert_eq!(guest_code, code, "mêmes codes des deux côtés");
        let devices = wait_for(&rx, |e| match e {
            Event::Devices(d) if d.len() == 2 => Some(d),
            _ => None,
        });
        assert!(devices.iter().any(|d| d.this_device) && devices.iter().any(|d| d.name == "Pixel"));

        // Le téléphone se synchronise vers le PC : le service reçoit, applique
        // et signale « changed ».
        {
            let v = phone.vault();
            v.add_bookmark(&vault::Bookmark::new("https://mobile.org", "Mobile")).unwrap();
            let e = SyncEngine::open(&v).unwrap();
            let r = nyx_sync::sync_with(&e, (LOCAL, port).into(), None).expect("sync");
            assert_eq!(r.peer.name, "Nyx");
        }
        let (last, changed) = wait_for(&rx, |e| match e {
            Event::Synced { last, changed } => Some((last, changed)),
            _ => None,
        });
        assert!(changed && last.ok && last.message.starts_with("Synchronisé avec Pixel"), "{last:?}");
        assert!(svc.stop(Duration::from_secs(5)), "arrêt propre");
        let v = pc.vault();
        assert!(v.list_bookmarks().unwrap().iter().any(|b| b.url == "https://mobile.org"));
        assert_eq!(read_last(&v).map(|l| l.ok), Some(true), "dernière synchro persistée");
    }

    #[test]
    fn appairage_refuse_ou_annule() {
        let (pc, phone) = (Tmp::new("refuse_pc"), Tmp::new("refuse_phone"));
        let (svc, rx) = start(&pc);
        port(&rx);
        svc.send(Cmd::Pair);
        let offer = wait_for(&rx, |e| match e {
            Event::PairOffer { offer, .. } => Some(offer),
            _ => None,
        });
        let phone_path = phone.0.clone();
        let guest = std::thread::spawn(move || {
            let v = Vault::open(&phone_path, "pw").unwrap();
            let e = SyncEngine::open(&v).unwrap();
            pairing::accept(&offer, &e, "Intrus", |_, _| true).is_err()
        });
        wait_for(&rx, |e| matches!(e, Event::PairCode { .. }).then_some(()));
        svc.send(Cmd::PairAnswer(false));
        let done = wait_for(&rx, |e| match e {
            Event::PairDone(r) => Some(r),
            _ => None,
        });
        assert_eq!(done, Err("Appairage annulé".to_string()));
        assert!(guest.join().unwrap(), "le téléphone voit le refus");

        // Annulation pendant l'attente du téléphone.
        svc.send(Cmd::Pair);
        wait_for(&rx, |e| matches!(e, Event::PairOffer { .. }).then_some(()));
        svc.send(Cmd::PairCancel);
        let done = wait_for(&rx, |e| match e {
            Event::PairDone(r) => Some(r),
            _ => None,
        });
        assert!(done.is_err());
        let devices = wait_for(&rx, |e| match e {
            Event::Devices(d) => Some(d),
            _ => None,
        });
        assert_eq!(devices.len(), 1, "personne n'a été ajouté");
        assert!(svc.stop(Duration::from_secs(5)));
    }

    #[test]
    fn renommer_synchro_manuelle_et_arret_pendant_appairage() {
        let pc = Tmp::new("rename_pc");
        let (svc, rx) = start(&pc);
        port(&rx);
        svc.send(Cmd::Rename("  Bureau  ".into()));
        let rows = wait_for(&rx, |e| match e {
            Event::Devices(d) if d.iter().any(|r| r.name == "Bureau") => Some(d),
            _ => None,
        });
        assert!(rows[0].this_device);
        svc.send(Cmd::SyncNow);
        let last = wait_for(&rx, |e| match e {
            Event::Synced { last, .. } => Some(last),
            _ => None,
        });
        assert_eq!(last.message, "Aucun appareil lié");
        svc.send(Cmd::Revoke(rows[0].id.clone()));
        let msg = wait_for(&rx, |e| match e {
            Event::Notice(m) => Some(m),
            _ => None,
        });
        assert!(msg.to_lowercase().contains("impossible"), "{msg}");
        // Arrêt pendant l'attente d'un téléphone : rapide.
        svc.send(Cmd::Pair);
        wait_for(&rx, |e| matches!(e, Event::PairOffer { .. }).then_some(()));
        let t = Instant::now();
        assert!(svc.stop(Duration::from_secs(5)));
        assert!(t.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn mauvaise_cle_signalee() {
        let pc = Tmp::new("badkey");
        drop(pc.vault());
        let (tx, rx) = mpsc::channel();
        let cfg = Config { key: Key::passphrase("autre"), ..config(&pc) };
        let svc = Service::start(cfg, move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        let msg = wait_for(&rx, |e| match e {
            Event::Failed(m) => Some(m),
            _ => None,
        });
        assert!(msg.starts_with("Coffre inaccessible"));
        assert!(svc.stop(Duration::from_secs(2)));
    }
}
