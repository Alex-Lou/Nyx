//! Pré-remplissage des identifiants — script et garde-fous partagés avec
//! Android (`nyx_core::vault_autofill::fill_script` : politique unique,
//! Deny par défaut, https + hôte exact + un seul compte). Ici : exclusion
//! des pages internes Windows. Pur, testé sur l'hôte.

use nyx_core::vault_autofill::{fill_script, Credential};
use vault::Password;

/// Script de pré-remplissage pour `page_url`, ou `None` si la politique ne
/// l'autorise pas silencieusement. `creds` : identifiants du coffre pour
/// l'hôte exact de la page. `private` : navigation privée (mode Shadow →
/// jamais d'autofill silencieux).
pub fn script(page_url: &str, creds: &[Password], private: bool) -> Option<String> {
    if crate::nav::is_internal(page_url) {
        return None;
    }
    let creds: Vec<Credential> = creds
        .iter()
        .map(|c| Credential { domain: &c.domain, username: &c.username, password: &c.password })
        .collect();
    fill_script(page_url, &creds, private)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cred(domain: &str, user: &str, pw: &str) -> Password {
        Password::new(domain, user, pw)
    }

    #[test]
    fn fills_exact_https_host() {
        let js = script("https://example.com/login", &[cred("example.com", "alex", "p\"w</script>")], false)
            .expect("autorisé");
        assert!(js.ends_with(r#"("https://example.com","alex","p\"w\u003c/script>")"#), "{js}");
        assert!(js.contains("window.top !== window"));
    }

    #[test]
    fn refuses_unsafe_contexts() {
        let c = [cred("example.com", "alex", "pw")];
        assert_eq!(script("http://example.com/login", &c, false), None, "http");
        assert_eq!(script("https://evil.com/login", &c, false), None, "autre hôte");
        assert_eq!(script("https://sub.example.com/", &c, false), None, "sous-domaine");
        assert_eq!(script("https://example.com:8443/", &c, false), None, "autre port");
        assert_eq!(script("https://example.com/", &c, true), None, "navigation privée");
        assert_eq!(script("https://nyx.localhost/settings", &[cred("nyx.localhost", "a", "b")], false), None);
        assert_eq!(script("file:///C:/x.html", &c, false), None);
        assert_eq!(script("https://example.com/", &[], false), None, "rien en coffre");
        assert_eq!(
            script("https://example.com/", &[cred("example.com", "a", "1"), cred("example.com", "b", "2")], false),
            None,
            "plusieurs comptes : pas de choix silencieux"
        );
        assert_eq!(script("https://example.com/", &[cred("example.com", "a", "")], false), None);
        // Homographe IDN : refusé par la politique même avec une entrée.
        assert_eq!(script("https://pаypal.com/", &[cred("pаypal.com", "a", "b")], false), None);
    }
}
