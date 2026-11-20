//! Section « Mot de passe principal » de la page Réglages (GTK + Windows).
//!
//! Les boutons ne portent qu'une action interne (`data-action`,
//! `nyx://master/set|remove`, cf. `security::Verdict::MasterPassword`) :
//! aucune passphrase dans la page ni dans l'URL. Chaque coquille relie
//! `data-action` à son canal interne et ouvre une saisie native.

use crate::url::html_escape;

pub const SET_ACTION: &str = "nyx://master/set";
pub const REMOVE_ACTION: &str = "nyx://master/remove";

/// `session` : « Linux » ou « Windows » (rappel discret du mode courant).
pub fn section(has_master: bool, session: &str) -> String {
    let (desc, buttons) = if has_master {
        (
            "Actif : demandé à chaque lancement de Nyx. Irrécupérable s'il est oublié.".to_string(),
            format!(
                "<button type=\"button\" class=\"preset\" data-action=\"{SET_ACTION}\">Changer…</button>\
                 <button type=\"button\" class=\"preset\" data-action=\"{REMOVE_ACTION}\">Retirer</button>"
            ),
        )
    } else {
        (
            format!(
                "Protection : session {} — le coffre s'ouvre sans rien demander tant que \
                 ta session est ouverte. Optionnel.",
                html_escape(session)
            ),
            format!("<button type=\"button\" class=\"preset\" data-action=\"{SET_ACTION}\">Définir…</button>"),
        )
    };
    format!(
        "<section class=\"section\" id=\"master\">\
           <div class=\"section-head\">Mot de passe principal</div>\
           <div class=\"divider\"></div>\
           <div class=\"row\">\
             <div class=\"row-text\"><div class=\"row-label\">Mot de passe principal</div>\
               <div class=\"row-desc\">{desc}</div></div>\
             <div class=\"profiles\">{buttons}</div>\
           </div>\
         </section>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_master() {
        let html = section(false, "Linux");
        assert!(html.contains("Protection : session Linux"));
        assert!(html.contains("data-action=\"nyx://master/set\">Définir…"));
        assert!(!html.contains(REMOVE_ACTION));
    }

    #[test]
    fn with_master() {
        let html = section(true, "Windows");
        assert!(html.contains("Changer…") && html.contains("data-action=\"nyx://master/remove\">Retirer"));
        assert!(!html.contains("Protection : session"));
    }

    #[test]
    fn session_is_escaped() {
        assert!(section(false, "<b>").contains("&lt;b&gt;"));
    }
}
