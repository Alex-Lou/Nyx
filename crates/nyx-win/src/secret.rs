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

/// Contrôle d'une nouvelle passphrase (création du coffre) : non vide et
/// identique à sa confirmation. Le message est affiché tel quel.
pub fn check_new_passphrase(passphrase: &Secret, confirm: &Secret) -> Result<(), &'static str> {
    if passphrase.expose().trim().is_empty() {
        Err("La passphrase est vide")
    } else if passphrase != confirm {
        Err("Les passphrases ne correspondent pas")
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_passphrase_rules() {
        let s = |v: &str| Secret::new(v.into());
        assert_eq!(check_new_passphrase(&s("a b c"), &s("a b c")), Ok(()));
        assert_eq!(check_new_passphrase(&s(""), &s("")), Err("La passphrase est vide"));
        assert_eq!(check_new_passphrase(&s("   "), &s("   ")), Err("La passphrase est vide"));
        assert_eq!(check_new_passphrase(&s("abc"), &s("abd")), Err("Les passphrases ne correspondent pas"));
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
