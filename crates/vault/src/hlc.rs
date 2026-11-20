//! Horloge logique hybride (HLC), empaquetée dans un `u64`.
//!
//! Les 48 bits de poids fort portent le temps physique en millisecondes Unix
//! (suffisant jusqu'en l'an 10889), les 16 bits de poids faible un compteur
//! logique qui départage les événements d'une même milliseconde — ou qui
//! fait avancer l'horloge quand l'horloge murale recule. Comparer deux HLC
//! revient donc à comparer deux entiers.
//!
//! Propriétés garanties par [`tick`] et [`observe`] :
//! * monotonie stricte (chaque valeur émise est > la précédente) ;
//! * causalité : après avoir observé une valeur distante `r`, toute valeur
//!   émise localement est > `r`.
//!
//! Les valeurs restent < 2^63 (stockables en `INTEGER` SQLite) tant que le
//! temps physique tient sur 47 bits, soit jusqu'en l'an 6429.

use std::time::{SystemTime, UNIX_EPOCH};

/// Bits du compteur logique.
pub const LOGICAL_BITS: u32 = 16;

/// Avance maximale tolérée d'une HLC distante sur notre horloge murale :
/// au-delà, l'enregistrement est refusé (horloge cassée ou pair malveillant)
/// plutôt que de propulser définitivement notre horloge dans le futur.
pub const MAX_DRIFT_MS: u64 = 24 * 3600 * 1000;

/// HLC correspondant au début de la milliseconde `ms` (compteur à zéro).
pub const fn from_millis(ms: u64) -> u64 {
    ms << LOGICAL_BITS
}

/// Partie physique (millisecondes Unix) d'une HLC.
pub const fn millis(hlc: u64) -> u64 {
    hlc >> LOGICAL_BITS
}

/// Horloge murale en millisecondes Unix (0 si l'horloge est avant 1970).
pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Événement local : nouvelle valeur strictement supérieure à `last`.
pub fn tick(last: u64, now_ms: u64) -> u64 {
    from_millis(now_ms).max(last.saturating_add(1))
}

/// Réception d'une valeur distante `remote` : nouvelle valeur strictement
/// supérieure à `last` ET à `remote`.
pub fn observe(last: u64, remote: u64, now_ms: u64) -> u64 {
    tick(last.max(remote), now_ms)
}

/// `true` si `remote` n'est pas trop en avance sur l'horloge murale.
pub fn is_plausible(remote: u64, now_ms: u64) -> bool {
    millis(remote) <= now_ms.saturating_add(MAX_DRIFT_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_suit_l_horloge_murale() {
        let h = tick(0, 1_000);
        assert_eq!(h, from_millis(1_000));
        assert_eq!(millis(h), 1_000);
        assert_eq!(tick(h, 2_000), from_millis(2_000));
    }

    #[test]
    fn tick_strictement_monotone_meme_milliseconde() {
        let a = tick(0, 1_000);
        let b = tick(a, 1_000);
        let c = tick(b, 1_000);
        assert!(a < b && b < c);
        assert_eq!(millis(c), 1_000);
        assert_eq!(c & 0xFFFF, 2);
    }

    #[test]
    fn tick_resiste_au_recul_de_l_horloge() {
        let a = tick(0, 5_000);
        let b = tick(a, 1_000); // l'horloge murale a reculé
        assert!(b > a);
        assert_eq!(millis(b), 5_000);
    }

    #[test]
    fn observe_depasse_la_valeur_distante() {
        let local = tick(0, 1_000);
        let remote = from_millis(9_000) + 7;
        let h = observe(local, remote, 1_000);
        assert!(h > remote && h > local);
        // Puis les ticks locaux restent au-dessus.
        assert!(tick(h, 1_000) > remote);
        // Une valeur distante ancienne ne fait pas reculer.
        let h2 = observe(h, 1, 1_000);
        assert!(h2 > h);
    }

    #[test]
    fn debordement_du_compteur_avance_la_milliseconde() {
        let last = from_millis(1_000) | 0xFFFF;
        let h = tick(last, 1_000);
        assert_eq!(h, from_millis(1_001));
    }

    #[test]
    fn plausibilite() {
        let now = 10_000_000;
        assert!(is_plausible(from_millis(now), now));
        assert!(is_plausible(from_millis(now + MAX_DRIFT_MS), now));
        assert!(!is_plausible(from_millis(now + MAX_DRIFT_MS + 1), now));
        assert!(tick(0, now_millis()) < (1u64 << 63));
    }
}
