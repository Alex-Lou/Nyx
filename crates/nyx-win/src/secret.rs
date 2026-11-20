//! Chaîne sensible (passphrase, mot de passe capturé) : jamais affichée par
//! `Debug`, effacée (au mieux) à la destruction.
//!
//! `zeroize` n'est pas une dépendance du projet : l'effacement est fait en
//! code sûr (octets mis à zéro puis `black_box` pour décourager
//! l'élimination). Les copies faites en amont (WebView2, wry, serde_json)
//! échappent à ce contrôle : on se contente de ne pas en ajouter et de
//! lâcher la nôtre au plus tôt.

use serde::{Deserialize, Deserializer};

#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    #[cfg(test)]
    pub fn new(s: String) -> Self {
        Self(s)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d).map(Self)
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        let mut bytes = std::mem::take(&mut self.0).into_bytes();
        bytes.iter_mut().for_each(|b| *b = 0);
        std::hint::black_box(&bytes);
    }
}

impl Secret {
    /// Copie vers le type du coffre (lui aussi effacé à la destruction).
    pub fn to_passphrase(&self) -> vault::Passphrase {
        vault::Passphrase::new(self.expose())
    }
}

/// Contrôle d'un nouveau mot de passe principal (règles de
/// `vault::keymode` : non vide, 8 caractères min., confirmé). Le message
/// est affiché tel quel.
pub fn check_new_passphrase(passphrase: &Secret, confirm: &Secret) -> Result<(), &'static str> {
    vault::keymode::check_new_passphrase(passphrase.expose(), confirm.expose())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_passphrase_rules() {
        let s = |v: &str| Secret::new(v.into());
        assert_eq!(check_new_passphrase(&s("a b c d e"), &s("a b c d e")), Ok(()));
        assert_eq!(check_new_passphrase(&s(""), &s("")), Err("Le mot de passe est vide"));
        assert_eq!(check_new_passphrase(&s("   "), &s("   ")), Err("Le mot de passe est vide"));
        assert!(check_new_passphrase(&s("abc"), &s("abc")).is_err(), "trop court");
        assert_eq!(
            check_new_passphrase(&s("abcdefgh"), &s("abcdefgi")),
            Err("Les mots de passe ne correspondent pas")
        );
        assert_eq!(s("x").to_passphrase().expose(), "x");
    }

    #[test]
    fn never_printed() {
        let s: Secret = serde_json::from_str("\"hunter2\"").expect("json");
        assert_eq!(s.expose(), "hunter2");
        assert_eq!(format!("{s:?}"), "Secret(***)");
        assert!(!format!("{:?}", Some(s.clone())).contains("hunter2"));
        assert!(Secret::default().is_empty());
    }
}
