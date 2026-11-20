//! Appareils liés côté application : démarrage/arrêt du service de
//! synchronisation (`sync_service`, thread dédié avec sa propre connexion
//! au coffre), panneaux du chrome (QR + code à 6 chiffres, confirmation de
//! retrait), actions `nyx://devices/…` de la page Réglages, et relecture de
//! l'état mémoire après une synchronisation qui a reçu des données.
//!
//! Le thread UI n'attend jamais le réseau : il envoie des `Cmd` et reçoit
//! des `Event` par la boucle d'événements (`UserEvent::Sync`).

use std::time::Duration;

use super::{App, UserEvent};
use crate::devices::DeviceOp;
use crate::ipc::{Overlay, PairStage};
use crate::nav;
use crate::sync_service::{Cmd, Config, Event, Service};
use crate::{adblock, store};

/// Attente maximale de l'arrêt du service avant un changement de clé du
/// coffre (une session en cours est bornée par ses délais d'E/S).
const STOP_BEFORE_REKEY: Duration = Duration::from_secs(10);

impl App {
    /// Démarre le service avec la clé du coffre ouvert (mémoire seulement).
    pub(super) fn start_sync(&mut self) {
        self.stop_sync(Duration::from_secs(2));
        let Some(key) = self.vault_key.clone() else { return };
        let proxy = self.proxy.clone();
        let cfg = Config::new(self.vault_path.clone(), key);
        match Service::start(cfg, move |e| {
            let _ = proxy.send_event(UserEvent::Sync(e));
        }) {
            Ok(s) => self.sync = Some(s),
            Err(e) => self.devices.error = Some(format!("Synchronisation indisponible : {e}")),
        }
    }

    /// Arrête le service (attente bornée, puis thread détaché).
    pub(super) fn stop_sync(&mut self, wait: Duration) {
        if let Some(s) = self.sync.take() {
            s.stop(wait);
        }
    }

    /// Avant un changement de clé : le thread de synchronisation doit avoir
    /// fermé sa connexion au coffre.
    pub(super) fn stop_sync_for_rekey(&mut self) {
        self.stop_sync(STOP_BEFORE_REKEY);
    }

    fn send_sync(&self, cmd: Cmd) -> bool {
        match &self.sync {
            Some(s) => {
                s.send(cmd);
                true
            }
            None => false,
        }
    }

    // ── Événements du service ───────────────────────────────────────────

    pub(super) fn on_sync_event(&mut self, event: Event) {
        let was_syncing = self.devices.syncing;
        let view_changed = self.devices.apply(&event);
        match event {
            Event::PairOffer { offer: text, qr_svg, expires_at_ms } => {
                if let Some(Overlay::Pairing { stage, qr, offer, expires_at, .. }) = &mut self.overlay {
                    *stage = PairStage::Waiting;
                    *qr = qr_svg;
                    *offer = text;
                    *expires_at = expires_at_ms;
                }
                self.push_state();
            }
            Event::PairCode { code: c, device: d } => {
                match &mut self.overlay {
                    Some(Overlay::Pairing { stage, code, device, .. }) => {
                        *stage = PairStage::Confirm;
                        *code = c;
                        *device = d;
                    }
                    // Panneau fermé entre-temps : refus.
                    _ => {
                        self.send_sync(Cmd::PairAnswer(false));
                    }
                }
                self.push_state();
            }
            Event::PairDone(result) => {
                if let Some(Overlay::Pairing { stage, device, error, qr, offer, .. }) = &mut self.overlay {
                    *stage = PairStage::Done;
                    qr.clear();
                    offer.clear();
                    match result {
                        Ok(name) => *device = name,
                        Err(e) => *error = e,
                    }
                    self.push_state();
                } else if let Ok(name) = result {
                    self.toast(&format!("« {name} » a rejoint vos appareils liés"));
                }
            }
            Event::Synced { last, changed } => {
                if changed {
                    self.reload_after_sync();
                }
                if was_syncing {
                    self.toast(&last.message);
                }
            }
            Event::Notice(text) => self.toast(&text),
            Event::Failed(text) => {
                if was_syncing {
                    self.toast(&text);
                }
            }
            Event::Listening { .. } | Event::Devices(_) | Event::LastSync(_) => {}
        }
        if view_changed {
            self.reload_pages(&["nyx://settings"]);
        }
    }

    /// Données reçues : favoris et réglages relus depuis le coffre (sinon
    /// une écriture suivante, faite depuis l'état mémoire, les écraserait),
    /// pages internes redessinées. Mots de passe et historique sont relus à
    /// chaque usage.
    fn reload_after_sync(&mut self) {
        if let Some(vault) = &self.vault {
            store::load_settings(&self.settings, vault, adblock::guard());
            store::load_bookmarks(&self.bookmarks, vault);
        }
        self.reload_pages(&["nyx://settings", "nyx://bookmarks", "nyx://history"]);
        self.last_state = None;
        self.push_state();
    }

    /// Recharge les onglets affichant une de ces pages internes.
    pub(super) fn reload_pages(&self, prefixes: &[&str]) {
        for tab in self.tabs.iter() {
            let url = nav::to_policy_url(&tab.url);
            if prefixes.iter().any(|p| url.starts_with(p)) {
                let _ = tab.view.reload();
            }
        }
    }

    // ── Actions de la page Réglages ─────────────────────────────────────

    pub(super) fn on_devices_action(&mut self, url: &str) {
        let Some(op) = DeviceOp::parse(url) else { return };
        if self.sync.is_none() {
            let why = self.devices.error.clone().unwrap_or_else(|| "service arrêté".into());
            return self.toast(&format!("Synchronisation indisponible : {why}"));
        }
        match op {
            DeviceOp::Pair => {
                self.set_overlay(Some(Overlay::pairing()));
                self.send_sync(Cmd::Pair);
            }
            DeviceOp::Sync => {
                self.devices.syncing = true;
                self.send_sync(Cmd::SyncNow);
                self.toast("Recherche de vos appareils sur le réseau local…");
                self.reload_pages(&["nyx://settings"]);
            }
            DeviceOp::Rename(name) => {
                self.send_sync(Cmd::Rename(name));
            }
            DeviceOp::Revoke(id) => {
                if let Some(name) = self.devices.active_name(&id).map(str::to_string) {
                    self.set_overlay(Some(Overlay::Revoke { id, name, error: String::new() }));
                }
            }
        }
    }

    // ── Panneaux du chrome ──────────────────────────────────────────────

    /// « Les codes correspondent » / « Annuler » (étape de confirmation).
    pub(super) fn on_pair_answer(&mut self, yes: bool) {
        if !matches!(self.overlay, Some(Overlay::Pairing { stage: PairStage::Confirm, .. })) {
            return;
        }
        self.send_sync(Cmd::PairAnswer(yes));
        if yes {
            // Finalisation : le service répondra par `PairDone`.
            if let Some(Overlay::Pairing { stage, .. }) = &mut self.overlay {
                *stage = PairStage::Starting;
            }
            self.push_state();
        } else {
            self.set_overlay(None);
            self.toast("Appairage annulé");
        }
    }

    pub(super) fn on_confirm_revoke(&mut self, id: String) {
        if !matches!(&self.overlay, Some(Overlay::Revoke { id: shown, .. }) if *shown == id) {
            return;
        }
        self.send_sync(Cmd::Revoke(id));
        self.set_overlay(None);
    }

    /// Fermeture d'un panneau d'appareils : l'appairage en cours est annulé.
    pub(super) fn close_devices_overlay(&mut self) {
        if let Some(Overlay::Pairing { stage, .. }) = &self.overlay {
            if *stage != PairStage::Done {
                self.send_sync(Cmd::PairCancel);
            }
        }
        self.set_overlay(None);
    }
}
