//! Fichier de clé d'appareil (`vault.key`, à côté de `vault.db`).
//!
//! Format : `NYXKEY1\n` suivi de la clé brute *scellée* par un [`Seal`] :
//! • Linux : [`OsAccountOnly`] — octets en clair, protégés par les droits du
//!   compte (fichier 0600, dossier 0700). Le trousseau Secret Service serait
//!   un progrès (cf. docs/security.md) ;
//! • Windows : DPAPI (`CryptProtectData`, portée utilisateur), fourni par
//!   `nyx-win` (seul endroit où vit le FFI).
//!
//! Écriture atomique : fichier temporaire du même dossier, `fsync`, puis
//! `rename` par-dessus la cible. Un plantage laisse l'ancien fichier ou le
//! nouveau, jamais un fichier tronqué.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::key::{wipe, RawKey};

/// En-tête du fichier (détecte un fichier étranger ou tronqué).
const MAGIC: &[u8] = b"NYXKEY1\n";

/// Protection de la clé au repos, propre à l'OS.
pub trait Seal {
    fn seal(&self, plain: &[u8]) -> Result<Vec<u8>>;
    fn unseal(&self, sealed: &[u8]) -> Result<Vec<u8>>;
}

/// Aucun chiffrement : la protection est celle du compte (droits du fichier).
#[derive(Debug, Default, Clone, Copy)]
pub struct OsAccountOnly;

impl Seal for OsAccountOnly {
    fn seal(&self, plain: &[u8]) -> Result<Vec<u8>> {
        Ok(plain.to_vec())
    }
    fn unseal(&self, sealed: &[u8]) -> Result<Vec<u8>> {
        Ok(sealed.to_vec())
    }
}

/// `vault.key` dans le même dossier que `db`.
pub fn path_for(db: &Path) -> PathBuf {
    db.with_file_name("vault.key")
}

/// Lit et descelle la clé. Erreur si absente, étrangère, tronquée ou si le
/// descellement échoue (autre compte Windows, fichier corrompu).
pub fn read(path: &Path, seal: &dyn Seal) -> Result<RawKey> {
    let mut data = std::fs::read(path).with_context(|| format!("lecture de {}", path.display()))?;
    let result = (|| {
        let Some(sealed) = data.strip_prefix(MAGIC) else { bail!("fichier de clé non reconnu") };
        let mut plain = seal.unseal(sealed).context("descellement de la clé d'appareil")?;
        let key = RawKey::from_slice(&plain);
        wipe(&mut plain);
        key
    })();
    wipe(&mut data);
    result
}

/// Écrit la clé scellée de façon atomique. Sous Unix : dossier 0700,
/// fichier 0600 dès sa création (jamais lisible par d'autres, même brièvement).
pub fn write(path: &Path, key: &RawKey, seal: &dyn Seal) -> Result<()> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).with_context(|| format!("création de {}", dir.display()))?;
    restrict_dir(dir)?;

    let mut sealed = seal.seal(key.expose()).context("scellement de la clé d'appareil")?;
    let mut data = Vec::with_capacity(MAGIC.len() + sealed.len());
    data.extend_from_slice(MAGIC);
    data.extend_from_slice(&sealed);
    wipe(&mut sealed);

    let tmp = temp_path(path);
    let result = write_new(&tmp, &data).and_then(|()| {
        std::fs::rename(&tmp, path).with_context(|| format!("remplacement de {}", path.display()))
    });
    wipe(&mut data);
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result?;
    sync_dir(dir);
    Ok(())
}

/// Supprime le fichier de clé (absent = déjà fait).
pub fn remove(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(e).with_context(|| format!("suppression de {}", path.display()))
        }
        _ => Ok(()),
    }
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(format!(".tmp-{}", std::process::id()));
    path.with_file_name(name)
}

fn write_new(tmp: &Path, data: &[u8]) -> Result<()> {
    let _ = std::fs::remove_file(tmp); // reste d'un essai interrompu
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(tmp).with_context(|| format!("création de {}", tmp.display()))?;
    f.write_all(data)?;
    f.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn restrict_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .with_context(|| format!("droits de {}", dir.display()))
}

#[cfg(not(unix))]
fn restrict_dir(_dir: &Path) -> Result<()> {
    Ok(())
}

/// `fsync` du dossier : rend le `rename` durable (au mieux, Unix).
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir().join(format!("nyx_keyfile_{}_{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            Self(p)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Scellement de test : XOR + tag, pour vérifier que le Seal est bien appelé.
    struct Xor;
    impl Seal for Xor {
        fn seal(&self, plain: &[u8]) -> Result<Vec<u8>> {
            let mut v: Vec<u8> = plain.iter().map(|b| b ^ 0x5a).collect();
            v.push(0x99);
            Ok(v)
        }
        fn unseal(&self, sealed: &[u8]) -> Result<Vec<u8>> {
            match sealed.split_last() {
                Some((0x99, body)) => Ok(body.iter().map(|b| b ^ 0x5a).collect()),
                _ => bail!("tag"),
            }
        }
    }

    #[test]
    fn round_trip_and_replace() {
        let dir = TempDir::new("rt");
        let path = dir.0.join("nyx").join("vault.key");
        let k1 = RawKey::generate().expect("rng");
        write(&path, &k1, &OsAccountOnly).expect("write");
        assert_eq!(read(&path, &OsAccountOnly).expect("read"), k1);
        let raw = std::fs::read(&path).expect("raw");
        assert_eq!(raw.len(), MAGIC.len() + 32);

        let k2 = RawKey::generate().expect("rng");
        write(&path, &k2, &OsAccountOnly).expect("rewrite");
        assert_eq!(read(&path, &OsAccountOnly).expect("read"), k2);
        // Aucun temporaire laissé derrière.
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names, vec![std::ffi::OsString::from("vault.key")]);
    }

    #[test]
    fn seal_is_applied() {
        let dir = TempDir::new("seal");
        let path = dir.0.join("vault.key");
        let k = RawKey::from_slice(&[7; 32]).expect("32");
        write(&path, &k, &Xor).expect("write");
        let raw = std::fs::read(&path).expect("raw");
        assert!(!raw.windows(32).any(|w| w == [7; 32]), "clé en clair");
        assert_eq!(read(&path, &Xor).expect("read"), k);
        // Autre « compte » : le descellement échoue proprement.
        assert!(read(&path, &OsAccountOnly).is_err());
    }

    #[test]
    fn rejects_foreign_or_truncated() {
        let dir = TempDir::new("bad");
        std::fs::create_dir_all(&dir.0).unwrap();
        let path = dir.0.join("vault.key");
        assert!(read(&path, &OsAccountOnly).is_err(), "absent");
        std::fs::write(&path, [0u8; 40]).unwrap();
        assert!(read(&path, &OsAccountOnly).is_err(), "sans en-tête");
        let mut short = MAGIC.to_vec();
        short.extend_from_slice(&[1; 31]);
        std::fs::write(&path, &short).unwrap();
        assert!(read(&path, &OsAccountOnly).is_err(), "tronqué");
    }

    #[test]
    fn stale_temp_is_overwritten() {
        let dir = TempDir::new("stale");
        std::fs::create_dir_all(&dir.0).unwrap();
        let path = dir.0.join("vault.key");
        std::fs::write(temp_path(&path), b"garbage").unwrap();
        let k = RawKey::generate().expect("rng");
        write(&path, &k, &OsAccountOnly).expect("write");
        assert_eq!(read(&path, &OsAccountOnly).expect("read"), k);
        assert!(!temp_path(&path).exists());
    }

    #[test]
    fn remove_is_idempotent() {
        let dir = TempDir::new("rm");
        let path = dir.0.join("vault.key");
        write(&path, &RawKey::generate().unwrap(), &OsAccountOnly).unwrap();
        remove(&path).expect("rm");
        assert!(!path.exists());
        remove(&path).expect("rm again");
    }

    #[cfg(unix)]
    #[test]
    fn unix_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new("perm");
        let path = dir.0.join("nyx").join("vault.key");
        write(&path, &RawKey::generate().unwrap(), &OsAccountOnly).unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
    }

    #[test]
    fn keyfile_next_to_db() {
        assert_eq!(path_for(Path::new("/a/nyx/vault.db")), PathBuf::from("/a/nyx/vault.key"));
    }
}
