//! Wrapper minimal autour de `sha2::Sha256`.
//!
//! Pourquoi un wrapper et pas un `pub use sha2;` ? Pour découpler l'API
//! publique de Nyx de la version de la crate `sha2` (changement majeur
//! → un seul fichier à toucher), et pour fournir une surface API
//! parfaitement scopée à l'usage du bridge download (post-flight hash
//! d'un fichier).
//!
//! Pure : ne touche pas au disque. Le browser feed les chunks lus depuis
//! le fichier temporaire. Sortie = chaîne hex 64 caractères (lowercase).

use sha2::{Digest, Sha256};

/// Hasher streaming. Construire, `update` n fois, `finalize_hex`.
pub struct Sha256Hasher(Sha256);

impl Sha256Hasher {
    pub fn new() -> Self { Self(Sha256::new()) }
    pub fn update(&mut self, chunk: &[u8]) { self.0.update(chunk); }

    /// Consomme le hasher et retourne le digest hex (64 chars, lowercase).
    pub fn finalize_hex(self) -> String {
        let digest = self.0.finalize();
        let mut out = String::with_capacity(64);
        for byte in digest.iter() {
            // `write!` sur String est infaillible — `to_string` allouerait deux fois.
            use std::fmt::Write;
            let _ = write!(out, "{byte:02x}");
        }
        out
    }
}

impl Default for Sha256Hasher {
    fn default() -> Self { Self::new() }
}

/// One-shot : SHA-256 hex (lowercase, 64 chars) d'un buffer complet.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256Hasher::new();
    h.update(bytes);
    h.finalize_hex()
}

#[cfg(test)]
mod tests;
