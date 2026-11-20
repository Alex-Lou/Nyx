//! Paire de clés statique Noise (X25519) de l'appareil.
//!
//! Générée une fois, stockée dans le coffre (`sync_meta`, chiffré au repos
//! par SQLCipher), ne quitte jamais l'appareil. La clé publique identifie
//! l'appareil auprès des autres membres du profil.

use anyhow::{anyhow, Result};
use std::fmt;
use vault::Vault;

use crate::wire::noise;

const META_KEY: &str = "sync.device_key";

pub struct DeviceKeys {
    private: [u8; 32],
    public: [u8; 32],
}

impl DeviceKeys {
    /// Nouvelle paire aléatoire (CSPRNG de l'OS via snow/getrandom).
    pub fn generate() -> Result<Self> {
        let kp = snow::Builder::new(crate::session::PARAMS.parse().map_err(noise)?)
            .generate_keypair()
            .map_err(noise)?;
        Self::from_parts(&kp.private, &kp.public)
    }

    fn from_parts(private: &[u8], public: &[u8]) -> Result<Self> {
        Ok(Self {
            private: private.try_into().map_err(|_| anyhow!("clé privée invalide"))?,
            public: public.try_into().map_err(|_| anyhow!("clé publique invalide"))?,
        })
    }

    /// Charge la paire de l'appareil depuis le coffre, ou la crée.
    pub fn load_or_create(vault: &Vault) -> Result<Self> {
        if let Some(raw) = vault.sync_meta(META_KEY)? {
            if raw.len() == 64 {
                return Self::from_parts(&raw[..32], &raw[32..]);
            }
            return Err(anyhow!("clé d'appareil corrompue dans le coffre"));
        }
        let k = Self::generate()?;
        let mut raw = [0u8; 64];
        raw[..32].copy_from_slice(&k.private);
        raw[32..].copy_from_slice(&k.public);
        vault.set_sync_meta(META_KEY, &raw)?;
        raw.fill(0);
        Ok(k)
    }

    pub fn public(&self) -> &[u8; 32] {
        &self.public
    }

    pub(crate) fn private(&self) -> &[u8; 32] {
        &self.private
    }
}

impl fmt::Debug for DeviceKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceKeys").field("public", &crate::profile::hex(&self.public)).finish_non_exhaustive()
    }
}

impl Drop for DeviceKeys {
    fn drop(&mut self) {
        // Effacement au mieux (sans `unsafe` : pas de garantie absolue).
        self.private.fill(0);
        std::hint::black_box(&self.private);
    }
}
