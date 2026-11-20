//! Clés du coffre SQLCipher : passphrase (mot de passe principal, dérivée
//! par PBKDF2 côté SQLCipher) ou clé brute de 32 octets (clé d'appareil,
//! utilisée telle quelle : `PRAGMA key = "x'<64 hex>'"`).
//!
//! Le matériel de clé n'est jamais affiché (`Debug` masqué) et est effacé à
//! la destruction, en code sûr (octets mis à zéro puis `black_box`, comme
//! `nyx-win::secret`). Les copies faites par rusqlite / SQLCipher en
//! préparant le `PRAGMA` échappent à ce contrôle : on n'en ajoute pas.

use anyhow::{Context, Result};

/// Longueur d'une clé brute SQLCipher (AES-256).
pub const RAW_KEY_LEN: usize = 32;

/// Met des octets à zéro de façon (raisonnablement) non éliminable.
pub(crate) fn wipe(bytes: &mut [u8]) {
    bytes.iter_mut().for_each(|b| *b = 0);
    std::hint::black_box(bytes);
}

/// Chaîne sensible effacée à la destruction.
#[derive(Clone, PartialEq, Eq)]
pub struct Passphrase(String);

impl Passphrase {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Passphrase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Passphrase(***)")
    }
}

impl Drop for Passphrase {
    fn drop(&mut self) {
        let mut bytes = std::mem::take(&mut self.0).into_bytes();
        wipe(&mut bytes);
    }
}

/// Clé brute de 32 octets (clé d'appareil).
#[derive(Clone, PartialEq, Eq)]
pub struct RawKey([u8; RAW_KEY_LEN]);

impl RawKey {
    /// Nouvelle clé tirée du CSPRNG de l'OS.
    pub fn generate() -> Result<Self> {
        let mut k = Self([0; RAW_KEY_LEN]);
        getrandom::getrandom(&mut k.0).map_err(|e| anyhow::anyhow!("CSPRNG indisponible : {e}"))?;
        Ok(k)
    }

    /// Clé depuis exactement 32 octets.
    pub fn from_slice(bytes: &[u8]) -> Result<Self> {
        let arr: [u8; RAW_KEY_LEN] = bytes.try_into().ok().context("clé d'appareil : longueur invalide")?;
        Ok(Self(arr))
    }

    pub fn expose(&self) -> &[u8; RAW_KEY_LEN] {
        &self.0
    }

    /// Valeur SQLCipher `x'<64 hex minuscules>'` (chaîne effacée à la destruction).
    fn pragma_value(&self) -> Passphrase {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        // Capacité exacte : aucune réallocation, donc aucune copie orpheline.
        let mut s = String::with_capacity(RAW_KEY_LEN * 2 + 3);
        s.push_str("x'");
        for b in self.0 {
            s.push(HEX[usize::from(b >> 4)] as char);
            s.push(HEX[usize::from(b & 0x0f)] as char);
        }
        s.push('\'');
        Passphrase(s)
    }
}

impl std::fmt::Debug for RawKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RawKey(***)")
    }
}

impl Drop for RawKey {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

/// Clé d'ouverture / de création / de changement du coffre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// Mot de passe principal (dérivé par SQLCipher).
    Passphrase(Passphrase),
    /// Clé d'appareil brute.
    Raw(RawKey),
}

impl Key {
    pub fn passphrase(s: impl Into<String>) -> Self {
        Self::Passphrase(Passphrase::new(s))
    }

    /// `true` pour un mot de passe principal.
    pub fn is_passphrase(&self) -> bool {
        matches!(self, Self::Passphrase(_))
    }

    /// Valeur à passer à `PRAGMA key` / `PRAGMA rekey`.
    pub(crate) fn pragma_value(&self) -> Passphrase {
        match self {
            Self::Passphrase(p) => p.clone(),
            Self::Raw(k) => k.pragma_value(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_pragma_format() {
        let mut bytes = [0u8; RAW_KEY_LEN];
        bytes[0] = 0xab;
        bytes[1] = 0x01;
        bytes[31] = 0xff;
        let k = Key::Raw(RawKey::from_slice(&bytes).expect("32"));
        let v = k.pragma_value();
        assert_eq!(v.expose().len(), 67);
        assert!(v.expose().starts_with("x'ab01"));
        assert!(v.expose().ends_with("ff'"));
        assert!(v.expose()[2..66].bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_eq!(Key::passphrase("pw").pragma_value().expose(), "pw");
    }

    #[test]
    fn rejects_bad_lengths() {
        assert!(RawKey::from_slice(&[0; 31]).is_err());
        assert!(RawKey::from_slice(&[0; 33]).is_err());
        assert!(RawKey::from_slice(&[]).is_err());
    }

    #[test]
    fn generated_keys_differ() {
        let a = RawKey::generate().expect("rng");
        let b = RawKey::generate().expect("rng");
        assert_ne!(a, b);
        assert_ne!(a.expose(), &[0; RAW_KEY_LEN]);
    }

    #[test]
    fn never_printed() {
        let k = Key::passphrase("hunter2");
        assert!(!format!("{k:?}").contains("hunter2"));
        let r = RawKey::from_slice(&[0xab; RAW_KEY_LEN]).expect("32");
        let dbg = format!("{:?}", Key::Raw(r));
        assert!(!dbg.contains("ab") && !dbg.contains("171"), "{dbg}");
    }
}
