//! Palette Nyx canonique pour les pages web internes — **source unique**,
//! injectée dans chaque page via le placeholder `{{TOKENS}}`.
//!
//! C'est le miroir web des `@define-color` de `assets/theme.css` : les deux
//! moteurs CSS (GTK vs WebKit) ne peuvent pas partager un fichier, mais les
//! valeurs vivent ici en un seul endroit côté web. Les pages référencent
//! `var(--nyx-*)` ; leurs réglages bespoke (alphas, nuances locales) restent
//! locaux et assumés.

pub const ROOT: &str = "<style>:root{\
--nyx-void:#06060f;--nyx-deep:#0a0a18;--nyx-surface:#0f0f22;--nyx-raised:#141430;\
--nyx-border:#1e1e40;--nyx-star:#c8d6ff;--nyx-moon:#7b8cde;--nyx-nebula:#9d6ef7;\
--nyx-aurora:#4af2c8;--nyx-twilight:#ff7eb3;--nyx-text:#dde4ff;--nyx-dim:#7078a8;\
--nyx-ghost:#3d4466}</style>";
