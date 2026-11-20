//! Enregistrement des identifiants capturés — décision pure (même règle que
//! le desktop `web/password_capture.rs`) : rien si déjà à jour, mise à jour
//! si l'identifiant existe avec un autre mot de passe, ajout sinon. Toujours
//! après un « Oui » explicite dans le chrome.

use vault::Password;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// Déjà enregistré tel quel.
    Nothing,
    Add,
    Update { id: i64 },
}

pub fn plan(existing: &[Password], username: &str, password: &str) -> Plan {
    if password.is_empty() {
        return Plan::Nothing;
    }
    match existing.iter().find(|p| p.username == username) {
        Some(known) if known.password == password => Plan::Nothing,
        Some(known) => known.id.map_or(Plan::Nothing, |id| Plan::Update { id }),
        None => Plan::Add,
    }
}

/// Question posée dans le chrome (jamais le mot de passe).
pub fn question(plan: Plan, username: &str, host: &str) -> Option<String> {
    let who = if username.is_empty() { "ce compte" } else { username };
    match plan {
        Plan::Nothing => None,
        Plan::Add => Some(format!("Enregistrer le mot de passe de {who} sur {host} ?")),
        Plan::Update { .. } => Some(format!("Mettre à jour le mot de passe de {who} sur {host} ?")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(id: i64, user: &str, pw: &str) -> Password {
        Password { id: Some(id), ..Password::new("example.com", user, pw) }
    }

    #[test]
    fn plans() {
        let existing = [stored(4, "alex", "old"), stored(5, "sam", "same")];
        assert_eq!(plan(&existing, "alex", "new"), Plan::Update { id: 4 });
        assert_eq!(plan(&existing, "sam", "same"), Plan::Nothing);
        assert_eq!(plan(&existing, "lou", "pw"), Plan::Add);
        assert_eq!(plan(&[], "", "pw"), Plan::Add);
        assert_eq!(plan(&[], "lou", ""), Plan::Nothing);
        // Ligne sans id (jamais lue du coffre) : on ne devine pas.
        assert_eq!(plan(&[Password::new("example.com", "alex", "x")], "alex", "y"), Plan::Nothing);
    }

    #[test]
    fn questions() {
        assert_eq!(
            question(Plan::Add, "alex", "example.com").as_deref(),
            Some("Enregistrer le mot de passe de alex sur example.com ?")
        );
        assert_eq!(
            question(Plan::Update { id: 1 }, "", "example.com").as_deref(),
            Some("Mettre à jour le mot de passe de ce compte sur example.com ?")
        );
        assert_eq!(question(Plan::Nothing, "a", "b"), None);
    }
}
