//! `DownloadStore` — CRUD pur sur un `Vec<DownloadEntry>`.
//!
//! Stratégie :
//!   - ajout en tête (les plus récents en haut du popover, sans tri à la
//!     lecture) ;
//!   - id monotone, jamais réutilisé ;
//!   - les transitions de status sont contrôlées par les `mark_*` /
//!     `update_progress` — pas d'API qui mute directement le champ.
//!
//! Pas de `Mutex`, pas de `RefCell` ici : le browser wrappera dans
//! `Rc<RefCell<…>>` (GTK est single-threaded).

use crate::download_policy::Kind;
use crate::downloads::entry::{DownloadEntry, DownloadStatus};
use crate::downloads::id::DownloadId;
use crate::sec_log::{self, Level};

/// Borne dure du nombre d'entrées en mémoire. Au-delà, on évince les
/// entrées **terminales** les plus anciennes (FIFO). En saturation extrême
/// (toutes en-cours), on évince la plus ancienne in-progress — le cap est
/// dur, jamais best-effort. Cible un mois de DL nourris à un rythme
/// raisonnable sans qu'un site adverse puisse forcer une fuite mémoire.
pub const MAX_ENTRIES: usize = 10_000;

#[derive(Debug, Default)]
pub struct DownloadStore {
    entries: Vec<DownloadEntry>,
    next_id: u64,
}

impl DownloadStore {
    pub fn new() -> Self { Self::default() }

    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }

    /// Snapshot ordonné — du plus récent au plus ancien.
    pub fn entries(&self) -> &[DownloadEntry] { &self.entries }

    /// Crée et insère une entrée en tête. Retourne son id.
    /// Applique le cap [`MAX_ENTRIES`] AVANT l'ajout — la nouvelle entrée
    /// est toujours acceptée, l'éviction libère la place si nécessaire.
    pub fn add(
        &mut self,
        filename: String,
        dest_path: String,
        kind: Kind,
        source_origin: String,
        started_at_unix: i64,
    ) -> DownloadId {
        self.enforce_cap();
        let id = self.alloc_id();
        sec_log::emit(Level::Allow, &format!(
            "download start {id}: {filename} ← {source_origin}"));
        let e = DownloadEntry::new(id, filename, dest_path, kind, source_origin, started_at_unix);
        self.entries.insert(0, e);
        id
    }

    /// Garantit `entries.len() < MAX_ENTRIES` avant un add. Stratégie :
    ///   1. Évince le plus ancien terminal (en fin de Vec : insertion en tête).
    ///   2. Saturation extrême (tout in-progress) → évince le plus ancien
    ///      in-progress + log Warn (rare, signale un abus possible).
    fn enforce_cap(&mut self) {
        while self.entries.len() >= MAX_ENTRIES {
            if let Some(pos) = self.entries.iter()
                .rposition(|e| e.status.is_terminal())
            {
                self.entries.remove(pos);
            } else {
                sec_log::emit(Level::Warn, &format!(
                    "download store saturated ({MAX_ENTRIES}) — evicting oldest in-progress"));
                self.entries.pop();
            }
        }
    }

    /// Retire l'entrée. `false` si introuvable.
    pub fn remove(&mut self, id: DownloadId) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        before != self.entries.len()
    }

    /// Vide toutes les entrées (le bouton « Effacer tout »).
    /// Ne touche jamais au disque — supprime juste l'historique en RAM.
    pub fn clear_all(&mut self) {
        let n = self.entries.len();
        self.entries.clear();
        if n > 0 {
            sec_log::emit(Level::Warn, &format!("downloads cleared ({n} entries)"));
        }
    }

    /// Met à jour la progression d'une entrée encore in-progress.
    /// No-op silencieux si l'entrée est terminale ou inconnue.
    pub fn update_progress(&mut self, id: DownloadId, received: u64, total: Option<u64>) {
        if let Some(e) = self.find_mut(id) {
            if let DownloadStatus::InProgress { .. } = e.status {
                e.status = DownloadStatus::InProgress { received, total };
            }
        }
    }

    /// Marque comme terminé avec succès. `final_path` permet de corriger le
    /// chemin si le browser a renommé le fichier (collision, sanitize tardif).
    /// No-op si l'entrée est déjà terminale.
    pub fn mark_completed(&mut self, id: DownloadId, final_path: Option<String>, now_unix: i64) {
        if let Some(e) = self.find_mut(id) {
            if e.status.is_terminal() { return; }
            if let Some(p) = final_path { e.dest_path = p; }
            e.status = DownloadStatus::Completed;
            e.finished_at_unix = now_unix;
            sec_log::emit(Level::Allow, &format!("download done {id}: {}", e.filename));
        }
    }

    /// Marque comme annulé.
    pub fn mark_cancelled(&mut self, id: DownloadId, now_unix: i64) {
        if let Some(e) = self.find_mut(id) {
            if e.status.is_terminal() { return; }
            e.status = DownloadStatus::Cancelled;
            e.finished_at_unix = now_unix;
            sec_log::emit(Level::Warn, &format!("download cancelled {id}"));
        }
    }

    /// Marque comme échoué. `reason` doit être déjà rédactée par le browser.
    pub fn mark_failed(&mut self, id: DownloadId, reason: String, now_unix: i64) {
        if let Some(e) = self.find_mut(id) {
            if e.status.is_terminal() { return; }
            e.status = DownloadStatus::Failed { reason: reason.clone() };
            e.finished_at_unix = now_unix;
            sec_log::emit(Level::Warn, &format!("download failed {id}: {reason}"));
        }
    }

    /// Retire les entrées terminées. Garde celles en cours.
    pub fn prune_completed(&mut self) {
        self.entries.retain(|e| !e.status.is_terminal());
    }

    fn find_mut(&mut self, id: DownloadId) -> Option<&mut DownloadEntry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    fn alloc_id(&mut self) -> DownloadId {
        let id = DownloadId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }
}
