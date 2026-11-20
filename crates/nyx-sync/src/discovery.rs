//! Découverte LAN par mDNS/DNS-SD (feature `mdns`).
//!
//! Service `_nyx-sync._tcp`. Rien d'identifiant en clair : le nom
//! d'instance et le TXT ne portent que
//! * `t` = étiquette de profil (HMAC du secret de profil, 64 bits) ;
//! * `d` = indice d'appareil (HMAC du secret + identifiant, 48 bits).
//!
//! Un observateur du réseau voit qu'« un appareil Nyx » est là et peut
//! relier ses annonces dans le temps (jusqu'à la prochaine rotation du
//! secret), mais n'apprend ni nom, ni clé, ni identifiant d'appareil.
//! Repli sans mDNS : [`crate::sync_with`] avec une IP saisie à la main.

use anyhow::{anyhow, Result};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use crate::profile::Device;
use crate::{SyncEngine, SyncSummary};

pub const SERVICE_TYPE: &str = "_nyx-sync._tcp.local.";

fn mdns(e: mdns_sd::Error) -> anyhow::Error {
    anyhow!("mDNS : {e}")
}

/// Annonce de cet appareil ; retirée au `drop`.
pub struct Advertiser {
    daemon: ServiceDaemon,
    fullname: String,
}

impl Advertiser {
    /// Annonce le service de synchronisation écoutant sur `port`.
    pub fn start(engine: &SyncEngine, port: u16) -> Result<Self> {
        let profile = engine.profile()?;
        let tag = profile.discovery_tag();
        let hint = profile.device_hint(engine.device_id());
        let daemon = ServiceDaemon::new().map_err(mdns)?;
        let instance = format!("nyx-{tag}-{hint}");
        let host = format!("nyx-{hint}.local.");
        let info = ServiceInfo::new(SERVICE_TYPE, &instance, &host, "", port, &[("t", tag.as_str()), ("d", hint.as_str())][..])
            .map_err(mdns)?
            .enable_addr_auto();
        let fullname = info.get_fullname().to_string();
        daemon.register(info).map_err(mdns)?;
        Ok(Self { daemon, fullname })
    }
}

impl Drop for Advertiser {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}

/// Un appareil du profil trouvé sur le réseau.
#[derive(Debug, Clone)]
pub struct Found {
    pub device: Device,
    pub addrs: Vec<SocketAddr>,
}

/// Cherche pendant `timeout` les autres appareils actifs du profil.
/// Les étiquettes de l'époque précédente sont acceptées (appareil qui n'a
/// pas encore appris une rotation : on pourra l'initier et le mettre à jour).
pub fn browse(engine: &SyncEngine, timeout: Duration) -> Result<Vec<Found>> {
    let profile = engine.profile()?;
    let tags = profile.accepted_tags();
    let peers: Vec<(Vec<String>, Device)> = profile
        .active()
        .filter(|d| d.device_id != engine.device_id())
        .map(|d| (profile.accepted_hints(&d.device_id), d.clone()))
        .collect();
    let daemon = ServiceDaemon::new().map_err(mdns)?;
    let rx = daemon.browse(SERVICE_TYPE).map_err(mdns)?;
    let deadline = Instant::now() + timeout;
    let mut found: Vec<Found> = Vec::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let Ok(ev) = rx.recv_timeout(left) else { break };
        let ServiceEvent::ServiceResolved(svc) = ev else { continue };
        let tag_ok = svc.get_property_val_str("t").is_some_and(|t| tags.iter().any(|x| x == t));
        let Some(hint) = svc.get_property_val_str("d").filter(|_| tag_ok) else { continue };
        let Some((_, dev)) = peers.iter().find(|(h, _)| h.iter().any(|x| x == hint)) else { continue };
        let addrs: Vec<SocketAddr> =
            svc.addresses.iter().map(|ip| SocketAddr::new(ip.to_ip_addr(), svc.port)).collect();
        match found.iter_mut().find(|f| f.device.device_id == dev.device_id) {
            Some(f) => {
                for a in addrs {
                    if !f.addrs.contains(&a) {
                        f.addrs.push(a);
                    }
                }
            }
            None => found.push(Found { device: dev.clone(), addrs }),
        }
    }
    let _ = daemon.stop_browse(SERVICE_TYPE);
    let _ = daemon.shutdown();
    Ok(found)
}

/// « Synchroniser maintenant » : cherche les appareils du profil pendant
/// `timeout` puis se synchronise avec chacun (première adresse qui
/// répond). Sans autre appareil actif dans le profil, ne touche pas au
/// réseau.
pub fn sync_all(engine: &SyncEngine, timeout: Duration) -> Result<SyncSummary> {
    let known = engine.profile()?.active().filter(|d| d.device_id != engine.device_id()).count();
    let mut summary = SyncSummary { known, ..SyncSummary::default() };
    if known == 0 {
        return Ok(summary);
    }
    let found = browse(engine, timeout)?;
    summary.found = found.len();
    for f in found {
        let mut last: Option<anyhow::Error> = None;
        let mut done = false;
        for addr in &f.addrs {
            match crate::sync_with(engine, *addr, Some(&f.device.device_id)) {
                Ok(r) => {
                    summary.reports.push(r);
                    done = true;
                    break;
                }
                Err(e) => last = Some(e),
            }
        }
        if !done {
            let why = last.map_or_else(|| "aucune adresse".to_string(), |e| format!("{e:#}"));
            summary.failures.push((f.device.name.clone(), why));
        }
    }
    Ok(summary)
}
