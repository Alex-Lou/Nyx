//! Profil : secret partagé + liste des appareils d'une même personne.
//!
//! Un profil = une personne (deux personnes d'un couple ont deux profils,
//! donc deux secrets, deux étiquettes de découverte : leurs appareils ne se
//! voient pas). Le profil est stocké dans le coffre (`sync_meta`, JSON,
//! chiffré au repos par SQLCipher) et transmis uniquement dans des canaux
//! Noise authentifiés (appairage, sessions KK).
//!
//! Fusion entre appareils ([`Profile::merge`]) : union des appareils,
//! `revoked` collant (une révocation ne s'annule jamais par fusion), le
//! secret de l'époque la plus haute gagne (égalité : plus grand secret, pour
//! un résultat déterministe).

use anyhow::{anyhow, Result};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::Sha256;
use std::fmt;
use vault::Vault;

const META_KEY: &str = "sync.profile";

/// Nombre maximal d'appareils dans un profil (borne aussi l'essai des clés
/// côté répondeur KK).
pub const MAX_DEVICES: usize = 32;
const MAX_NAME: usize = 64;

pub(crate) fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

mod hex32 {
    use super::*;
    pub fn serialize<S: Serializer>(v: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex(v))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let s = String::deserialize(d)?;
        unhex32(&s).ok_or_else(|| serde::de::Error::custom("clé hexadécimale de 32 octets attendue"))
    }
}

mod hex32_opt {
    use super::*;
    pub fn serialize<S: Serializer>(v: &Option<[u8; 32]>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(v) => s.serialize_some(&hex(v)),
            None => s.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<[u8; 32]>, D::Error> {
        match Option::<String>::deserialize(d)? {
            None => Ok(None),
            Some(s) => unhex32(&s).map(Some).ok_or_else(|| serde::de::Error::custom("clé invalide")),
        }
    }
}

/// Un appareil membre du profil.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// Identifiant stable (celui du coffre de l'appareil).
    pub device_id: String,
    /// Nom choisi par l'utilisateur (« PC du salon », « Pixel »…).
    pub name: String,
    /// Clé statique Noise X25519.
    #[serde(with = "hex32")]
    pub public_key: [u8; 32],
    /// Date d'ajout (ms Unix).
    pub added_at: u64,
    /// Révoqué : plus aucune session acceptée avec lui.
    pub revoked: bool,
}

impl Device {
    fn valid(&self) -> bool {
        !self.device_id.is_empty()
            && self.device_id.len() <= 64
            && self.device_id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && self.name.chars().count() <= MAX_NAME
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Profile {
    /// Incrémentée à chaque rotation du secret.
    pub epoch: u64,
    #[serde(with = "hex32")]
    secret: [u8; 32],
    /// Secret de l'époque précédente, gardé localement pour reconnaître en
    /// mDNS un appareil qui n'a pas encore appris la rotation.
    #[serde(with = "hex32_opt", default, skip_serializing_if = "Option::is_none")]
    previous_secret: Option<[u8; 32]>,
    pub devices: Vec<Device>,
}

impl fmt::Debug for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Profile")
            .field("epoch", &self.epoch)
            .field("secret", &"<masqué>")
            .field("devices", &self.devices)
            .finish()
    }
}

fn random32() -> Result<[u8; 32]> {
    let mut b = [0u8; 32];
    getrandom::getrandom(&mut b).map_err(|e| anyhow!("CSPRNG indisponible : {e}"))?;
    Ok(b)
}

fn hmac(key: &[u8; 32], parts: &[&[u8]]) -> [u8; 32] {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC accepte toute longueur de clé");
    for p in parts {
        m.update(p);
    }
    m.finalize().into_bytes().into()
}

impl Profile {
    /// Nouveau profil, seul membre : `me`.
    pub fn new(me: Device) -> Result<Self> {
        Ok(Self { epoch: 0, secret: random32()?, previous_secret: None, devices: vec![me] })
    }

    /// Secret de profil (32 octets). En phase A il dérive l'étiquette de
    /// découverte ; en phase B il dérivera la clé des blobs du relais.
    pub fn secret(&self) -> &[u8; 32] {
        &self.secret
    }

    fn tag_for(secret: &[u8; 32]) -> String {
        hex(&hmac(secret, &[b"nyx-discovery-v1"])[..8])
    }

    /// Étiquette mDNS opaque : HMAC-SHA256(secret, "nyx-discovery-v1")
    /// tronqué à 64 bits. Ne révèle ni nom ni clé ; change à chaque rotation.
    pub fn discovery_tag(&self) -> String {
        Self::tag_for(&self.secret)
    }

    /// Étiquettes acceptées en découverte (courante + précédente).
    pub fn accepted_tags(&self) -> Vec<String> {
        let mut v = vec![self.discovery_tag()];
        v.extend(self.previous_secret.as_ref().map(Self::tag_for));
        v
    }

    /// Indice opaque d'appareil (48 bits) publié en mDNS : seuls les membres
    /// du profil peuvent le relier à un appareil.
    pub fn device_hint(&self, device_id: &str) -> String {
        Self::hint_for(&self.secret, device_id)
    }

    fn hint_for(secret: &[u8; 32], device_id: &str) -> String {
        hex(&hmac(secret, &[b"nyx-device-v1", device_id.as_bytes()])[..6])
    }

    /// Indices acceptés en découverte pour `device_id` (courant + précédent).
    pub fn accepted_hints(&self, device_id: &str) -> Vec<String> {
        let mut v = vec![self.device_hint(device_id)];
        v.extend(self.previous_secret.as_ref().map(|s| Self::hint_for(s, device_id)));
        v
    }

    pub fn device(&self, id: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.device_id == id)
    }

    pub fn by_key(&self, key: &[u8; 32]) -> Option<&Device> {
        self.devices.iter().find(|d| &d.public_key == key)
    }

    /// Appareils non révoqués.
    pub fn active(&self) -> impl Iterator<Item = &Device> {
        self.devices.iter().filter(|d| !d.revoked)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        let ok = self.devices.len() <= MAX_DEVICES && self.devices.iter().all(Device::valid) && {
            let mut ids: Vec<&str> = self.devices.iter().map(|d| d.device_id.as_str()).collect();
            ids.sort_unstable();
            ids.windows(2).all(|w| w[0] != w[1])
        };
        if ok { Ok(()) } else { Err(crate::SyncError::Protocol("profil invalide").into()) }
    }

    /// Ajoute ou remplace (ré-appairage) un appareil.
    pub(crate) fn upsert(&mut self, d: Device) -> Result<()> {
        match self.devices.iter_mut().find(|x| x.device_id == d.device_id) {
            Some(x) => *x = d,
            None => {
                if self.devices.len() >= MAX_DEVICES {
                    return Err(anyhow!("trop d'appareils dans le profil ({MAX_DEVICES} au plus)"));
                }
                self.devices.push(d)
            }
        }
        Ok(())
    }

    /// Révoque un appareil et fait tourner le secret.
    pub(crate) fn revoke(&mut self, device_id: &str) -> Result<bool> {
        let Some(d) = self.devices.iter_mut().find(|d| d.device_id == device_id) else { return Ok(false) };
        d.revoked = true;
        self.rotate()?;
        Ok(true)
    }

    /// Nouveau secret aléatoire, époque + 1.
    pub(crate) fn rotate(&mut self) -> Result<()> {
        self.previous_secret = Some(self.secret);
        self.secret = random32()?;
        self.epoch += 1;
        Ok(())
    }

    /// Fusionne `other` dans `self`. Retourne `true` si `self` a changé.
    /// Commutative et idempotente sur (époque, secret, appareils, révocations).
    pub fn merge(&mut self, other: &Profile) -> bool {
        let mut changed = false;
        if (other.epoch, other.secret) > (self.epoch, self.secret) {
            self.previous_secret = Some(self.secret);
            self.secret = other.secret;
            self.epoch = other.epoch;
            changed = true;
        }
        for od in &other.devices {
            let n = self.devices.len();
            match self.devices.iter_mut().find(|d| d.device_id == od.device_id) {
                Some(d) => {
                    if od.revoked && !d.revoked {
                        d.revoked = true;
                        changed = true;
                    }
                }
                None if n < MAX_DEVICES && od.valid() => {
                    self.devices.push(od.clone());
                    changed = true;
                }
                None => {}
            }
        }
        changed
    }

    pub(crate) fn load(vault: &Vault) -> Result<Option<Self>> {
        match vault.sync_meta(META_KEY)? {
            None => Ok(None),
            Some(raw) => Ok(Some(serde_json::from_slice(&raw)?)),
        }
    }

    pub(crate) fn save(&self, vault: &Vault) -> Result<()> {
        vault.set_sync_meta(META_KEY, &serde_json::to_vec(self)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: &str, k: u8) -> Device {
        Device { device_id: id.into(), name: id.into(), public_key: [k; 32], added_at: 1, revoked: false }
    }

    #[test]
    fn serde_aller_retour_et_debug_masque() {
        let mut p = Profile::new(dev("a", 1)).unwrap();
        p.rotate().unwrap();
        let json = serde_json::to_string(&p).unwrap();
        let q: Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(q.secret, p.secret);
        assert_eq!(q.previous_secret, p.previous_secret);
        assert_eq!(q.devices, p.devices);
        assert!(!format!("{p:?}").contains(&hex(&p.secret)));
    }

    #[test]
    fn etiquettes_opaques_et_rotation() {
        let mut p = Profile::new(dev("a", 1)).unwrap();
        let tag = p.discovery_tag();
        assert_eq!(tag.len(), 16);
        assert!(!tag.contains(&hex(&[1u8; 4])));
        let hint = p.device_hint("a");
        assert_eq!(hint.len(), 12);
        assert_ne!(hint, p.device_hint("b"));
        p.rotate().unwrap();
        assert_ne!(p.discovery_tag(), tag);
        assert_eq!(p.accepted_tags(), vec![p.discovery_tag(), tag]);
        assert_eq!(p.epoch, 1);
    }

    #[test]
    fn fusion_union_revocation_collante_epoque_max() {
        let mut a = Profile::new(dev("a", 1)).unwrap();
        let mut b = a.clone();
        b.upsert(dev("b", 2)).unwrap();
        assert!(a.merge(&b));
        assert_eq!(a.devices.len(), 2);
        assert!(!a.merge(&b), "idempotente");

        // Révocation + rotation chez b → apprise par a.
        b.revoke("a2").unwrap_or(false);
        b.upsert(dev("c", 3)).unwrap();
        b.revoke("c").unwrap();
        assert!(a.merge(&b));
        assert!(a.device("c").unwrap().revoked);
        assert_eq!(a.secret, b.secret);
        assert_eq!(a.epoch, b.epoch);
        // Une copie ancienne (non révoquée) ne ressuscite pas c.
        let mut stale = Profile::new(dev("a", 1)).unwrap();
        stale.upsert(dev("c", 3)).unwrap();
        a.merge(&stale);
        assert!(a.device("c").unwrap().revoked);
        assert_eq!(a.secret, b.secret, "époque inférieure ignorée");

        // Rotations concurrentes à la même époque : résultat déterministe.
        let mut x = a.clone();
        let mut y = a.clone();
        x.rotate().unwrap();
        y.rotate().unwrap();
        let (mut x2, mut y2) = (x.clone(), y.clone());
        x2.merge(&y);
        y2.merge(&x);
        assert_eq!(x2.secret, y2.secret);
    }

    #[test]
    fn validation() {
        let mut p = Profile::new(dev("a", 1)).unwrap();
        assert!(p.validate().is_ok());
        p.devices.push(dev("a", 2));
        assert!(p.validate().is_err(), "identifiant dupliqué");
        let mut p = Profile::new(dev("a b", 1)).unwrap();
        assert!(p.validate().is_err());
        p.devices.clear();
        for i in 0..MAX_DEVICES {
            p.upsert(dev(&format!("d{i}"), i as u8)).unwrap();
        }
        assert!(p.upsert(dev("trop", 0)).is_err());
    }
}
