//! Mode sombre forcé sur les sites web — approche `color-scheme`.
//!
//! Le smart-invert classique (`filter: invert(1) hue-rotate(180deg)`)
//! cassait tous les sites déjà sombres : il les transformait en blanc
//! crème éblouissant. Reproche user direct.
//!
//! Nouvelle approche : on FORCE `color-scheme: dark` au niveau :root,
//! ce qui pousse :
//!   - le navigateur à utiliser les UA styles dark (scrollbars, form
//!     controls, default text contrast) ;
//!   - les sites qui supportent `@media (prefers-color-scheme: dark)`
//!     à basculer eux-mêmes vers leur palette sombre native.
//!
//! Sites SANS support color-scheme restent en clair. Compromis assumé :
//! ne JAMAIS casser un site déjà sombre. Pour aller plus loin il
//! faudrait un moteur type Dark Reader (parsing dynamique du DOM) —
//! hors scope ici.

use webkit2gtk::{UserContentInjectedFrames, UserStyleLevel, UserStyleSheet};

const DARK_CSS: &str = r#"
:root {
    color-scheme: dark !important;
}
html {
    color-scheme: dark;
}
"#;

/// Feuille de style « dark » prête à injecter. La block-list exclut les
/// pages internes (`file://`, `nyx://`) pour ne pas perturber notre UI.
pub fn stylesheet() -> UserStyleSheet {
    UserStyleSheet::new(
        DARK_CSS,
        UserContentInjectedFrames::AllFrames,
        UserStyleLevel::User,
        &[],
        &["file://*", "nyx://*"],
    )
}
