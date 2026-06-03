//! HSTS — politique HTTP Strict Transport Security pure.
//!
//! Le navigateur consulte [`HstsStore::decide`] avant chaque navigation
//! `http://` ; si la décision est `UpgradeToHttps`, il rewrite l'URL en
//! `https://` AVANT d'émettre la requête. La réception d'une réponse HTTPS
//! avec `Strict-Transport-Security: …` est passée à [`upsert_from_header`].
//!
//! Ce qu'il ne fait PAS :
//!   - aucun I/O réseau (lookup pur en mémoire) ;
//!   - aucune persistance disque (un wrapper futur s'en chargera) ;
//!   - aucun parsing PSL — la preload list est statique, embeddée.
//!
//! Voir docs/security.md §X « Transport ».

use std::collections::HashMap;

use crate::sec_log::{self, Level};

mod preload;

// ─── Modèle ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HstsEntry {
    pub host: String,
    pub include_subdomains: bool,
    pub expires_at_unix: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Aucune action — laisser passer (HTTPS direct, scheme hors scope, etc.).
    Pass,
    /// Réécrire en `https://` avant émission réseau.
    UpgradeToHttps,
    /// Réservé : refus catégorique (cert invalide signalé hors-bande).
    /// Pas émis par l'API actuelle — placeholder pour intégrations futures.
    Block,
}

/// Magasin en mémoire : entrées indexées par host normalisé (lowercase, sans
/// point de fin). Reset à chaque démarrage tant que la persistance n'est pas
/// branchée.
#[derive(Debug, Default)]
pub struct HstsStore {
    entries: HashMap<String, HstsEntry>,
}

const TWO_YEARS_SECONDS: u64 = 2 * 365 * 86_400; // 63_072_000

// ─── API publique ──────────────────────────────────────────────────────────

impl HstsStore {
    pub fn new() -> Self { Self::default() }

    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }

    /// Applique un header `Strict-Transport-Security`. `max-age=0` purge.
    /// `now` est l'epoch unix courant (passé par le browser pour rester pur).
    pub fn upsert_from_header(&mut self, host: &str, header: &str, now: i64) {
        let host = normalize_host(host);
        if host.is_empty() { return; }
        let Some((max_age, include_sub)) = parse_sts_header(header) else { return; };

        if max_age == 0 {
            self.entries.remove(&host);
            sec_log::emit(Level::Warn, &format!("hsts purge {host} (max-age=0)"));
            return;
        }

        let clamped = max_age.min(TWO_YEARS_SECONDS);
        let expires_at_unix = now.saturating_add(clamped as i64);
        sec_log::emit(Level::Warn,
            &format!("hsts upsert {host} max-age={clamped} sub={include_sub}"));
        self.entries.insert(host.clone(), HstsEntry {
            host, include_subdomains: include_sub, expires_at_unix,
        });
    }

    /// Décide pour une URL. `now` est l'epoch unix courant.
    pub fn decide(&self, url: &str, now: i64) -> Decision {
        let (scheme, host) = parse_url(url);
        if scheme != "http" { return Decision::Pass; } // https/file/nyx/data → no-op
        if host.is_empty() { return Decision::Pass; }
        let host = host.to_ascii_lowercase();

        // 1. Match exact non-expiré.
        if let Some(e) = self.entries.get(host.as_str()) {
            if e.expires_at_unix > now {
                sec_log::emit(Level::Allow, &format!("hsts upgrade http://{host}"));
                return Decision::UpgradeToHttps;
            }
        }
        // 2. Match parent avec includeSubdomains.
        for parent in parents(host.as_str()) {
            if let Some(e) = self.entries.get(parent) {
                if e.include_subdomains && e.expires_at_unix > now {
                    sec_log::emit(Level::Allow,
                        &format!("hsts upgrade http://{host} (parent {parent})"));
                    return Decision::UpgradeToHttps;
                }
            }
        }
        // 3. Preload list (toujours includeSubdomains côté politique).
        if preload::is_preloaded(host.as_str()) {
            sec_log::emit(Level::Allow,
                &format!("hsts upgrade http://{host} (preload)"));
            return Decision::UpgradeToHttps;
        }

        Decision::Pass
    }

    /// Supprime les entrées expirées. À appeler périodiquement (ex. au
    /// démarrage et à chaque tour de boucle de housekeeping).
    pub fn purge_expired(&mut self, now: i64) {
        self.entries.retain(|_, e| e.expires_at_unix > now);
    }
}

// ─── Parsing header (visible pour les tests, pas pour l'extérieur) ─────────

/// Parse un header `Strict-Transport-Security` minimal mais robuste.
///
/// Retourne `Some((max_age, include_subdomains))` si `max-age` est présent
/// et valide ; `None` sinon (le header est alors ignoré). Tolère :
/// - whitespace autour des `;` et `=` ;
/// - guillemets autour de la valeur de max-age (RFC 7230 §3.2.6) ;
/// - directives inconnues (juste ignorées) ;
/// - casse mélangée sur les noms (`Max-Age`, `INCLUDESUBDOMAINS`, …).
pub(crate) fn parse_sts_header(h: &str) -> Option<(u64, bool)> {
    let mut max_age: Option<u64> = None;
    let mut include_sub = false;
    for raw in h.split(';') {
        let token = raw.trim();
        if token.is_empty() { continue; }
        if let Some((k, v)) = token.split_once('=') {
            if k.trim().eq_ignore_ascii_case("max-age") {
                let v = v.trim().trim_matches('"');
                if let Ok(n) = v.parse::<u64>() { max_age = Some(n); }
            }
            // valeurs inconnues : ignorées.
        } else if token.eq_ignore_ascii_case("includesubdomains") {
            include_sub = true;
        }
        // `preload`, `; ;`, inconnues : ignorées silencieusement.
    }
    max_age.map(|m| (m, include_sub))
}

// ─── URL / host helpers ────────────────────────────────────────────────────

fn parse_url(url: &str) -> (&str, &str) {
    let s = url.trim();
    let Some(idx) = s.find("://") else { return ("", ""); };
    let scheme = &s[..idx];
    let after = &s[idx + 3..];
    let end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let authority = &after[..end];
    // Strip userinfo
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    // Strip port — IPv6 brackets keep the host
    let host = if let Some(stripped) = authority.strip_prefix('[') {
        // [::1]:8080 → ::1 (on retire les brackets)
        if let Some(end) = stripped.find(']') { &stripped[..end] } else { stripped }
    } else {
        authority.split(':').next().unwrap_or(authority)
    };
    (scheme, host)
}

fn normalize_host(host: &str) -> String {
    host.trim()
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

/// Itère les domaines parents (sans inclure le host lui-même).
/// `"a.b.c.com"` → `["b.c.com", "c.com", "com"]`.
fn parents(host: &str) -> impl Iterator<Item = &str> {
    let mut s = host;
    std::iter::from_fn(move || {
        let dot = s.find('.')?;
        s = &s[dot + 1..];
        if s.is_empty() { None } else { Some(s) }
    })
}

#[cfg(test)]
mod tests;
