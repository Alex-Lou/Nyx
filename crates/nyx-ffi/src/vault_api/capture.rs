//! Capture des mots de passe soumis (`assets/password_capture.js`, reçu par
//! `WebViewCompat.addWebMessageListener`) — pur, testé sur l'hôte.
//!
//! N'importe quelle page peut poster n'importe quoi : chaque message est
//! revalidé contre l'origine de l'émetteur (fournie par la WebView, pas par
//! la page) ET l'URL principale actuelle de l'onglet. Rien n'est enregistré
//! sans « Oui » explicite ; le réglage « Enregistrer les mots de passe » et
//! la navigation privée coupent tout. Même plan que Windows (`capture.rs`) :
//! rien si déjà à jour, mise à jour si l'identifiant existe, ajout sinon.

use serde::{Deserialize, Serialize};

use nyx_core::url::{host_of, scheme_of};
use vault::Password;

use super::Session;

/// Taille max d'un message (identifiant + mot de passe + hôte).
pub const MAX_MESSAGE_LEN: usize = 8 * 1024;

#[derive(Deserialize)]
struct Message {
    nyx: String,
    #[serde(default)]
    username: String,
    password: String,
    #[serde(default)]
    host: String,
}

impl Drop for Message {
    fn drop(&mut self) {
        let mut bytes = std::mem::take(&mut self.password).into_bytes();
        super::wipe(&mut bytes);
    }
}

/// Question à poser (jamais le mot de passe).
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct CaptureOffer {
    pub host: String,
    pub question: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Plan {
    Nothing,
    Add,
    Update { id: i64 },
}

fn plan(existing: &[Password], username: &str, password: &str) -> Plan {
    if password.is_empty() {
        return Plan::Nothing;
    }
    match existing.iter().find(|p| p.username == username) {
        Some(known) if known.password == password => Plan::Nothing,
        Some(known) => known.id.map_or(Plan::Nothing, |id| Plan::Update { id }),
        None => Plan::Add,
    }
}

fn question(plan: Plan, username: &str, host: &str) -> Option<String> {
    let who = if username.is_empty() { "ce compte" } else { username };
    match plan {
        Plan::Nothing => None,
        Plan::Add => Some(format!("Enregistrer le mot de passe de {who} pour {host} ?")),
        Plan::Update { .. } => Some(format!("Mettre à jour le mot de passe de {who} pour {host} ?")),
    }
}

/// Message valide : https, hôte annoncé = hôte de l'émetteur = hôte de la
/// page principale de l'onglet. Renvoie l'hôte (minuscules) et le message.
fn accept(source_origin: &str, top_url: &str, raw: &str) -> Option<(String, Message)> {
    if raw.len() > MAX_MESSAGE_LEN || scheme_of(source_origin).as_deref() != Some("https") {
        return None;
    }
    if scheme_of(top_url).as_deref() != Some("https") {
        return None;
    }
    let msg: Message = serde_json::from_str(raw).ok()?;
    if msg.nyx != "password" {
        return None;
    }
    let source = host_of(source_origin)?;
    let top = host_of(top_url)?;
    if !source.eq_ignore_ascii_case(&top) || !msg.host.eq_ignore_ascii_case(&source) {
        return None;
    }
    if source.eq_ignore_ascii_case(super::APP_ASSETS_HOST) {
        return None;
    }
    Some((source.to_ascii_lowercase(), msg))
}

/// Offre d'enregistrement pour un message de capture, ou `None` s'il faut
/// l'ignorer (émetteur non conforme, réglage coupé, privé, déjà à jour).
pub fn capture_offer(
    session: &Session,
    source_origin: &str,
    top_url: &str,
    raw: &str,
    private: bool,
) -> Option<CaptureOffer> {
    if private || !session.is_open() || !session.app_settings().save_passwords {
        return None;
    }
    let (host, msg) = accept(source_origin, top_url, raw)?;
    let existing = session.vault().ok()?.passwords_for(&host).ok()?;
    let question = question(plan(&existing, &msg.username, &msg.password), &msg.username, &host)?;
    Some(CaptureOffer { host, question })
}

/// « Oui » : revalide le message pour `host` (celui de l'offre), recalcule
/// le plan sur l'état actuel du coffre et l'applique. `true` = enregistré
/// (ou déjà à jour).
pub fn capture_save(session: &Session, host: &str, raw: &str) -> Result<bool, String> {
    let origin = format!("https://{host}");
    let Some((host, msg)) = accept(&origin, &origin, raw) else {
        return Err("message refusé".into());
    };
    let vault = session.vault()?;
    let existing = vault.passwords_for(&host).map_err(|e| format!("{e:#}"))?;
    match plan(&existing, &msg.username, &msg.password) {
        Plan::Nothing => Ok(true),
        Plan::Add => vault
            .add_password(&Password::new(&host, &msg.username, &msg.password))
            .map(|_| true)
            .map_err(|e| format!("{e:#}")),
        Plan::Update { id } => vault.update_password(id, &msg.password).map(|_| true).map_err(|e| format!("{e:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{open_session, TempDir};
    use super::*;

    const MSG: &str = r#"{"nyx":"password","username":"alex","password":"hunter2","host":"example.com"}"#;

    #[test]
    fn plans_and_questions() {
        let stored = |id, user: &str, pw: &str| Password { id: Some(id), ..Password::new("example.com", user, pw) };
        let existing = [stored(4, "alex", "old"), stored(5, "sam", "same")];
        assert_eq!(plan(&existing, "alex", "new"), Plan::Update { id: 4 });
        assert_eq!(plan(&existing, "sam", "same"), Plan::Nothing);
        assert_eq!(plan(&existing, "lou", "pw"), Plan::Add);
        assert_eq!(plan(&[], "lou", ""), Plan::Nothing);
        assert_eq!(
            question(Plan::Add, "", "example.com").as_deref(),
            Some("Enregistrer le mot de passe de ce compte pour example.com ?")
        );
    }

    #[test]
    fn sender_checks() {
        let ok = |src: &str, top: &str, raw: &str| accept(src, top, raw).map(|(h, _)| h);
        assert_eq!(ok("https://Example.com", "https://example.com/login", MSG).as_deref(), Some("example.com"));
        assert_eq!(ok("https://evil.com", "https://example.com/login", MSG), None, "autre émetteur");
        assert_eq!(ok("https://example.com", "https://evil.com/", MSG), None, "onglet ailleurs");
        assert_eq!(ok("http://example.com", "http://example.com/", MSG), None, "http");
        let claimed = MSG.replace("example.com", "bank.com");
        assert_eq!(ok("https://example.com", "https://example.com/", &claimed), None, "hôte annoncé");
        let other = MSG.replace("\"password\",", "\"action\",");
        assert_eq!(ok("https://example.com", "https://example.com/", &other), None);
        assert_eq!(ok("https://example.com", "https://example.com/", "pas du json"), None);
        let huge = MSG.replace("hunter2", &"x".repeat(MAX_MESSAGE_LEN));
        assert_eq!(ok("https://example.com", "https://example.com/", &huge), None);
    }

    #[test]
    fn offer_then_save_then_update() {
        let tmp = TempDir::new("capture");
        let s = open_session(&tmp);
        let offer = capture_offer(&s, "https://example.com", "https://example.com/login", MSG, false).expect("offre");
        assert_eq!(offer.host, "example.com");
        assert_eq!(offer.question, "Enregistrer le mot de passe de alex pour example.com ?");
        assert!(!offer.question.contains("hunter2"));
        assert!(capture_offer(&s, "https://example.com", "https://example.com/", MSG, true).is_none(), "privé");

        assert!(capture_save(&s, "example.com", MSG).expect("save"));
        assert!(capture_save(&s, "evil.com", MSG).is_err(), "hôte différent de l'offre");
        assert!(capture_offer(&s, "https://example.com", "https://example.com/", MSG, false).is_none(), "à jour");

        let changed = MSG.replace("hunter2", "nouveau");
        let offer = capture_offer(&s, "https://example.com", "https://example.com/", &changed, false).expect("maj");
        assert!(offer.question.starts_with("Mettre à jour"));
        assert!(capture_save(&s, "example.com", &changed).expect("update"));
        let rows: Vec<serde_json::Value> = serde_json::from_str(&s.passwords_json().unwrap()).unwrap();
        assert_eq!(rows.len(), 1);
        let id = rows[0]["id"].as_i64().unwrap();
        assert_eq!(s.password_reveal("example.com", id).unwrap(), "nouveau");

        // Réglage coupé : plus aucune offre.
        let mut a = s.app_settings();
        a.save_passwords = false;
        s.save_app_settings(&a).unwrap();
        let third = MSG.replace("alex", "sam");
        assert!(capture_offer(&s, "https://example.com", "https://example.com/", &third, false).is_none());
    }
}
