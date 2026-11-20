//! Appairage par QR : le PC (hôte) publie une offre courte durée, le
//! téléphone (invité) la scanne et rejoint le profil.
//!
//! ```text
//! offre = "nyxpair:v1:" base64url( v=1 | jeton[16] | clé_hôte[32]
//!                                  | expire_à u64 | port u16 | n | adresses… )
//! ```
//!
//! Canal : Noise `XXpsk3_25519_ChaChaPoly_BLAKE2s`, PSK = SHA-256(domaine ‖
//! jeton), prologue = octets exacts de l'offre (jeton compris). Un attaquant
//! réseau qui n'a pas vu le QR échoue dès le message 1 (prologue) et au plus
//! tard au message 3 (PSK). Même avec le QR (jeton volé), il ne peut pas se
//! faire passer pour le PC : l'invité exige que la clé statique reçue au
//! message 2 soit celle de l'offre, avant de révéler la sienne. Les deux
//! écrans affichent en plus un code à 6 chiffres dérivé du hash de
//! handshake ; l'utilisateur les compare.
//!
//! Jeton à usage unique (consommé au premier handshake réussi), expiration
//! ≤ 5 min, offre invalidée après [`MAX_ATTEMPTS`] handshakes ratés.

use anyhow::{bail, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, UdpSocket};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::profile::{Device, Profile};
use crate::wire::{self, confirmation_code, hs_read, hs_write, noise, Channel};
use crate::{hlc, SyncEngine, SyncError};

pub const PREFIX: &str = "nyxpair:v1:";
/// Durée de vie maximale d'une offre.
pub const MAX_TTL: Duration = Duration::from_secs(5 * 60);
/// Handshakes ratés tolérés avant invalidation de l'offre.
pub const MAX_ATTEMPTS: u32 = 5;
/// Longueur du jeton (128 bits).
pub const TOKEN_LEN: usize = 16;
const MAX_ADDRS: usize = 8;
const VERSION: u8 = 1;
const PARAMS: &str = "Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s";

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Contenu décodé d'une offre (ce que porte le QR).
#[derive(Clone, PartialEq, Eq)]
pub struct OfferInfo {
    token: [u8; TOKEN_LEN],
    /// Clé statique Noise du PC.
    pub host_key: [u8; 32],
    /// Adresses LAN du PC.
    pub addrs: Vec<IpAddr>,
    pub port: u16,
    /// Expiration (secondes Unix).
    pub expires_at: u64,
}

impl fmt::Debug for OfferInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OfferInfo")
            .field("token", &"<masqué>")
            .field("addrs", &self.addrs)
            .field("port", &self.port)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl OfferInfo {
    fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(60 + 17 * self.addrs.len());
        b.push(VERSION);
        b.extend_from_slice(&self.token);
        b.extend_from_slice(&self.host_key);
        b.extend_from_slice(&self.expires_at.to_be_bytes());
        b.extend_from_slice(&self.port.to_be_bytes());
        b.push(self.addrs.len() as u8);
        for a in &self.addrs {
            match a {
                IpAddr::V4(v4) => {
                    b.push(4);
                    b.extend_from_slice(&v4.octets());
                }
                IpAddr::V6(v6) => {
                    b.push(6);
                    b.extend_from_slice(&v6.octets());
                }
            }
        }
        b
    }

    fn from_bytes(b: &[u8]) -> Result<Self, SyncError> {
        let bad = SyncError::OfferInvalid;
        let mut r = b;
        let mut take = |n: usize| -> Result<&[u8], SyncError> {
            if r.len() < n {
                return Err(bad("tronquée"));
            }
            let (h, t) = r.split_at(n);
            r = t;
            Ok(h)
        };
        if take(1)?[0] != VERSION {
            return Err(bad("version inconnue"));
        }
        let token: [u8; TOKEN_LEN] = take(TOKEN_LEN)?.try_into().map_err(|_| bad("jeton"))?;
        let host_key: [u8; 32] = take(32)?.try_into().map_err(|_| bad("clé"))?;
        let expires_at = u64::from_be_bytes(take(8)?.try_into().map_err(|_| bad("date"))?);
        let port = u16::from_be_bytes(take(2)?.try_into().map_err(|_| bad("port"))?);
        let n = take(1)?[0] as usize;
        if n == 0 || n > MAX_ADDRS {
            return Err(bad("nombre d'adresses"));
        }
        let mut addrs = Vec::with_capacity(n);
        for _ in 0..n {
            let a = match take(1)?[0] {
                4 => IpAddr::V4(Ipv4Addr::from(<[u8; 4]>::try_from(take(4)?).map_err(|_| bad("ipv4"))?)),
                6 => IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(take(16)?).map_err(|_| bad("ipv6"))?)),
                _ => return Err(bad("famille d'adresse")),
            };
            addrs.push(a);
        }
        if !r.is_empty() {
            return Err(bad("octets en trop"));
        }
        if port == 0 {
            return Err(bad("port nul"));
        }
        Ok(Self { token, host_key, addrs, port, expires_at })
    }

    /// Chaîne compacte `nyxpair:v1:<base64url>` (à mettre dans le QR).
    pub fn encode(&self) -> String {
        format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(self.to_bytes()))
    }

    pub fn decode(s: &str) -> Result<Self, SyncError> {
        let body = s.trim().strip_prefix(PREFIX).ok_or(SyncError::OfferInvalid("préfixe"))?;
        if body.len() > 1024 {
            return Err(SyncError::OfferInvalid("trop longue"));
        }
        let bytes = URL_SAFE_NO_PAD.decode(body).map_err(|_| SyncError::OfferInvalid("base64"))?;
        Self::from_bytes(&bytes)
    }

    pub fn is_expired_at(&self, now_secs: u64) -> bool {
        now_secs >= self.expires_at
    }

    fn psk(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(b"nyx-pair-psk-v1");
        h.update(self.token);
        h.finalize().into()
    }

    fn prologue(&self) -> Vec<u8> {
        let mut p = b"nyx-pair/v1\0".to_vec();
        p.extend_from_slice(&self.to_bytes());
        p
    }

    fn builder<'a>(&self, engine: &'a SyncEngine, psk: &'a [u8; 32], prologue: &'a [u8]) -> Result<snow::Builder<'a>> {
        snow::Builder::new(PARAMS.parse().map_err(noise)?)
            .local_private_key(engine.keys().private())
            .map_err(noise)?
            .psk(3, psk)
            .map_err(noise)?
            .prologue(prologue)
            .map_err(noise)
    }
}

/// Offre côté PC : contenu + état (usage unique, compteur d'échecs).
pub struct Offer {
    info: OfferInfo,
    consumed: bool,
    failures: u32,
}

impl Offer {
    /// Offre valable [`MAX_TTL`] pour joindre ce PC sur `addrs:port`.
    pub fn new(engine: &SyncEngine, addrs: Vec<IpAddr>, port: u16) -> Result<Self> {
        Self::with_ttl(engine, addrs, port, MAX_TTL)
    }

    /// Idem avec une durée de vie choisie (plafonnée à [`MAX_TTL`]).
    pub fn with_ttl(engine: &SyncEngine, mut addrs: Vec<IpAddr>, port: u16, ttl: Duration) -> Result<Self> {
        addrs.dedup();
        addrs.truncate(MAX_ADDRS);
        if addrs.is_empty() || port == 0 {
            bail!(SyncError::OfferInvalid("adresse ou port manquant"));
        }
        let mut token = [0u8; TOKEN_LEN];
        getrandom::getrandom(&mut token).map_err(|e| anyhow::anyhow!("CSPRNG indisponible : {e}"))?;
        let ttl = ttl.min(MAX_TTL).as_secs();
        let info = OfferInfo { token, host_key: *engine.public_key(), addrs, port, expires_at: now_secs() + ttl };
        Ok(Self { info, consumed: false, failures: 0 })
    }

    /// Chaîne à afficher en QR.
    pub fn encode(&self) -> String {
        self.info.encode()
    }

    pub fn info(&self) -> &OfferInfo {
        &self.info
    }

    /// Encore utilisable (non consommée, non expirée).
    pub fn is_valid(&self) -> bool {
        !self.consumed && !self.info.is_expired_at(now_secs())
    }

    /// Handshakes ratés jusqu'ici.
    pub fn failures(&self) -> u32 {
        self.failures
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "m", rename_all = "snake_case")]
enum PairMsg {
    Join { device_id: String, name: String },
    Welcome { profile: Profile },
    Rejected,
    Ack,
    Nack,
    Done,
}

fn expect_protocol<T>(what: &'static str) -> Result<T> {
    Err(SyncError::Protocol(what).into())
}

/// Côté PC : attend l'invité sur `listener` jusqu'à succès, expiration ou
/// trop d'échecs. `confirm(code, appareil)` est appelé une fois le canal
/// établi : l'UI affiche le code et le nom proposé, l'utilisateur valide.
/// Retourne l'appareil ajouté au profil.
pub fn host<F>(engine: &SyncEngine, listener: &TcpListener, offer: &mut Offer, mut confirm: F) -> Result<Device>
where
    F: FnMut(&str, &Device) -> bool,
{
    listener.set_nonblocking(true)?;
    let result = loop {
        if offer.consumed {
            break Err(SyncError::TokenUsed.into());
        }
        if offer.info.is_expired_at(now_secs()) {
            break Err(SyncError::OfferExpired.into());
        }
        let stream = match listener.accept() {
            Ok((s, _)) => s,
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            Err(e) => break Err(e.into()),
        };
        match host_handshake(engine, &offer.info, stream) {
            Ok(ch) => {
                offer.consumed = true; // usage unique, quoi qu'il arrive ensuite
                break host_session(engine, ch, &mut confirm);
            }
            Err(_) => {
                offer.failures += 1;
                if offer.failures >= MAX_ATTEMPTS {
                    offer.consumed = true;
                    break Err(SyncError::TooManyAttempts.into());
                }
            }
        }
    };
    listener.set_nonblocking(false)?;
    result
}

fn host_handshake(engine: &SyncEngine, info: &OfferInfo, mut stream: std::net::TcpStream) -> Result<Channel> {
    wire::prepare(&stream, engine.io_timeout())?;
    let (psk, prologue) = (info.psk(), info.prologue());
    let mut hs = info.builder(engine, &psk, &prologue)?.build_responder().map_err(noise)?;
    hs_read(&mut stream, &mut hs)?;
    hs_write(&mut stream, &mut hs)?;
    hs_read(&mut stream, &mut hs)?; // échoue sans le bon jeton
    Channel::new(stream, hs)
}

fn host_session<F>(engine: &SyncEngine, mut ch: Channel, confirm: &mut F) -> Result<Device>
where
    F: FnMut(&str, &Device) -> bool,
{
    let PairMsg::Join { device_id, name } = ch.recv_json()? else { return expect_protocol("Join attendu") };
    let candidate = Device {
        device_id,
        name: name.trim().chars().take(64).collect(),
        public_key: ch.remote,
        added_at: hlc::now_millis(),
        revoked: false,
    };
    if candidate.device_id == engine.device_id() || candidate.public_key == *engine.public_key() {
        return expect_protocol("l'invité se présente comme l'hôte");
    }
    let mut proposed = engine.profile()?;
    proposed.upsert(candidate.clone())?;
    proposed.validate()?;

    let code = confirmation_code(&ch.hash);
    if !confirm(&code, &candidate) {
        let _ = ch.send_json(&PairMsg::Rejected);
        bail!(SyncError::Rejected);
    }
    ch.send_json(&PairMsg::Welcome { profile: proposed.clone() })?;
    match ch.recv_json()? {
        PairMsg::Ack => {}
        PairMsg::Nack => bail!(SyncError::Rejected),
        _ => return expect_protocol("Ack attendu"),
    }
    // Validation : relit le profil (il a pu changer entre-temps) et y ajoute
    // l'invité. Le PC valide en premier ; l'invité seulement après `Done`
    // (si `Done` se perd, un nouvel appairage remplace simplement l'entrée).
    let mut current = engine.profile()?;
    current.merge(&proposed);
    current.upsert(candidate.clone())?;
    engine.save_profile(&current)?;
    engine.vault().sync_forget_peer(&candidate.device_id)?;
    ch.send_json(&PairMsg::Done)?;
    Ok(candidate)
}

/// Côté téléphone : scanne `offer`, se connecte au PC, rejoint son profil
/// sous le nom `device_name`. `confirm(code, pc)` laisse l'utilisateur
/// comparer le code affiché par les deux appareils. Retourne le PC.
///
/// Rejoindre un profil REMPLACE le profil actuel de l'appareil (ses données
/// locales restent et seront fusionnées à la première synchronisation).
pub fn accept<F>(offer: &str, engine: &SyncEngine, device_name: &str, confirm: F) -> Result<Device>
where
    F: FnOnce(&str, &Device) -> bool,
{
    let info = OfferInfo::decode(offer)?;
    if info.is_expired_at(now_secs()) {
        bail!(SyncError::OfferExpired);
    }
    if info.host_key == *engine.public_key() {
        bail!(SyncError::Protocol("offre émise par cet appareil"));
    }
    let mut last: anyhow::Error = SyncError::HandshakeFailed.into();
    for ip in &info.addrs {
        match guest_handshake(engine, &info, SocketAddr::new(*ip, info.port)) {
            Ok(ch) => return guest_session(engine, &info, ch, device_name, confirm),
            Err(e) if e.downcast_ref::<SyncError>() == Some(&SyncError::HostKeyMismatch) => return Err(e),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn guest_handshake(engine: &SyncEngine, info: &OfferInfo, addr: SocketAddr) -> Result<Channel> {
    let mut stream = wire::connect(addr, engine.io_timeout())?;
    let (psk, prologue) = (info.psk(), info.prologue());
    let mut hs = info.builder(engine, &psk, &prologue)?.build_initiator().map_err(noise)?;
    hs_write(&mut stream, &mut hs)?;
    // Mauvais jeton/offre : l'hôte ne déchiffre pas le message 1 (PSK et
    // prologue y sont déjà liés) et coupe.
    hs_read(&mut stream, &mut hs).map_err(|e| if wire::is_io(&e) { SyncError::HandshakeFailed.into() } else { e })?;
    // Message 2 : clé statique de l'hôte → doit être celle du QR, sinon on
    // s'arrête AVANT de révéler la nôtre (message 3).
    if hs.get_remote_static() != Some(&info.host_key[..]) {
        bail!(SyncError::HostKeyMismatch);
    }
    hs_write(&mut stream, &mut hs)?;
    Channel::new(stream, hs)
}

fn guest_session<F>(engine: &SyncEngine, info: &OfferInfo, mut ch: Channel, name: &str, confirm: F) -> Result<Device>
where
    F: FnOnce(&str, &Device) -> bool,
{
    let name: String = name.trim().chars().take(64).collect();
    ch.send_json(&PairMsg::Join { device_id: engine.device_id().to_string(), name })?;
    let profile = match ch.recv_json() {
        Ok(PairMsg::Welcome { profile }) => profile,
        Ok(PairMsg::Rejected) => bail!(SyncError::Rejected),
        Ok(_) => return expect_protocol("Welcome attendu"),
        // Mauvais jeton : l'hôte coupe après le message 3.
        Err(e) if wire::is_io(&e) => bail!(SyncError::HandshakeFailed),
        Err(e) => return Err(e),
    };
    profile.validate()?;
    let host = match profile.by_key(&info.host_key) {
        Some(h) if !h.revoked => h.clone(),
        _ => return expect_protocol("hôte absent du profil"),
    };
    match profile.device(engine.device_id()) {
        Some(me) if me.public_key == *engine.public_key() && !me.revoked => {}
        _ => return expect_protocol("profil reçu sans cet appareil"),
    }
    if !confirm(&confirmation_code(&ch.hash), &host) {
        let _ = ch.send_json(&PairMsg::Nack);
        bail!(SyncError::Rejected);
    }
    ch.send_json(&PairMsg::Ack)?;
    match ch.recv_json()? {
        PairMsg::Done => {}
        _ => return expect_protocol("Done attendu"),
    }
    engine.save_profile(&profile)?;
    engine.vault().sync_forget_peer(&host.device_id)?;
    Ok(host)
}

/// Adresses LAN probables de cet appareil (IPv4 puis IPv6), sans envoyer de
/// paquet : l'OS choisit l'interface de sortie d'un socket UDP « connecté ».
pub fn lan_addrs() -> Vec<IpAddr> {
    let probe = |bind: &str, to: &str| -> Option<IpAddr> {
        let s = UdpSocket::bind(bind).ok()?;
        s.connect(to).ok()?;
        let ip = s.local_addr().ok()?.ip();
        (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
    };
    [probe("0.0.0.0:0", "192.0.2.1:9"), probe("[::]:0", "[2001:db8::1]:9")].into_iter().flatten().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(expires_at: u64) -> OfferInfo {
        OfferInfo {
            token: [7; TOKEN_LEN],
            host_key: [9; 32],
            addrs: vec!["192.168.1.20".parse().unwrap(), "fe80::1".parse().unwrap()],
            port: 47_833,
            expires_at,
        }
    }

    #[test]
    fn qr_encode_decode() {
        let o = sample(1_900_000_000);
        let s = o.encode();
        assert!(s.starts_with("nyxpair:v1:"));
        assert!(s.len() < 140, "QR compact : {} car.", s.len());
        assert!(s[PREFIX.len()..].bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
        assert_eq!(OfferInfo::decode(&s).unwrap(), o);
        assert_eq!(OfferInfo::decode(&format!("  {s}\n")).unwrap(), o);
        assert!(!format!("{o:?}").contains("7, 7"), "jeton masqué en Debug");
    }

    #[test]
    fn qr_rejets() {
        let s = sample(1).encode();
        assert_eq!(OfferInfo::decode("nyxpair:v2:AAAA"), Err(SyncError::OfferInvalid("préfixe")));
        assert_eq!(OfferInfo::decode("nyxpair:v1:!!!"), Err(SyncError::OfferInvalid("base64")));
        let bytes = sample(1).to_bytes();
        for cut in [1, 10, 40, bytes.len() - 1] {
            let t = format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(&bytes[..cut]));
            assert!(OfferInfo::decode(&t).is_err(), "tronquée à {cut}");
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(OfferInfo::decode(&format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(&extra))).is_err());
        let mut v2 = bytes;
        v2[0] = 2;
        assert_eq!(
            OfferInfo::decode(&format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(&v2))),
            Err(SyncError::OfferInvalid("version inconnue"))
        );
        assert!(OfferInfo::decode(&s).is_ok());
    }

    #[test]
    fn expiration() {
        let o = sample(1_000);
        assert!(!o.is_expired_at(999));
        assert!(o.is_expired_at(1_000));
        assert!(o.is_expired_at(5_000));
    }

    struct Tmp(std::path::PathBuf);
    impl Tmp {
        fn new(n: &str) -> Self {
            let p = std::env::temp_dir().join(format!("nyx_sync_pair_{}_{n}.db", std::process::id()));
            let _ = std::fs::remove_file(&p);
            Self(p)
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            for ext in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{ext}", self.0.display()));
            }
        }
    }

    /// Attaquant qui a VU le QR (jeton compris) mais ne possède pas la clé
    /// privée du PC : il répond avec sa propre clé statique. L'invité doit
    /// s'arrêter après le message 2, sans avoir révélé sa clé ni son profil.
    #[test]
    fn mitm_avec_jeton_vole_mais_mauvaise_cle_statique() {
        let (tp, ta, tg) = (Tmp::new("mitm_pc"), Tmp::new("mitm_att"), Tmp::new("mitm_guest"));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let pc_vault = vault::Vault::open(&tp.0, "pw").unwrap();
        let pc = SyncEngine::open(&pc_vault).unwrap();
        let genuine = Offer::new(&pc, vec![IpAddr::V4(Ipv4Addr::LOCALHOST)], port).unwrap();
        let info = genuine.info().clone();
        let qr = genuine.encode();
        let att_path = ta.0.clone();
        let h = std::thread::spawn(move || {
            let v = vault::Vault::open(&att_path, "pw").unwrap();
            let mut att = SyncEngine::open(&v).unwrap();
            att.set_io_timeout(Duration::from_secs(3));
            let (s, _) = listener.accept().unwrap();
            // Mêmes PSK et prologue que le vrai PC, autre clé statique.
            host_handshake(&att, &info, s).map(|_| ())
        });
        let gv = vault::Vault::open(&tg.0, "pw").unwrap();
        let mut guest = SyncEngine::open(&gv).unwrap();
        guest.set_io_timeout(Duration::from_secs(3));
        let err = accept(&qr, &guest, "victime", |_, _| panic!("aucune confirmation")).unwrap_err();
        assert_eq!(err.downcast_ref::<SyncError>(), Some(&SyncError::HostKeyMismatch));
        assert!(h.join().unwrap().is_err(), "l'attaquant n'obtient pas de canal");
        assert_eq!(guest.devices().unwrap().len(), 1, "profil de l'invité intact");
    }

    #[test]
    fn psk_lie_au_jeton() {
        let a = sample(1);
        let mut b = sample(1);
        b.token[0] ^= 1;
        assert_ne!(a.psk(), b.psk());
        assert_ne!(a.prologue(), b.prologue());
    }
}
