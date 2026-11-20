//! Coffre côté application : démarrage (clé d'appareil DPAPI ou mot de
//! passe principal), création, mot de passe principal (réglages), import de
//! la capsule, capture + pré-remplissage des mots de passe, historique,
//! favoris, pages internes et leurs actions. Toute décision vient des
//! modules purs (`vault::keymode`, `capture`, `autofill`, `history`,
//! `migrate`, `store`, `import_chrome`, `pages`) et de `nyx-core`.

use std::path::Path;

use nyx_core::security::Page;
use nyx_core::state::bookmarks as bm;
use vault::keymode::{KeyError, KeyStore, Startup};
use vault::{keyfile, Key, Vault};

use super::{App, Pending, Responder, UserEvent};
use crate::dpapi::Dpapi;
use crate::ipc::{CapsuleView, MasterMode, Overlay};
use crate::migrate::{self, CapsuleCopy};
use crate::nav::{self, Action};
use crate::paths::{self, Capsule};
use crate::protocol::{self, Asset};
use crate::secret::{self, Secret};
use crate::tabs::TabId;
use crate::{adblock, autofill, capture, import_chrome, pages, store, webview2};

/// Fichier de clé présent mais inutilisable : on n'efface rien, on demande.
const DEVICE_KEY_FAILED: &str = "La clé de cet appareil n'ouvre pas le coffre (fichier corrompu ou créé \
par un autre compte Windows). Saisis ton mot de passe principal.";
const CAPSULE_PASSPHRASE_NEEDED: &str = "Saisissez la passphrase de la capsule";

fn capsule_view(status: Option<&Capsule>) -> CapsuleView {
    match status {
        None => CapsuleView::Probing,
        Some(Capsule::Found(_)) => CapsuleView::Found,
        Some(Capsule::Missing) => CapsuleView::Missing,
        Some(Capsule::Unreachable) => CapsuleView::Unreachable,
    }
}

impl App {
    // ── Panneaux plein écran ────────────────────────────────────────────

    pub(super) fn set_overlay(&mut self, overlay: Option<Overlay>) {
        self.overlay = overlay;
        self.layout();
        self.push_state();
        if self.overlay.is_none() {
            if let Some(view) = self.active_view() {
                let _ = view.focus();
            }
        }
    }

    /// Message d'erreur du panneau courant.
    fn overlay_error(&mut self, message: &str) {
        let mut overlay = self
            .overlay
            .take()
            .unwrap_or(Overlay::Capsule { capsule: capsule_view(self.capsule.as_ref()), error: String::new() });
        overlay.set_error(message);
        // Toujours renvoyer l'état : le chrome a vidé les champs et attend
        // une réponse, même pour une erreur identique à la précédente.
        self.last_state = None;
        self.set_overlay(Some(overlay));
    }

    // ── Capsule WSL ─────────────────────────────────────────────────────

    /// Sonde la capsule hors du thread UI (`\\wsl.localhost` peut être lent).
    pub(super) fn probe_capsule(&self) {
        let proxy = self.proxy.clone();
        let spawned = std::thread::Builder::new().name("nyx-capsule".into()).spawn(move || {
            let _ = proxy.send_event(UserEvent::Capsule(paths::probe_capsule(Path::exists)));
        });
        if spawned.is_err() {
            let _ = self.proxy.send_event(UserEvent::Capsule(Capsule::Unreachable));
        }
    }

    pub(super) fn probe_capsule_again(&mut self) {
        self.capsule = None;
        self.refresh_overlay_capsule();
        self.probe_capsule();
    }

    pub(super) fn on_capsule_probed(&mut self, status: Capsule) {
        self.capsule = Some(status);
        self.refresh_overlay_capsule();
    }

    fn refresh_overlay_capsule(&mut self) {
        let view = capsule_view(self.capsule.as_ref());
        let Some(overlay) = &mut self.overlay else { return };
        overlay.set_capsule(view);
        self.push_state();
    }

    fn capsule_path(&self) -> Option<&Path> {
        match &self.capsule {
            Some(Capsule::Found(p)) => Some(p.as_path()),
            _ => None,
        }
    }

    // ── Démarrage / déverrouillage / création ───────────────────────────

    /// `%APPDATA%\Nyx\vault.db` + `vault.key` scellé par DPAPI.
    fn key_store(&self) -> KeyStore<Dpapi> {
        KeyStore::new(self.vault_path.clone(), Dpapi)
    }

    /// Démarrage : clé d'appareil → ouverture silencieuse (repli sur le mot
    /// de passe principal si elle échoue, rien n'est supprimé) ; coffre seul
    /// → mot de passe principal ; rien → bienvenue.
    pub(super) fn start_vault(&mut self) {
        let ks = self.key_store();
        let overlay = match ks.startup() {
            Startup::FirstLaunch => {
                self.probe_capsule();
                Overlay::Welcome { capsule: CapsuleView::Probing, error: String::new() }
            }
            Startup::DeviceKey => match ks.open_with_device_key() {
                Ok(vault) => {
                    let key = self.device_key();
                    return self.finish_unlock(vault, key, None);
                }
                Err(_) => Overlay::Unlock { notice: DEVICE_KEY_FAILED.into(), error: String::new() },
            },
            Startup::MasterPassword => Overlay::Unlock { notice: String::new(), error: String::new() },
        };
        self.set_overlay(Some(overlay));
    }

    pub(super) fn on_unlock(&mut self, passphrase: Secret) {
        if self.vault.is_some() || !matches!(self.overlay, Some(Overlay::Unlock { .. })) {
            return;
        }
        let pass = passphrase.to_passphrase();
        match self.key_store().open_with_master(&pass) {
            Ok(vault) => self.finish_unlock(vault, Some(Key::Passphrase(pass)), None),
            Err(e) => self.overlay_error(&e.to_string()),
        }
    }

    /// Premier lancement : `master` faux = « Plus tard » (clé d'appareil).
    pub(super) fn on_create(
        &mut self,
        master: bool,
        passphrase: Secret,
        confirm: Secret,
        import: bool,
        capsule_pass: Secret,
    ) {
        if self.vault.is_some() || !matches!(self.overlay, Some(Overlay::Welcome { .. })) {
            return;
        }
        let ks = self.key_store();
        if ks.startup() != Startup::FirstLaunch {
            return;
        }
        if master {
            if let Err(msg) = secret::check_new_passphrase(&passphrase, &confirm) {
                return self.overlay_error(msg);
            }
        }
        drop(confirm);
        // Capsule d'abord : une mauvaise passphrase ne crée pas le coffre.
        let copy = if import {
            let Some(source) = self.capsule_path().map(Path::to_path_buf) else {
                return self.overlay_error(&format!(
                    "Capsule injoignable. Lancez « {} » puis Réessayer.",
                    paths::START_CAPSULE_CMD
                ));
            };
            let Some(key) = migrate::capsule_key(&source, capsule_pass.expose()) else {
                return self.overlay_error(CAPSULE_PASSPHRASE_NEEDED);
            };
            match CapsuleCopy::open(&source, &std::env::temp_dir(), &key) {
                Ok(copy) => Some(copy),
                Err(e) => return self.overlay_error(&e.message()),
            }
        } else {
            None
        };
        drop(capsule_pass);
        let pass = passphrase.to_passphrase();
        let created = if master { ks.create_with_master(&pass) } else { ks.create_with_device_key() };
        drop(passphrase);
        let vault = match created {
            Ok(v) => v,
            Err(e) => return self.overlay_error(&format!("Création du coffre impossible : {e}")),
        };
        let key = if master { Some(Key::Passphrase(pass)) } else { self.device_key() };
        let summary = copy.map(|c| migrate::merge(c.vault(), &vault).map(|s| s.text()));
        let note = summary.map(|r| r.unwrap_or_else(|e| e.message()));
        self.finish_unlock(vault, key, note);
    }

    /// Clé d'appareil (fichier scellé DPAPI), pour la connexion du thread de
    /// synchronisation. Mémoire seulement.
    fn device_key(&self) -> Option<Key> {
        keyfile::read(self.key_store().keyfile_path(), &Dpapi).ok().map(Key::Raw)
    }

    /// Coffre ouvert : charge réglages + favoris, ferme le panneau, ouvre
    /// l'accueil, démarre la synchronisation (si la clé est connue).
    fn finish_unlock(&mut self, vault: Vault, key: Option<Key>, note: Option<String>) {
        store::load_settings(&self.settings, &vault, adblock::guard());
        store::load_bookmarks(&self.bookmarks, &vault);
        self.vault = Some(vault);
        self.vault_key = key;
        self.start_sync();
        self.set_overlay(None);
        let home = nav::home_url(&self.settings.borrow());
        if self.open_tab(&home, true).is_err() {
            self.toast("Impossible d'ouvrir un onglet");
        }
        if let Some(note) = note {
            self.toast(&note);
        }
    }

    pub(super) fn on_import_capsule(&mut self, passphrase: Secret) {
        let (Some(vault), Some(source)) = (&self.vault, self.capsule_path()) else {
            return self.overlay_error(&format!(
                "Capsule injoignable. Lancez « {} » puis Réessayer.",
                paths::START_CAPSULE_CMD
            ));
        };
        let Some(key) = migrate::capsule_key(source, passphrase.expose()) else {
            return self.overlay_error(CAPSULE_PASSPHRASE_NEEDED);
        };
        drop(passphrase);
        match migrate::import_capsule(source, &std::env::temp_dir(), &key, vault) {
            Ok(summary) => {
                store::load_bookmarks(&self.bookmarks, vault);
                if summary.settings {
                    store::load_settings(&self.settings, vault, adblock::guard());
                }
                self.set_overlay(None);
                self.toast(&summary.text());
            }
            Err(e) => self.overlay_error(&e.message()),
        }
    }

    // ── Mot de passe principal (réglages) ───────────────────────────────
    // Saisie dans le chrome (origine `nyxui`) ; chaque commande n'est
    // acceptée que si le panneau correspondant est ouvert (ouvert lui-même
    // par l'action interne `nyx://master/…` d'une page interne).

    fn master_mode(&self) -> Option<MasterMode> {
        match &self.overlay {
            Some(Overlay::Master { mode, .. }) => Some(*mode),
            _ => None,
        }
    }

    pub(super) fn on_set_master(&mut self, passphrase: Secret, confirm: Secret) {
        if self.master_mode() != Some(MasterMode::Define) {
            return;
        }
        if let Err(msg) = secret::check_new_passphrase(&passphrase, &confirm) {
            return self.overlay_error(msg);
        }
        if self.vault.is_none() {
            return;
        }
        let new = passphrase.to_passphrase();
        self.stop_sync_for_rekey();
        let Some(vault) = &self.vault else { return };
        let result = self.key_store().set_master(vault, &new);
        self.master_done(result, Some(Key::Passphrase(new)), "Mot de passe principal défini");
    }

    pub(super) fn on_change_master(&mut self, old: Secret, passphrase: Secret, confirm: Secret) {
        if self.master_mode() != Some(MasterMode::Change) {
            return;
        }
        if let Err(msg) = secret::check_new_passphrase(&passphrase, &confirm) {
            return self.overlay_error(msg);
        }
        if self.vault.is_none() {
            return;
        }
        let new = passphrase.to_passphrase();
        self.stop_sync_for_rekey();
        let Some(vault) = &self.vault else { return };
        let result = self.key_store().change_master(vault, &old.to_passphrase(), &new);
        self.master_done(result, Some(Key::Passphrase(new)), "Mot de passe principal changé");
    }

    pub(super) fn on_remove_master(&mut self, passphrase: Secret) {
        if self.master_mode() != Some(MasterMode::Remove) {
            return;
        }
        if passphrase.is_empty() {
            return self.overlay_error("Saisis le mot de passe principal actuel");
        }
        if self.vault.is_none() {
            return;
        }
        self.stop_sync_for_rekey();
        let Some(vault) = &self.vault else { return };
        let result = self.key_store().remove_master(vault, &passphrase.to_passphrase());
        let new = if result.is_ok() { self.device_key() } else { None };
        self.master_done(result, new, "Mot de passe principal retiré : protection par la session Windows");
    }

    /// Fin d'un changement de clé : la synchronisation (arrêtée avant le
    /// rechiffrement) redémarre avec la nouvelle clé, ou l'ancienne si
    /// l'opération a échoué (état précédent restauré par `KeyStore`).
    fn master_done(&mut self, result: Result<(), KeyError>, new_key: Option<Key>, ok: &str) {
        if result.is_ok() {
            self.vault_key = new_key;
        }
        self.start_sync();
        match result {
            Ok(()) => {
                self.set_overlay(None);
                self.toast(ok);
                // Pages Réglages ouvertes : redessinées avec le nouvel état.
                self.reload_pages(&["nyx://settings"]);
            }
            Err(e) => self.overlay_error(&e.to_string()),
        }
    }

    // ── Mots de passe ───────────────────────────────────────────────────

    pub(super) fn on_credential(&mut self, host: String, username: String, password: Secret) {
        let Some(vault) = &self.vault else { return };
        if !self.settings.borrow().save_passwords {
            return;
        }
        let existing = vault.passwords_for(&host).unwrap_or_default();
        let plan = capture::plan(&existing, &username, password.expose());
        let Some(text) = capture::question(plan, &username, &host) else { return };
        let id = self.next_prompt;
        self.next_prompt = self.next_prompt.wrapping_add(1).max(1);
        self.prompt = Some(Pending { id, text, host, username, password, plan });
        self.layout();
        self.push_state();
    }

    pub(super) fn on_prompt_answer(&mut self, id: u32, yes: bool) {
        if self.prompt.as_ref().is_none_or(|p| p.id != id) {
            return;
        }
        let Some(p) = self.prompt.take() else { return };
        if let (true, Some(vault)) = (yes, &self.vault) {
            let saved = match p.plan {
                capture::Plan::Add => {
                    vault.add_password(&vault::Password::new(&p.host, &p.username, p.password.expose())).is_ok()
                }
                capture::Plan::Update { id } => vault.update_password(id, p.password.expose()).is_ok(),
                capture::Plan::Nothing => true,
            };
            if !saved {
                self.toast("Enregistrement du mot de passe impossible");
            }
        }
        self.layout();
        self.push_state();
    }

    // ── Chargement terminé : historique + pré-remplissage ───────────────

    pub(super) fn after_load(&mut self, id: TabId) {
        let Some(vault) = &self.vault else { return };
        let Some(tab) = self.tabs.get(id) else { return };
        let private = self.settings.borrow().private_mode;
        let url = tab.url.clone();
        if self.recorder.on_loaded(id, &url, private) {
            let _ = vault.push_history(&url, &webview2::document_title(&tab.view));
        }
        if let Some(host) = nyx_core::url::host_of(&url) {
            let creds = vault.passwords_for(&host).unwrap_or_default();
            if let Some(js) = autofill::script(&url, &creds, private) {
                let _ = tab.view.evaluate_script(&js);
            }
        }
    }

    pub(super) fn retitle(&self, id: TabId) {
        let (Some(vault), Some(tab)) = (&self.vault, self.tabs.get(id)) else { return };
        if !tab.title.is_empty() && self.recorder.on_title(id, &tab.url) {
            let _ = vault.retitle_history(&tab.url, &tab.title);
        }
    }

    // ── Favoris ─────────────────────────────────────────────────────────

    pub(super) fn toggle_bookmark(&mut self) {
        let (Some(vault), Some(tab)) = (&self.vault, self.tabs.active()) else { return };
        if !crate::history::recordable(&tab.url) {
            return;
        }
        let title = tab.title.trim().to_string();
        // Le coffre fait foi (synchronisation) : relu avant modification,
        // car la persistance remplace toute la liste.
        store::load_bookmarks(&self.bookmarks, vault);
        let added = bm::toggle(&self.bookmarks, &tab.url, &title);
        store::persist_bookmarks(&self.bookmarks, vault);
        self.toast(if added { "Ajouté aux favoris" } else { "Retiré des favoris" });
        self.push_state();
    }

    // ── Pages internes et leurs actions ─────────────────────────────────

    pub(super) fn serve(&self, page: Page, responder: Responder) {
        let asset = match (&self.vault, page) {
            (None, _) => Asset::not_found(),
            (Some(v), Page::Settings) => {
                let user_data = paths::chrome_user_data(std::env::var("LOCALAPPDATA").ok().as_deref());
                let profiles = import_chrome::profiles(user_data.as_deref());
                Asset::page(pages::settings_html(
                    &self.settings.borrow(),
                    &profiles,
                    v.has_master_password(),
                    &self.devices,
                ))
            }
            (Some(_), Page::Bookmarks) => Asset::page(nyx_core::pages::bookmarks::page_html(&self.bookmarks.borrow())),
            (Some(v), Page::History) => {
                Asset::page(pages::history_html(&v.recent_history(pages::HISTORY_SHOWN).unwrap_or_default()))
            }
            (Some(_), Page::NewTab) => Asset::not_found(),
        };
        responder.0.respond(protocol::to_response(asset));
    }

    pub(super) fn on_action(&mut self, tab: TabId, action: Action) {
        let Some(vault) = &self.vault else { return };
        match action {
            Action::Apply(url) => store::apply_settings(&url, &self.settings, vault, adblock::guard()),
            Action::MoveBookmark(url) => {
                // Le coffre fait foi : relu avant de réécrire toute la liste
                // (sinon des favoris reçus par synchronisation seraient
                // effacés). La page affichée est rechargée après chaque
                // synchronisation qui reçoit des données.
                if let Some((i, to)) = bm::parse_move(&url) {
                    store::load_bookmarks(&self.bookmarks, vault);
                    bm::move_to(&self.bookmarks, i, to);
                    store::persist_bookmarks(&self.bookmarks, vault);
                }
            }
            Action::ClearHistory => {
                let _ = vault.clear_history();
                if let Some(t) = self.tabs.get(tab) {
                    let _ = t.view.load_url(nav::HISTORY_URL);
                }
                self.toast("Historique effacé");
            }
            Action::ImportChrome(url) => {
                let user_data = paths::chrome_user_data(std::env::var("LOCALAPPDATA").ok().as_deref());
                let profiles = import_chrome::profiles(user_data.as_deref());
                let text = match import_chrome::select(&url, &profiles) {
                    Some(profile) => {
                        store::load_bookmarks(&self.bookmarks, vault);
                        let text = import_chrome::run(profile, &self.bookmarks, vault);
                        store::persist_bookmarks(&self.bookmarks, vault);
                        text
                    }
                    None => "Profil Chrome introuvable".to_string(),
                };
                self.toast(&text);
            }
            Action::ImportCapsule => {
                let capsule = capsule_view(self.capsule.as_ref());
                self.set_overlay(Some(Overlay::Capsule { capsule, error: String::new() }));
                self.probe_capsule_again();
            }
            Action::MasterPassword(op) => {
                if let Some(mode) = MasterMode::for_op(op, vault.has_master_password()) {
                    self.set_overlay(Some(Overlay::Master { mode, error: String::new() }));
                }
            }
            Action::Devices(url) => self.on_devices_action(&url),
        }
        self.push_state();
    }
}
