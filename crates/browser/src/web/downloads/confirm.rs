//! Confirmation dialog Ask / AskDanger pour le post-flight du bridge.
//!
//! Délègue à `crate::ui::dialog::show` (Nyx-thémé, centré, custom CSS).
//! 'Conserver' / 'Jeter' au lieu des verbes ambigus de MessageDialog.

use nyx_core::download_policy::{Kind, Verdict};

use crate::ui::dialog::{self, ConfirmLevel, ConfirmParams};

pub fn show<F: FnOnce(bool) + 'static>(
    verdict: Verdict, filename: String, kind: Kind, callback: F,
) {
    let level = match verdict {
        Verdict::AskDanger => ConfirmLevel::Danger,
        _                  => ConfirmLevel::Normal,
    };
    let title = match verdict {
        Verdict::AskDanger => format!("Téléchargement à risque : {filename}"),
        _                  => format!("Conserver « {filename} » ?"),
    };
    let body = body_for_kind(kind);

    dialog::show(
        ConfirmParams {
            parent:  None,
            level,
            title,
            body:    body.into(),
            accept:  "Conserver".into(),
            cancel:  "Jeter".into(),
        },
        callback,
    );
}

fn body_for_kind(kind: Kind) -> &'static str {
    match kind {
        Kind::Executable => "Ce fichier est un exécutable. Ouvrir un binaire \
                             téléchargé peut compromettre votre système.",
        Kind::MacroDoc   => "Ce document contient des macros qui peuvent \
                             exécuter du code arbitraire à l'ouverture.",
        Kind::Archive    => "Cette archive peut contenir n'importe quel type \
                             de fichier. Vérifiez son contenu avant extraction.",
        Kind::ActiveDoc  => "Ce document peut embarquer du contenu actif \
                             (script, redirection).",
        _ => "Type de fichier inconnu — origine et nom incohérents.",
    }
}
