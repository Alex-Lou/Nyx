//! Theming de l'inspecteur WebKit (best-effort).
//!
//! Le `WebInspector` héberge une `WebView` interne qui rend l'UI DevTools.
//! On y injecte une `UserStyleSheet` qui override les variables CSS
//! connues + quelques sélecteurs pragmatiques. Limites :
//!
//! - certaines parties de l'inspecteur utilisent du Shadow DOM côté WebKit
//!   (les User stylesheets ne traversent pas sans `::part`/`::slotted`) ;
//! - les noms de variables CSS évoluent version à version (WebKit 2.42
//!   ≠ 2.46) — la feuille `assets/inspector.css` tente une union.
//!
//! Résultat : le chrome top-level (toolbar, sidebar, console) passe en
//! dark Nyx ; certains panneaux internes restent au gris WebKit par
//! défaut. Mieux que rien, sans hack invasif.

use webkit2gtk::{
    UserContentInjectedFrames, UserContentManagerExt, UserStyleLevel,
    UserStyleSheet, WebInspector, WebInspectorExt, WebViewExt,
};

const NYX_CSS: &str = include_str!("../../../../assets/inspector.css");

/// Injecte la stylesheet Nyx sur la WebView de l'inspecteur.
/// À appeler après chaque `show()` — si la WebView n'est pas encore
/// disponible (premier appel pré-render), no-op silencieux, à
/// rappeler au prochain toggle.
///
/// Pas d'idempotence forcée : injecter plusieurs fois la même feuille
/// donne le même rendu visuel (les règles sont identiques). Le faible
/// surcoût mémoire est acceptable pour un outil que l'utilisateur
/// toggle manuellement.
pub fn theme(inspector: &WebInspector) {
    let Some(wv) = inspector.web_view() else { return; };
    let Some(ucm) = wv.user_content_manager() else { return; };
    let sheet = UserStyleSheet::new(
        NYX_CSS,
        UserContentInjectedFrames::AllFrames,
        UserStyleLevel::User,
        &[], &[],
    );
    ucm.add_style_sheet(&sheet);
}
