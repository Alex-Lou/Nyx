//! « Appareils liés » côté UI — pur, testé sur l'hôte : actions internes
//! `nyx://devices/…` émises par la page Réglages, état affiché (alimenté
//! par les événements de `sync_service`), rendu de la section HTML.

use nyx_core::url::{form_decode, form_encode, html_escape};

use crate::sync_service::{DeviceRow, Event, LastSync};

/// Action demandée par la page Réglages (déjà autorisée par
/// `security::decide` : page émettrice interne).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceOp {
    Pair,
    Sync,
    Rename(String),
    /// Ouvre la confirmation (dans le chrome) ; rien n'est retiré ici.
    Revoke(String),
}

fn query_value(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        (k == key).then(|| form_decode(v))
    })
}

impl DeviceOp {
    /// `nyx://devices/<op>[?…]` → action. `None` si inconnue ou incomplète.
    pub fn parse(policy_url: &str) -> Option<Self> {
        let rest = policy_url.trim().strip_prefix("nyx://devices/")?;
        let op = rest.split(['?', '#']).next().unwrap_or("");
        match op {
            "pair" => Some(Self::Pair),
            "sync" => Some(Self::Sync),
            "rename" => {
                let name: String = query_value(rest, "name")?.trim().chars().take(64).collect();
                (!name.is_empty()).then_some(Self::Rename(name))
            }
            "revoke" => {
                let id = query_value(rest, "id")?;
                (!id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
                    .then_some(Self::Revoke(id))
            }
            _ => None,
        }
    }
}

/// État affiché dans Réglages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DevicesUi {
    pub devices: Vec<DeviceRow>,
    pub last: Option<LastSync>,
    pub port: Option<u16>,
    /// Service indisponible (port, coffre) : pourquoi.
    pub error: Option<String>,
    /// Synchronisation manuelle en cours.
    pub syncing: bool,
}

impl DevicesUi {
    /// Intègre un événement du service. `true` si l'affichage a changé.
    pub fn apply(&mut self, event: &Event) -> bool {
        let before = self.clone();
        match event {
            Event::Listening { port, error } => {
                self.port = *port;
                self.error.clone_from(error);
            }
            Event::Devices(rows) => self.devices.clone_from(rows),
            Event::LastSync(last) => self.last = Some(last.clone()),
            Event::Synced { last, .. } => {
                self.last = Some(last.clone());
                self.syncing = false;
            }
            Event::Failed(msg) => {
                self.error = Some(msg.clone());
                self.port = None;
            }
            Event::PairOffer { .. } | Event::PairCode { .. } | Event::PairDone(_) | Event::Notice(_) => {}
        }
        *self != before
    }

    /// Nom de cet appareil.
    pub fn my_name(&self) -> &str {
        self.devices.iter().find(|d| d.this_device).map_or("", |d| d.name.as_str())
    }

    /// Nom d'un autre appareil actif (confirmation de révocation).
    pub fn active_name(&self, id: &str) -> Option<&str> {
        self.devices.iter().find(|d| d.id == id && !d.this_device && !d.revoked).map(|d| d.name.as_str())
    }
}

fn format_date(ms: u64, fmt: &str) -> String {
    i64::try_from(ms)
        .ok()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|d| d.with_timezone(&chrono::Local).format(fmt).to_string())
        .unwrap_or_default()
}

/// Section « Appareils liés » de la page Réglages. Toute donnée est
/// échappée ; les boutons n'émettent que des actions `nyx://devices/…`.
pub fn section_html(ui: &DevicesUi) -> String {
    let others: String = ui
        .devices
        .iter()
        .filter(|d| !d.this_device && !d.revoked)
        .map(|d| {
            let action = format!("nyx://devices/revoke?id={}", form_encode(&d.id));
            format!(
                "<div class=\"row\"><div class=\"row-text\"><div class=\"row-label\">{}</div>\
                 <div class=\"row-desc\">Ajouté le {}</div></div>\
                 <button type=\"button\" class=\"preset\" data-action=\"{}\">Retirer</button></div>",
                html_escape(&d.name),
                html_escape(&format_date(d.added_at_ms, "%d/%m/%Y")),
                html_escape(&action),
            )
        })
        .collect();
    let others = if others.is_empty() {
        "<div class=\"row\"><span class=\"none\">Aucun appareil lié pour l'instant</span></div>".to_string()
    } else {
        others
    };
    let last = match &ui.last {
        _ if ui.syncing => "Synchronisation en cours…".to_string(),
        Some(l) => format!(
            "Dernière synchronisation : {} — {}",
            format_date(l.at_ms, "%d/%m %H:%M"),
            if l.message.is_empty() { if l.ok { "réussie" } else { "échec" } } else { l.message.as_str() }
        ),
        None => "Jamais synchronisé".to_string(),
    };
    let service = match (&ui.error, ui.port) {
        (Some(e), _) => format!("<div class=\"row-desc warn\">{}</div>", html_escape(e)),
        (None, Some(p)) => format!("<div class=\"row-desc\">Service actif sur le port {p} (réseau local).</div>"),
        (None, None) => "<div class=\"row-desc\">Démarrage du service…</div>".to_string(),
    };
    format!(
        "<section class=\"section\" id=\"devices\">\
         <div class=\"section-head\">Appareils liés</div><div class=\"divider\"></div>\
         <div class=\"stack\"><div class=\"row-text\"><div class=\"row-label\">Nom de cet appareil</div>\
         <div class=\"row-desc\">Affiché sur vos autres appareils lors des prochains appairages.</div></div>\
         <div class=\"inline\"><input type=\"text\" id=\"devname\" class=\"field\" maxlength=\"64\" value=\"{name}\">\
         <button type=\"button\" class=\"preset\" id=\"devname-save\">Renommer</button></div></div>\
         {others}\
         <div class=\"row\"><div class=\"row-text\"><div class=\"row-label\">{last}</div>{service}\
         <div class=\"row-desc\">Au premier usage, Windows demande d'autoriser Nyx : choisissez « Autoriser » \
         sur les réseaux privés (jamais publics). Mots de passe, favoris, historique et réglages sont chiffrés \
         de bout en bout, sans compte ni serveur.</div></div></div>\
         <div class=\"row\"><button type=\"button\" class=\"preset\" data-action=\"nyx://devices/pair\">Ajouter un téléphone</button>\
         <button type=\"button\" class=\"preset\" data-action=\"nyx://devices/sync\">Synchroniser maintenant</button></div>\
         </section>",
        name = html_escape(ui.my_name()),
        last = html_escape(&last),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, name: &str, me: bool, revoked: bool) -> DeviceRow {
        DeviceRow { id: id.into(), name: name.into(), added_at_ms: 1_700_000_000_000, revoked, this_device: me }
    }

    #[test]
    fn parse_actions() {
        assert_eq!(DeviceOp::parse("nyx://devices/pair"), Some(DeviceOp::Pair));
        assert_eq!(DeviceOp::parse("nyx://devices/sync?t=1"), Some(DeviceOp::Sync));
        assert_eq!(
            DeviceOp::parse("nyx://devices/rename?name=PC+du+salon%20%C3%A9"),
            Some(DeviceOp::Rename("PC du salon é".into()))
        );
        assert_eq!(DeviceOp::parse("nyx://devices/rename?name=++"), None);
        assert_eq!(DeviceOp::parse("nyx://devices/rename"), None);
        let long = format!("nyx://devices/rename?name={}", "a".repeat(200));
        assert_eq!(DeviceOp::parse(&long), Some(DeviceOp::Rename("a".repeat(64))));
        assert_eq!(DeviceOp::parse("nyx://devices/revoke?id=0123abcd"), Some(DeviceOp::Revoke("0123abcd".into())));
        assert_eq!(DeviceOp::parse("nyx://devices/revoke?id=a%22b"), None);
        assert_eq!(DeviceOp::parse("nyx://devices/revoke?id="), None);
        assert_eq!(DeviceOp::parse("nyx://devices/wipe"), None);
        assert_eq!(DeviceOp::parse("nyx://settings"), None);
    }

    #[test]
    fn etat_suivi_par_evenements() {
        let mut ui = DevicesUi::default();
        assert!(ui.apply(&Event::Listening { port: Some(47420), error: None }));
        assert!(!ui.apply(&Event::Listening { port: Some(47420), error: None }));
        assert!(ui.apply(&Event::Devices(vec![row("me", "PC", true, false), row("p", "Pixel", false, false)])));
        assert_eq!(ui.my_name(), "PC");
        assert_eq!(ui.active_name("p"), Some("Pixel"));
        assert_eq!(ui.active_name("me"), None, "jamais soi-même");
        ui.syncing = true;
        let last = LastSync { at_ms: 1, ok: true, message: "ok".into() };
        assert!(ui.apply(&Event::Synced { last: last.clone(), changed: false }));
        assert!(!ui.syncing);
        assert_eq!(ui.last, Some(last));
        assert!(!ui.apply(&Event::Notice("x".into())));
        assert!(ui.apply(&Event::Failed("Coffre inaccessible".into())));
        assert_eq!(ui.port, None);
    }

    #[test]
    fn section_echappee_et_complete() {
        let ui = DevicesUi {
            devices: vec![
                row("me", "PC <b>", true, false),
                row("p1", "Tél \"x\"", false, false),
                row("old", "Volé", false, true),
            ],
            port: Some(47421),
            ..DevicesUi::default()
        };
        let html = section_html(&ui);
        assert!(html.contains("value=\"PC &lt;b&gt;\""));
        assert!(html.contains("Tél &quot;x&quot;") || html.contains("Tél &#34;x&#34;"), "{html}");
        assert!(!html.contains("Volé"), "révoqués masqués");
        assert!(html.contains("data-action=\"nyx://devices/revoke?id=p1\""));
        assert!(html.contains("data-action=\"nyx://devices/pair\"") && html.contains("nyx://devices/sync"));
        assert!(html.contains("port 47421") && html.contains("Jamais synchronisé"));
        assert!(html.contains("réseaux privés"));
        let empty = section_html(&DevicesUi { error: Some("<x>".into()), ..DevicesUi::default() });
        assert!(empty.contains("Aucun appareil lié") && empty.contains("&lt;x&gt;"));
    }
}
