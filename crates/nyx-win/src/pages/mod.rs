//! Pages internes dynamiques (`nyx://settings`, `nyx://bookmarks`,
//! `nyx://history`) — rendu pur, testé sur l'hôte. Favoris et historique
//! réutilisent les rendus partagés de `nyx_core::pages` ; les réglages ont un
//! gabarit Windows (options réellement implémentées ici + imports).

use nyx_core::import::chrome::ChromeProfile;
use nyx_core::pages::history::{self, Item};
use nyx_core::state::settings::AppSettings;
use nyx_core::theme;
use nyx_core::url::{form_encode, html_escape};
use vault::HistoryEntry;

use crate::import_chrome;
use crate::nav::HISTORY_CLEAR_URL;

const SETTINGS_TEMPLATE: &str = include_str!("settings.html");

/// Nombre de visites affichées.
pub const HISTORY_SHOWN: usize = 200;

/// Page historique (lien « Tout effacer » = action interne en navigation
/// principale, revalidée par `nav::decide`).
pub fn history_html(entries: &[HistoryEntry]) -> String {
    let items: Vec<Item> = entries
        .iter()
        .map(|e| Item {
            url: e.url.clone(),
            title: e.title.clone(),
            when: e.visited_at.with_timezone(&chrono::Local).format("%d/%m %H:%M").to_string(),
        })
        .collect();
    history::html(&items, HISTORY_CLEAR_URL)
}

/// Page réglages avec les valeurs courantes et les profils Chrome détectés.
pub fn settings_html(s: &AppSettings, profiles: &[ChromeProfile]) -> String {
    let chk = |on: bool| if on { " checked" } else { "" };
    let engine = s.search_engine.id();
    // Options du desktop sans interrupteur ici : renvoyées telles quelles,
    // sinon `apply_from_url` (case absente = false) les effacerait.
    let mut keep = String::new();
    for (name, on) in [("dark", s.dark_websites), ("rejectcookies", s.reject_cookies)] {
        if on {
            keep.push_str(&format!("<input type=\"hidden\" name=\"{name}\" value=\"true\">"));
        }
    }
    let chrome = if profiles.is_empty() {
        "<span class=\"none\">Aucun profil Chrome détecté</span>".to_string()
    } else {
        profiles
            .iter()
            .map(|p| {
                let action = format!("nyx://import/chrome?profile={}", form_encode(&import_chrome::dir_name(p)));
                format!(
                    "<button type=\"button\" class=\"preset\" data-action=\"{}\">{}</button>",
                    html_escape(&action),
                    html_escape(&p.label)
                )
            })
            .collect()
    };
    SETTINGS_TEMPLATE
        .replace("{{TOKENS}}", theme::ROOT)
        .replace("{{ENGINE_DDG}}", chk(engine == "ddg"))
        .replace("{{ENGINE_BRAVE}}", chk(engine == "brave"))
        .replace("{{ENGINE_ECOSIA}}", chk(engine == "ecosia"))
        .replace("{{HOME_URL}}", &html_escape(&s.home_url))
        .replace("{{ADBLOCK}}", chk(s.adblock_enabled))
        .replace("{{BLOCKAUTH}}", chk(s.block_third_party))
        .replace("{{SAVEPW}}", chk(s.save_passwords))
        .replace("{{PRIVATE}}", chk(s.private_mode))
        .replace("{{KEEP}}", &keep)
        .replace("{{CHROME_PROFILES}}", &chrome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn settings_reflect_values() {
        let s = AppSettings {
            home_url: "https://x.org/?a=\"><script>".into(),
            save_passwords: false,
            dark_websites: true,
            ..AppSettings::default()
        };
        let html = settings_html(&s, &[]);
        assert!(!html.contains("{{"), "placeholders");
        assert!(html.contains("value=\"ddg\" checked"));
        assert!(html.contains("name=\"adblock\" value=\"true\" checked"));
        assert!(!html.contains("name=\"savepw\" value=\"true\" checked"));
        assert!(html.contains("<input type=\"hidden\" name=\"dark\" value=\"true\">"));
        assert!(!html.contains("\"><script>"));
        assert!(html.contains("Aucun profil Chrome"));
        assert!(html.contains("nyx://import/capsule"));
    }

    #[test]
    fn chrome_profiles_are_escaped_buttons() {
        let p = ChromeProfile {
            label: "<i>Profile 1</i> (Chrome)".into(),
            dir: PathBuf::from("/u").join("Profile 1"),
            has_bookmarks: true,
            has_history: true,
        };
        let html = settings_html(&AppSettings::default(), &[p]);
        assert!(html.contains("data-action=\"nyx://import/chrome?profile=Profile+1\""), "{html}");
        assert!(html.contains("&lt;i&gt;Profile 1"));
    }

    #[test]
    fn history_links_to_internal_clear() {
        let html = history_html(&[]);
        assert!(html.contains("href=\"https://nyx.localhost/history/clear\""));
    }
}
