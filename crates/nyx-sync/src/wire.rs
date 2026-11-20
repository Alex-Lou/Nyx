//! Cadrage TCP et canal Noise.
//!
//! * Trame réseau : longueur `u16` gros-boutiste + octets (≤ 65 535, la
//!   taille maximale d'un message Noise) — format recommandé par la spec
//!   Noise §13.
//! * Message applicatif : `u32` (longueur totale) + contenu, découpé en
//!   morceaux chiffrés chacun dans un message Noise (≤ 65 519 octets clairs).
//!   Taille totale bornée par [`MAX_MSG`].
//! * Délais : lecture/écriture bornées par le `io_timeout` du moteur.

use anyhow::{anyhow, bail, Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use snow::{HandshakeState, TransportState};
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use crate::SyncError;

/// Taille maximale d'un message Noise sur le fil (spec Noise).
pub const MAX_FRAME: usize = 65_535;
const TAG_LEN: usize = 16;
const MAX_PLAIN: usize = MAX_FRAME - TAG_LEN;
/// Taille maximale d'un message applicatif (après réassemblage).
pub const MAX_MSG: usize = 16 << 20;

pub(crate) fn noise(e: snow::Error) -> anyhow::Error {
    anyhow!("Noise : {e}")
}

pub(crate) fn write_frame(s: &mut impl Write, bytes: &[u8]) -> Result<()> {
    let len = u16::try_from(bytes.len()).map_err(|_| anyhow!("trame trop grande"))?;
    s.write_all(&len.to_be_bytes())?;
    s.write_all(bytes)?;
    s.flush()?;
    Ok(())
}

pub(crate) fn read_frame(s: &mut impl Read) -> Result<Vec<u8>> {
    let mut len = [0u8; 2];
    s.read_exact(&mut len).context("connexion fermée par le pair")?;
    let mut buf = vec![0u8; u16::from_be_bytes(len) as usize];
    s.read_exact(&mut buf).context("trame tronquée")?;
    Ok(buf)
}

/// Connexion TCP sortante avec délais.
pub(crate) fn connect(addr: SocketAddr, timeout: Duration) -> Result<TcpStream> {
    let s = TcpStream::connect_timeout(&addr, timeout.min(Duration::from_secs(5)))
        .with_context(|| format!("connexion à {addr}"))?;
    prepare(&s, timeout)?;
    Ok(s)
}

pub(crate) fn prepare(s: &TcpStream, timeout: Duration) -> Result<()> {
    s.set_nonblocking(false)?;
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(timeout))?;
    s.set_nodelay(true)?;
    Ok(())
}

/// Écrit le prochain message de handshake (charge utile vide).
pub(crate) fn hs_write(s: &mut TcpStream, hs: &mut HandshakeState) -> Result<()> {
    let mut buf = vec![0u8; MAX_FRAME];
    let n = hs.write_message(&[], &mut buf).map_err(noise)?;
    write_frame(s, &buf[..n])
}

/// Lit et traite le prochain message de handshake.
pub(crate) fn hs_read(s: &mut TcpStream, hs: &mut HandshakeState) -> Result<()> {
    let frame = read_frame(s)?;
    hs_read_frame(hs, &frame)
}

pub(crate) fn hs_read_frame(hs: &mut HandshakeState, frame: &[u8]) -> Result<()> {
    let mut buf = vec![0u8; MAX_FRAME];
    hs.read_message(frame, &mut buf).map_err(|_| SyncError::HandshakeFailed)?;
    Ok(())
}

/// Code de confirmation à 6 chiffres dérivé du hash de handshake : identique
/// des deux côtés si et seulement si personne ne s'est interposé.
pub fn confirmation_code(handshake_hash: &[u8]) -> String {
    let mut b = [0u8; 8];
    b.copy_from_slice(&handshake_hash[..8]);
    format!("{:06}", u64::from_be_bytes(b) % 1_000_000)
}

/// Canal chiffré établi (après handshake).
pub(crate) struct Channel {
    stream: TcpStream,
    noise: TransportState,
    /// Hash du handshake (liaison de canal, code de confirmation).
    pub hash: Vec<u8>,
    /// Clé statique du pair, authentifiée par le handshake.
    pub remote: [u8; 32],
}

impl Channel {
    pub fn new(stream: TcpStream, hs: HandshakeState) -> Result<Self> {
        let hash = hs.get_handshake_hash().to_vec();
        let remote: [u8; 32] = hs
            .get_remote_static()
            .and_then(|k| k.try_into().ok())
            .ok_or(SyncError::Protocol("clé statique du pair absente"))?;
        let noise = hs.into_transport_mode().map_err(noise)?;
        Ok(Self { stream, noise, hash, remote })
    }

    pub fn send(&mut self, msg: &[u8]) -> Result<()> {
        if msg.len() > MAX_MSG {
            bail!(SyncError::Protocol("message trop grand"));
        }
        let mut plain = Vec::with_capacity(msg.len() + 4);
        plain.extend_from_slice(&(msg.len() as u32).to_be_bytes());
        plain.extend_from_slice(msg);
        let mut out = vec![0u8; MAX_FRAME];
        for chunk in plain.chunks(MAX_PLAIN) {
            let n = self.noise.write_message(chunk, &mut out).map_err(noise)?;
            write_frame(&mut self.stream, &out[..n])?;
        }
        Ok(())
    }

    pub fn recv(&mut self) -> Result<Vec<u8>> {
        let mut plain = vec![0u8; MAX_FRAME];
        let mut read_chunk = |this: &mut Self| -> Result<Vec<u8>> {
            let frame = read_frame(&mut this.stream)?;
            let n = this
                .noise
                .read_message(&frame, &mut plain)
                .map_err(|_| SyncError::Protocol("message altéré ou rejoué"))?;
            Ok(plain[..n].to_vec())
        };
        let first = read_chunk(self)?;
        if first.len() < 4 {
            bail!(SyncError::Protocol("en-tête de message tronqué"));
        }
        let total = u32::from_be_bytes([first[0], first[1], first[2], first[3]]) as usize;
        if total > MAX_MSG {
            bail!(SyncError::Protocol("message trop grand"));
        }
        let mut msg = first[4..].to_vec();
        while msg.len() < total {
            msg.extend_from_slice(&read_chunk(self)?);
        }
        if msg.len() != total {
            bail!(SyncError::Protocol("longueur de message incohérente"));
        }
        Ok(msg)
    }

    pub fn send_json<T: Serialize>(&mut self, v: &T) -> Result<()> {
        self.send(&serde_json::to_vec(v)?)
    }

    pub fn recv_json<T: DeserializeOwned>(&mut self) -> Result<T> {
        let bytes = self.recv()?;
        serde_json::from_slice(&bytes).map_err(|_| SyncError::Protocol("message illisible").into())
    }
}

/// `true` si l'erreur est un simple délai dépassé / connexion coupée.
pub(crate) fn is_io(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        c.downcast_ref::<std::io::Error>()
            .is_some_and(|io| {
                matches!(
                    io.kind(),
                    ErrorKind::WouldBlock
                        | ErrorKind::TimedOut
                        | ErrorKind::UnexpectedEof
                        | ErrorKind::ConnectionReset
                        | ErrorKind::ConnectionAborted
                        | ErrorKind::BrokenPipe
                )
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trames_aller_retour() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"abc").unwrap();
        write_frame(&mut buf, &[]).unwrap();
        let mut r = &buf[..];
        assert_eq!(read_frame(&mut r).unwrap(), b"abc");
        assert_eq!(read_frame(&mut r).unwrap(), b"");
        assert!(read_frame(&mut r).is_err());
        assert!(write_frame(&mut Vec::new(), &vec![0u8; MAX_FRAME + 1]).is_err());
        // Trame tronquée.
        let mut r: &[u8] = &[0, 5, 1, 2];
        assert!(read_frame(&mut r).is_err());
    }

    #[test]
    fn code_de_confirmation() {
        let h = [0u8; 32];
        assert_eq!(confirmation_code(&h), "000000");
        let mut h2 = [0u8; 32];
        h2[7] = 1;
        assert_eq!(confirmation_code(&h2), "000001");
        let h3 = [0xFFu8; 32];
        assert_eq!(confirmation_code(&h3).len(), 6);
    }
}
