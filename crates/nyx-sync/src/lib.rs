//! Synchronisation sans compte, chiffrée de bout en bout, entre les appareils
//! d'une même personne (phase A : réseau local uniquement).
//!
//! * Identité = clés : chaque appareil a une paire statique Noise X25519
//!   ([`DeviceKeys`]) qui ne le quitte jamais ; les appareils d'une personne
//!   partagent un [`Profile`] (secret 32 octets + liste d'appareils), stocké
//!   dans le coffre SQLCipher.
//! * Appairage ([`pairing`]) : le PC affiche une offre `nyxpair:v1:…` (QR),
//!   le téléphone s'y connecte en Noise `XXpsk3` lié au jeton à usage unique.
//! * Synchronisation ([`session`]) : TCP + Noise `KK` entre appareils
//!   appairés, échange de filigranes puis de lots de changements, fusion
//!   « dernier écrivain gagne » par enregistrement (côté `vault`).
//! * Découverte ([`discovery`], feature `mdns`) : `_nyx-sync._tcp` avec une
//!   étiquette opaque dérivée du secret de profil ; repli IP manuelle.
//!
//! Tout est bloquant (threads std + `TcpStream`), sans runtime async. Un
//! [`SyncEngine`] emprunte un `Vault` (non `Sync`) : les coquilles le font
//! vivre sur un thread dédié avec sa propre connexion au coffre.
//!
//! Voir `docs/sync.md` pour le modèle de menace.

use anyhow::{anyhow, Result};
use std::fmt;
use std::time::Duration;
use vault::Vault;

#[cfg(feature = "mdns")]
pub mod discovery;
mod keys;
pub mod pairing;
pub mod profile;
pub mod session;
mod wire;

pub use keys::DeviceKeys;
pub use profile::{Device, Profile};
pub use session::{serve_one, serve_stream, sync_with, SyncReport};
pub use vault::hlc;
pub use vault::{ApplyStats, Table};
pub use wire::confirmation_code;

/// Port TCP par défaut (saisie manuelle d'IP ; mDNS publie le port réel).
pub const DEFAULT_PORT: u16 = 47_833;

/// Délai d'entrée/sortie par défaut d'une connexion.
pub const DEFAULT_IO_TIMEOUT: Duration = Duration::from_secs(20);

/// Erreurs typées (encapsulées dans `anyhow::Error`, récupérables par
/// `downcast_ref::<SyncError>()`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncError {
    /// Chaîne d'offre illisible.
    OfferInvalid(&'static str),
    /// Offre expirée.
    OfferExpired,
    /// Jeton déjà utilisé (ou invalidé après trop d'essais).
    TokenUsed,
    /// Trop d'essais d'appairage ratés : l'offre est invalidée.
    TooManyAttempts,
    /// La clé statique du PC ne correspond pas à celle de l'offre (MITM).
    HostKeyMismatch,
    /// Handshake Noise échoué (mauvais jeton, mauvaise clé, altération).
    HandshakeFailed,
    /// Refus de l'utilisateur (codes différents, annulation).
    Rejected,
    /// Pair inconnu ou révoqué.
    UnknownPeer,
    /// Violation de protocole.
    Protocol(&'static str),
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncError::OfferInvalid(why) => write!(f, "offre d'appairage invalide : {why}"),
            SyncError::OfferExpired => f.write_str("offre d'appairage expirée"),
            SyncError::TokenUsed => f.write_str("jeton d'appairage déjà utilisé"),
            SyncError::TooManyAttempts => f.write_str("trop d'essais d'appairage, offre invalidée"),
            SyncError::HostKeyMismatch => f.write_str("clé de l'appareil hôte inattendue (interception ?)"),
            SyncError::HandshakeFailed => f.write_str("échec du handshake chiffré"),
            SyncError::Rejected => f.write_str("appairage refusé"),
            SyncError::UnknownPeer => f.write_str("appareil inconnu ou révoqué"),
            SyncError::Protocol(why) => write!(f, "erreur de protocole : {why}"),
        }
    }
}

impl std::error::Error for SyncError {}

/// Moteur de synchronisation d'un appareil : coffre + clés + profil.
pub struct SyncEngine<'v> {
    vault: &'v Vault,
    keys: DeviceKeys,
    io_timeout: Duration,
}

impl<'v> SyncEngine<'v> {
    /// Moteur sur `vault` avec `keys` (voir [`DeviceKeys::load_or_create`]).
    /// Crée un profil (seul membre : cet appareil) s'il n'en existe pas.
    pub fn new(vault: &'v Vault, keys: DeviceKeys) -> Result<Self> {
        let engine = Self { vault, keys, io_timeout: DEFAULT_IO_TIMEOUT };
        let me = engine.self_device("Nyx");
        let profile = match Profile::load(vault)? {
            None => Profile::new(me)?,
            Some(mut p) => {
                // Clés régénérées → notre propre entrée suit.
                match p.devices.iter_mut().find(|d| d.device_id == me.device_id) {
                    Some(d) if d.public_key == me.public_key => return Ok(engine),
                    Some(d) => d.public_key = me.public_key,
                    None => p.upsert(me)?,
                }
                p
            }
        };
        profile.save(vault)?;
        Ok(engine)
    }

    /// Raccourci : clés chargées (ou créées) depuis le coffre.
    pub fn open(vault: &'v Vault) -> Result<Self> {
        Self::new(vault, DeviceKeys::load_or_create(vault)?)
    }

    fn self_device(&self, name: &str) -> Device {
        Device {
            device_id: self.vault.device_id().to_string(),
            name: name.to_string(),
            public_key: *self.keys.public(),
            added_at: hlc::now_millis(),
            revoked: false,
        }
    }

    pub fn vault(&self) -> &'v Vault {
        self.vault
    }

    pub fn device_id(&self) -> &str {
        self.vault.device_id()
    }

    pub fn public_key(&self) -> &[u8; 32] {
        self.keys.public()
    }

    pub(crate) fn keys(&self) -> &DeviceKeys {
        &self.keys
    }

    pub fn io_timeout(&self) -> Duration {
        self.io_timeout
    }

    /// Délai d'entrée/sortie des connexions (défaut [`DEFAULT_IO_TIMEOUT`]).
    pub fn set_io_timeout(&mut self, d: Duration) {
        self.io_timeout = d.max(Duration::from_millis(100));
    }

    /// Profil courant (relu depuis le coffre : cohérent entre threads).
    pub fn profile(&self) -> Result<Profile> {
        Profile::load(self.vault)?.ok_or_else(|| anyhow!("profil absent"))
    }

    /// Appareils du profil (révoqués compris, marqués).
    pub fn devices(&self) -> Result<Vec<Device>> {
        Ok(self.profile()?.devices)
    }

    /// Étiquette mDNS opaque du profil.
    pub fn discovery_tag(&self) -> Result<String> {
        Ok(self.profile()?.discovery_tag())
    }

    /// Nom de cet appareil (propagé aux autres lors des appairages suivants).
    pub fn set_device_name(&self, name: &str) -> Result<()> {
        let name: String = name.trim().chars().take(64).collect();
        let mut p = self.profile()?;
        if let Some(d) = p.devices.iter_mut().find(|d| d.device_id == self.device_id()) {
            d.name = name;
        }
        p.save(self.vault)
    }

    /// Révoque un appareil : marqué révoqué (plus aucune session acceptée)
    /// et rotation du secret de profil (nouvelle étiquette de découverte ;
    /// en phase B, nouvelle clé des blobs). Les autres appareils l'apprennent
    /// à leur prochaine synchronisation avec celui-ci. Ce que l'appareil
    /// révoqué possède déjà reste en sa possession (voir docs/sync.md).
    pub fn revoke(&self, device_id: &str) -> Result<()> {
        if device_id == self.device_id() {
            return Err(anyhow!("impossible de révoquer cet appareil-ci"));
        }
        let mut p = self.profile()?;
        if !p.revoke(device_id)? {
            return Err(SyncError::UnknownPeer.into());
        }
        p.save(self.vault)?;
        self.vault.sync_forget_peer(device_id)
    }

    /// Remplace le profil (appairage) ou fusionne celui d'un pair.
    pub(crate) fn save_profile(&self, p: &Profile) -> Result<()> {
        p.save(self.vault)
    }
}
