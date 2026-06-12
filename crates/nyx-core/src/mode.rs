//! Mode global du navigateur — partagé entre toutes les surfaces de sécurité.
//!
//! Scope : énumération + un seul prédicat (`is_strict`). Aucune politique :
//! chaque module de sécurité (permissions, download_policy, vault_autofill,
//! cookie_policy…) consomme `Mode` et applique sa propre sémantique.
//!
//! Ce qu'il ne fait PAS :
//!   - aucune décision (pas de Verdict, pas de Decision).
//!   - aucun lien WebKit / I/O.
//!
//! Voir docs/security.md §1 « Modes ».

/// Mode d'usage du navigateur.
///
/// Default = `Normal`. Les modules de sécurité interprètent les variantes
/// selon leur surface ; les invariants partagés sont :
///
/// - `Normal` : politique par défaut, mémoire autorisée.
/// - `Shadow` : aucune persistance, défauts plus stricts.
/// - `Banking` : whitelist explicite, refus secs sur tout le reste.
/// - `Dev` : assouplissements pour le développement local. Tous les modules
///   ne distinguent pas `Dev` de `Normal` (ex. `permissions` traite `Dev`
///   exactement comme `Normal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Mode {
    /// Politique standard. Mémoire des décisions utilisateur autorisée.
    #[default]
    Normal,
    /// Session éphémère : aucune persistance, défauts au plus prudent.
    Shadow,
    /// Mode bancaire : whitelist stricte, tout 3rd-party refusé.
    Banking,
    /// Développement local : assouplissements ciblés (localhost, .deb, etc.).
    Dev,
}

impl Mode {
    /// `true` pour les modes qui durcissent la politique par défaut.
    ///
    /// Sert aux modules qui veulent un test générique "doit-on être strict ?"
    /// sans dupliquer un `matches!(...)` à chaque appel. `Dev` ne durcit
    /// rien (au contraire), donc renvoie `false`.
    pub fn is_strict(self) -> bool {
        matches!(self, Self::Shadow | Self::Banking)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_normal() {
        assert_eq!(Mode::default(), Mode::Normal);
    }

    #[test]
    fn is_strict_covers_all_variants() {
        assert!(!Mode::Normal.is_strict());
        assert!( Mode::Shadow.is_strict());
        assert!( Mode::Banking.is_strict());
        assert!(!Mode::Dev.is_strict());
    }

    #[test]
    fn copy_clone_and_eq() {
        let m = Mode::Banking;
        let c = m;
        assert_eq!(m, c);
        assert_eq!(m.clone(), Mode::Banking);
    }

    #[test]
    fn debug_formats_variant_name() {
        // Stable enough to be useful in logs sans casser si on renomme.
        for (m, name) in [
            (Mode::Normal,  "Normal"),
            (Mode::Shadow,  "Shadow"),
            (Mode::Banking, "Banking"),
            (Mode::Dev,     "Dev"),
        ] {
            assert_eq!(format!("{m:?}"), name);
        }
    }

    /// Stress : usage en HashMap (le derive Hash est utilisé par les
    /// stores origin-scopés des modules de sécurité).
    #[test]
    fn hash_usable_in_map() {
        use std::collections::HashMap;
        let mut m = HashMap::new();
        for v in [Mode::Normal, Mode::Shadow, Mode::Banking, Mode::Dev] {
            m.insert(v, v.is_strict());
        }
        assert_eq!(m.len(), 4);
        assert!(m[&Mode::Banking]);
        assert!(!m[&Mode::Dev]);
    }
}
