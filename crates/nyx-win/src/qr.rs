//! QR code de l'offre d'appairage (`nyxpair:v1:…`) → SVG autonome — pur,
//! testé sur l'hôte. Rendu local par la crate `qrcode` : aucune ressource
//! réseau, aucun CDN. Noir sur blanc avec zone de silence (4 modules) : les
//! lecteurs de QR des téléphones n'aiment ni l'inversion ni les couleurs.
//!
//! Le chrome l'affiche via `<img src="data:image/svg+xml,…">` (CSP du
//! chrome : `img-src 'self' data:`) : une image SVG n'exécute aucun script.

use qrcode::render::svg;
use qrcode::{EcLevel, QrCode};

/// Taille d'un module en unités SVG (le chrome redimensionne).
const MODULE: u32 = 8;

/// SVG du QR de `data`, ou `None` si trop long pour un QR.
pub fn svg(data: &str) -> Option<String> {
    let code = QrCode::with_error_correction_level(data.as_bytes(), EcLevel::M).ok()?;
    Some(
        code.render::<svg::Color>()
            .module_dimensions(MODULE, MODULE)
            .quiet_zone(true)
            .dark_color(svg::Color("#000000"))
            .light_color(svg::Color("#ffffff"))
            .build(),
    )
}

/// Côté (en modules, zone de silence comprise) du QR de `data`.
#[cfg(test)]
fn side_modules(data: &str) -> Option<u32> {
    let code = QrCode::with_error_correction_level(data.as_bytes(), EcLevel::M).ok()?;
    u32::try_from(code.width()).ok().map(|w| w + 8)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFFER: &str = "nyxpair:v1:AQ3q1bE2GQtxXwJ7Xc4bG0fM1wq3J9cX2p0kE8yQwq3J9cX2p0kE8yQwq3J9cX2p0kE8yQwq3J9cX2p0kE8yQ_-AAAAAGc8b2a5RrwECgAAAQ";

    #[test]
    fn svg_autonome_et_dimensionne() {
        let s = svg(OFFER).expect("svg");
        assert!(s.contains("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(s.ends_with("</svg>"));
        assert!(s.contains("fill=\"#000000\"") && s.contains("fill=\"#ffffff\""));
        // Aucune ressource externe ni script.
        let lower = s.to_ascii_lowercase();
        for bad in ["<script", "href", "<image", "url(", "onload"] {
            assert!(!lower.contains(bad), "{bad}");
        }
        assert_eq!(s.matches("http").count(), 1, "seulement le xmlns");
        // viewBox = côté en modules × taille d'un module.
        let side = side_modules(OFFER).expect("side") * MODULE;
        assert!(s.contains(&format!("viewBox=\"0 0 {side} {side}\"")), "{s:.300}");
        // Offre réelle (~100-130 car.) : QR petit, lisible de loin (≤ version 7).
        assert!(side_modules(OFFER).expect("side") <= 45 + 8);
    }

    #[test]
    fn deterministe_et_dependant_du_contenu() {
        assert_eq!(svg("nyxpair:v1:a"), svg("nyxpair:v1:a"));
        assert_ne!(svg("nyxpair:v1:a"), svg("nyxpair:v1:b"));
    }

    #[test]
    fn trop_long() {
        assert!(svg(&"x".repeat(10_000)).is_none());
    }
}
