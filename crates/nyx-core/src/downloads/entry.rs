//! Modèle d'une entrée de téléchargement.
//!
//! `DownloadEntry` est le source-of-truth pour l'UI : le popover lit la
//! liste, rend une ligne par entrée, et appelle les actions du store.
//! Toutes les valeurs sont `String`/`i64` pour rester sérialisables et
//! sans aucune dépendance lourde (pas de `PathBuf` dans `nyx-core` —
//! le browser convertit à la frontière).

use crate::download_policy::Kind;
use crate::downloads::id::DownloadId;

/// État courant d'un téléchargement.
///
/// Les transitions valides sont strictement : `InProgress → Completed`,
/// `InProgress → Cancelled`, `InProgress → Failed`. Une fois terminée,
/// l'entrée ne mute plus (sauf suppression via `remove`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadStatus {
    /// En cours. `received` est en octets, `total` peut être inconnu (None).
    InProgress { received: u64, total: Option<u64> },
    Completed,
    /// Annulé par l'utilisateur ou par une politique (download_policy::Block).
    Cancelled,
    /// Échec côté réseau / disque. La raison est libre (déjà rédactée par
    /// le browser, jamais brute).
    Failed { reason: String },
}

impl DownloadStatus {
    /// `true` si l'entrée est terminale (ne peut plus muter).
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::InProgress { .. })
    }

    /// Progression normalisée \[0.0, 1.0\] si calculable.
    pub fn fraction(&self) -> Option<f64> {
        match self {
            Self::InProgress { received, total: Some(t) } if *t > 0 =>
                Some((*received as f64 / *t as f64).clamp(0.0, 1.0)),
            Self::Completed => Some(1.0),
            _ => None,
        }
    }
}

/// Entrée publique. Tous les champs sont en clair pour l'UI ; la value
/// d'un éventuel token a déjà été stripée par le browser avant insertion
/// (cf. `sec_log::redact_url`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadEntry {
    pub id: DownloadId,
    /// Nom de fichier final (sanitizé par `download_policy::analyze`).
    pub filename: String,
    /// Chemin absolu de destination, posix-style ("/" en séparateur).
    /// Le browser convertit en `Path` à la frontière.
    pub dest_path: String,
    pub kind: Kind,
    /// Origine source (scheme + host, sans path / query / fragment).
    pub source_origin: String,
    pub status: DownloadStatus,
    /// Unix epoch (s). Le browser fournit le temps (nyx-core reste pur).
    pub started_at_unix: i64,
    /// Unix epoch (s) — `0` tant que pas terminé.
    pub finished_at_unix: i64,
}

impl DownloadEntry {
    /// Constructeur minimal — un nouveau DL en cours, sans progression connue.
    pub fn new(
        id: DownloadId, filename: String, dest_path: String,
        kind: Kind, source_origin: String, started_at_unix: i64,
    ) -> Self {
        Self {
            id, filename, dest_path, kind, source_origin,
            status: DownloadStatus::InProgress { received: 0, total: None },
            started_at_unix, finished_at_unix: 0,
        }
    }
}
