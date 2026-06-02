//! Mode sombre forcé sur les sites web.
//!
//! Technique « smart invert » : on inverse la page entière puis on ré-inverse
//! les médias pour qu'ils gardent leurs vraies couleurs. Injecté via
//! `WebKitUserContentManager`, en excluant les pages internes Nyx déjà sombres.

use webkit2gtk::{UserContentInjectedFrames, UserStyleLevel, UserStyleSheet};

const DARK_CSS: &str = r#"
html {
    filter: invert(1) hue-rotate(180deg) !important;
    background: #0e0e16 !important;
}
img, video, picture, canvas, svg, iframe, embed, object,
[style*="background-image"], [style*="background:url"], [style*="background: url"] {
    filter: invert(1) hue-rotate(180deg) !important;
}
"#;

/// Feuille de style « dark » prête à injecter. La block-list exclut les pages
/// internes (`file://`, `nyx://`) pour ne pas inverser notre propre UI.
pub fn stylesheet() -> UserStyleSheet {
    UserStyleSheet::new(
        DARK_CSS,
        UserContentInjectedFrames::AllFrames,
        UserStyleLevel::User,
        &[],
        &["file://*", "nyx://*"],
    )
}
