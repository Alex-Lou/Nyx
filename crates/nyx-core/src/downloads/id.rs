//! Identifiant opaque d'un téléchargement.
//!
//! Monotone, non réutilisé après suppression : un id retiré du store ne
//! revient jamais (évite que deux callbacks WebKit en vol se retrouvent
//! à muter la même entrée par erreur).

/// Identifiant unique. Opaque côté UI ; juste comparable et hashable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DownloadId(pub(super) u64);

impl DownloadId {
    /// Le `u64` brut — utile pour logger ou sérialiser. Pas pour réimporter
    /// dans le store (le store gère son propre compteur).
    pub fn as_u64(self) -> u64 { self.0 }
}

impl std::fmt::Display for DownloadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}
