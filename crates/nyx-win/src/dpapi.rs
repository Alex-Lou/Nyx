//! Scellement DPAPI de la clé d'appareil (`%APPDATA%\Nyx\vault.key`).
//!
//! `CryptProtectData` / `CryptUnprotectData`, portée utilisateur courant
//! (pas `CRYPTPROTECT_LOCAL_MACHINE`), sans interface
//! (`CRYPTPROTECT_UI_FORBIDDEN`), entropie = chaîne fixe propre à Nyx (un
//! autre programme du même compte doit au moins la connaître). Un autre
//! compte Windows, ou la même session sur une autre machine sans profil
//! itinérant, ne peut pas desceller : `vault::keymode` retombe alors sur le
//! mot de passe principal, sans rien supprimer.
//!
//! Seul module `unsafe` avec `webview2.rs` : FFI Win32 confinée ici, deux
//! appels, tampons de sortie libérés par `LocalFree` après copie (et
//! effacés avant libération pour le clair).

#![allow(unsafe_code)]

use anyhow::{anyhow, Result};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

use vault::keyfile::Seal;

/// Entropie additionnelle (non secrète) : lie le blob à Nyx.
const ENTROPY: &[u8] = b"Nyx vault device key v1";

/// Protection DPAPI, utilisateur courant.
#[derive(Debug, Default, Clone, Copy)]
pub struct Dpapi;

fn blob(data: &[u8]) -> Result<CRYPT_INTEGER_BLOB> {
    Ok(CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(data.len()).map_err(|_| anyhow!("données trop grandes"))?,
        // DPAPI ne modifie pas l'entrée ; le type Win32 est mutable par héritage.
        pbData: data.as_ptr().cast_mut(),
    })
}

/// Copie le tampon alloué par DPAPI, l'efface (si `wipe`) puis le libère.
fn take(out: CRYPT_INTEGER_BLOB, wipe: bool) -> Vec<u8> {
    if out.pbData.is_null() {
        return Vec::new();
    }
    let len = out.cbData as usize;
    // SAFETY : en cas de succès, DPAPI renvoie `cbData` octets valides à
    // `pbData`, alloués par LocalAlloc, dont nous sommes propriétaires.
    let bytes = unsafe { std::slice::from_raw_parts_mut(out.pbData, len) };
    let copy = bytes.to_vec();
    if wipe {
        bytes.iter_mut().for_each(|b| *b = 0);
        std::hint::black_box(&bytes);
    }
    // SAFETY : tampon LocalAlloc rendu par DPAPI, libéré une seule fois.
    unsafe {
        let _ = LocalFree(Some(HLOCAL(out.pbData.cast())));
    }
    copy
}

impl Seal for Dpapi {
    fn seal(&self, plain: &[u8]) -> Result<Vec<u8>> {
        let input = blob(plain)?;
        let entropy = blob(ENTROPY)?;
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY : entrées valides pour la durée de l'appel ; `out` reçoit
        // un tampon LocalAlloc libéré par `take`.
        unsafe {
            CryptProtectData(
                &input,
                PCWSTR::null(),
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        }
        .map_err(|e| anyhow!("CryptProtectData : {e}"))?;
        Ok(take(out, false))
    }

    fn unseal(&self, sealed: &[u8]) -> Result<Vec<u8>> {
        let input = blob(sealed)?;
        let entropy = blob(ENTROPY)?;
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY : idem `seal` ; aucune description demandée.
        unsafe {
            CryptUnprotectData(&input, None, Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        }
        .map_err(|e| anyhow!("CryptUnprotectData : {e}"))?;
        Ok(take(out, true))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault::keyfile;

    #[test]
    fn round_trip_and_tamper() {
        let plain = [0x42u8; 32];
        let sealed = Dpapi.seal(&plain).expect("seal");
        assert!(!sealed.windows(32).any(|w| w == plain), "clé en clair dans le blob");
        assert_eq!(Dpapi.unseal(&sealed).expect("unseal"), plain);
        let mut bad = sealed.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0xff;
        assert!(Dpapi.unseal(&bad).is_err());
        assert!(Dpapi.unseal(&[]).is_err());
    }

    #[test]
    fn keyfile_with_dpapi() {
        let dir = std::env::temp_dir().join(format!("nyx_dpapi_{}", std::process::id()));
        let path = dir.join("vault.key");
        let key = vault::RawKey::generate().expect("rng");
        keyfile::write(&path, &key, &Dpapi).expect("write");
        assert_eq!(keyfile::read(&path, &Dpapi).expect("read"), key);
        assert!(keyfile::read(&path, &keyfile::OsAccountOnly).is_err(), "blob DPAPI ≠ clé brute");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
