//! Session de synchronisation entre deux appareils appairés.
//!
//! Canal : TCP + Noise `KK_25519_ChaChaPoly_BLAKE2s` (les deux clés
//! statiques sont connues d'avance). Le premier message ne porte AUCUN
//! identifiant en clair : le répondeur essaie chacune des clés des appareils
//! actifs de son profil (≤ 32) jusqu'à ce que le message se déchiffre.
//!
//! Protocole (semi-duplex, l'initiateur mène) :
//!
//! ```text
//! I → R  Hello { appareil, profil, filigranes que I a pour R }
//! R → I  Hello { … }
//! I → R  Batch { table, enregistrements, upto }*  Done
//! R → I  Batch*  Done
//! ```
//!
//! Chaque `Batch` est appliqué dans une transaction qui avance aussi le
//! filigrane (`upto` = `local_hlc` de l'émetteur) : une session coupée
//! reprend au dernier lot validé, et rejouer un lot ne change rien.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::{SocketAddr, TcpListener, TcpStream};
use vault::{ApplyStats, Change, Record, Table};

use crate::profile::{Device, Profile};
use crate::wire::{self, hs_read_frame, hs_write, noise, read_frame, Channel};
use crate::{SyncEngine, SyncError};

pub(crate) const PARAMS: &str = "Noise_KK_25519_ChaChaPoly_BLAKE2s";
const PROLOGUE: &[u8] = b"nyx-sync/v1";
/// Enregistrements lus par requête au coffre.
pub const BATCH_RECORDS: usize = 256;
/// Taille visée d'un lot sérialisé.
pub const BATCH_BYTES: usize = 512 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(tag = "m", rename_all = "snake_case")]
enum Msg {
    Hello { device_id: String, profile: Profile, watermarks: BTreeMap<Table, u64> },
    Batch { table: Table, records: Vec<Record>, upto: u64 },
    Done,
}

/// Bilan d'une session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncReport {
    pub peer: Device,
    /// Enregistrements envoyés.
    pub sent: usize,
    /// Bilan de l'application des enregistrements reçus.
    pub received: ApplyStats,
}

fn builder<'a>(engine: &'a SyncEngine, remote: &'a [u8; 32]) -> Result<snow::Builder<'a>> {
    snow::Builder::new(PARAMS.parse().map_err(noise)?)
        .local_private_key(engine.keys().private())
        .map_err(noise)?
        .remote_public_key(remote)
        .map_err(noise)?
        .prologue(PROLOGUE)
        .map_err(noise)
}

fn peers(engine: &SyncEngine) -> Result<Vec<Device>> {
    let me = engine.device_id();
    Ok(engine.profile()?.active().filter(|d| d.device_id != me).cloned().collect())
}

/// Se synchronise avec l'appareil joignable à `addr`. `peer` = identifiant
/// attendu (découverte mDNS) ; `None` (IP saisie à la main) essaie tour à
/// tour chaque appareil actif du profil.
pub fn sync_with(engine: &SyncEngine, addr: SocketAddr, peer: Option<&str>) -> Result<SyncReport> {
    let candidates: Vec<Device> =
        peers(engine)?.into_iter().filter(|d| peer.is_none_or(|p| p == d.device_id)).collect();
    if candidates.is_empty() {
        bail!(SyncError::UnknownPeer);
    }
    let mut last: anyhow::Error = SyncError::UnknownPeer.into();
    for dev in candidates {
        match initiate(engine, addr, &dev) {
            Ok(ch) => return run(engine, ch, dev, true),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn initiate(engine: &SyncEngine, addr: SocketAddr, dev: &Device) -> Result<Channel> {
    let mut stream = wire::connect(addr, engine.io_timeout())?;
    let mut hs = builder(engine, &dev.public_key)?.build_initiator().map_err(noise)?;
    hs_write(&mut stream, &mut hs)?;
    // Le répondeur coupe s'il ne nous reconnaît pas (ou n'est pas `dev`).
    let frame = read_frame(&mut stream).map_err(|_| SyncError::HandshakeFailed)?;
    hs_read_frame(&mut hs, &frame)?;
    Channel::new(stream, hs)
}

/// Accepte UNE connexion sur `listener` et la synchronise.
pub fn serve_one(engine: &SyncEngine, listener: &TcpListener) -> Result<SyncReport> {
    let (stream, _) = listener.accept()?;
    serve_stream(engine, stream)
}

/// Répond à une connexion entrante déjà acceptée.
pub fn serve_stream(engine: &SyncEngine, mut stream: TcpStream) -> Result<SyncReport> {
    wire::prepare(&stream, engine.io_timeout())?;
    let first = read_frame(&mut stream)?;
    for dev in peers(engine)? {
        let mut hs = builder(engine, &dev.public_key)?.build_responder().map_err(noise)?;
        if hs_read_frame(&mut hs, &first).is_err() {
            continue; // pas cet appareil-là
        }
        hs_write(&mut stream, &mut hs)?;
        let ch = Channel::new(stream, hs)?;
        return run(engine, ch, dev, false);
    }
    bail!(SyncError::UnknownPeer)
}

fn run(engine: &SyncEngine, mut ch: Channel, peer: Device, initiator: bool) -> Result<SyncReport> {
    if ch.remote != peer.public_key {
        bail!(SyncError::UnknownPeer);
    }
    let vault = engine.vault();
    let mut wm = BTreeMap::new();
    for t in Table::ALL {
        wm.insert(t, vault.sync_watermark(&peer.device_id, t)?);
    }
    let hello = Msg::Hello { device_id: engine.device_id().to_string(), profile: engine.profile()?, watermarks: wm };
    let theirs = if initiator {
        ch.send_json(&hello)?;
        ch.recv_json::<Msg>()?
    } else {
        let m = ch.recv_json::<Msg>()?;
        ch.send_json(&hello)?;
        m
    };
    let Msg::Hello { device_id, profile, watermarks } = theirs else { bail!(SyncError::Protocol("Hello attendu")) };
    if device_id != peer.device_id {
        bail!(SyncError::Protocol("identité annoncée ≠ clé authentifiée"));
    }
    // Profils : union des appareils, révocations, rotation la plus récente.
    profile.validate()?;
    let mut mine = engine.profile()?;
    if mine.merge(&profile) {
        engine.save_profile(&mine)?;
    }
    if mine.device(&peer.device_id).is_none_or(|d| d.revoked) {
        bail!(SyncError::UnknownPeer);
    }

    let (sent, received) = if initiator {
        let s = send_changes(engine, &mut ch, &watermarks)?;
        (s, recv_changes(engine, &mut ch, &peer)?)
    } else {
        let r = recv_changes(engine, &mut ch, &peer)?;
        (send_changes(engine, &mut ch, &watermarks)?, r)
    };
    Ok(SyncReport { peer, sent, received })
}

/// Découpe `changes` en lots ≤ [`BATCH_BYTES`]. Le `upto` d'un lot ne
/// coupe jamais un groupe de `local_hlc` égales (sinon une reprise
/// sauterait la fin du groupe) : il recule alors d'un cran, et le pair
/// recevra une seconde fois ces lignes, sans effet.
fn split_batches(changes: Vec<Change>) -> Result<Vec<(Vec<Record>, u64)>> {
    let mut out = Vec::new();
    let mut cur: Vec<Record> = Vec::new();
    let mut bytes = 0usize;
    let hlcs: Vec<u64> = changes.iter().map(|c| c.local_hlc).collect();
    for (i, c) in changes.into_iter().enumerate() {
        bytes += serde_json::to_vec(&c.record)?.len();
        cur.push(c.record);
        let last = i + 1 == hlcs.len();
        if bytes >= BATCH_BYTES || last {
            let upto = if !last && hlcs[i + 1] == hlcs[i] { hlcs[i].saturating_sub(1) } else { hlcs[i] };
            out.push((std::mem::take(&mut cur), upto));
            bytes = 0;
        }
    }
    Ok(out)
}

fn send_changes(engine: &SyncEngine, ch: &mut Channel, their_wm: &BTreeMap<Table, u64>) -> Result<usize> {
    let mut sent = 0;
    for t in Table::ALL {
        let mut after = their_wm.get(&t).copied().unwrap_or(0);
        loop {
            let changes = engine.vault().sync_changes(t, after, BATCH_RECORDS)?;
            let Some(next) = changes.last().map(|c| c.local_hlc) else { break };
            for (records, upto) in split_batches(changes)? {
                sent += records.len();
                ch.send_json(&Msg::Batch { table: t, records, upto })?;
            }
            after = next;
        }
    }
    ch.send_json(&Msg::Done)?;
    Ok(sent)
}

fn recv_changes(engine: &SyncEngine, ch: &mut Channel, peer: &Device) -> Result<ApplyStats> {
    let mut total = ApplyStats::default();
    loop {
        match ch.recv_json::<Msg>()? {
            Msg::Batch { table, records, upto } => {
                let (ok, bad): (Vec<Record>, Vec<Record>) = records.into_iter().partition(|r| r.table() == table);
                let s = engine.vault().sync_apply(&ok, Some((&peer.device_id, table, upto)))?;
                total.applied += s.applied;
                total.skipped += s.skipped;
                total.rejected += s.rejected + bad.len();
            }
            Msg::Done => return Ok(total),
            Msg::Hello { .. } => bail!(SyncError::Protocol("Hello inattendu")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault::RecordData;

    fn change(h: u64, size: usize) -> Change {
        Change {
            local_hlc: h,
            record: Record {
                uuid: format!("{h:032x}"),
                updated_at: h,
                deleted: false,
                origin_device: "d".into(),
                data: RecordData::Setting { key: "k".into(), value: "x".repeat(size) },
            },
        }
    }

    #[test]
    fn lots_bornes_et_upto_sur() {
        let big = BATCH_BYTES / 2 + 1;
        let b = split_batches(vec![change(1, big), change(2, big), change(3, 10)]).unwrap();
        assert_eq!(b.len(), 2);
        assert_eq!((b[0].0.len(), b[0].1), (2, 2));
        assert_eq!((b[1].0.len(), b[1].1), (1, 3));
        // Coupure au milieu d'un groupe d'égales : upto recule.
        let b = split_batches(vec![change(5, big), change(5, big), change(5, 10)]).unwrap();
        assert_eq!(b[0].1, 4);
        assert_eq!(b[1].1, 5);
        assert!(split_batches(vec![]).unwrap().is_empty());
    }
}
