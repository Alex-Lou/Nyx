use gtk::cairo;
use gtk::gdk_pixbuf::InterpType;
use gtk::prelude::*;
use gtk::{IconSize, Image};
use webkit2gtk::{WebView, WebViewExt};

pub const FAVICON_PX: i32 = 16;

/// Branche `connect_favicon_notify` sur la WebView — appelé une fois à la
/// création du label d'onglet.
pub fn bind(img: &Image, wv: &WebView) {
    let i = img.clone();
    wv.connect_favicon_notify(move |wv| refresh(&i, wv));
}

/// Tentative de chargement ; repli sur l'icône générique si la surface est
/// absente, vide ou non-image.
pub fn refresh(img: &Image, wv: &WebView) {
    match try_load(wv) {
        Some(pb) => img.set_from_pixbuf(Some(&pb)),
        None     => set_fallback(img),
    }
}

pub fn set_fallback(img: &Image) {
    img.set_from_icon_name(Some("text-html-symbolic"), IconSize::Menu);
}

fn try_load(wv: &WebView) -> Option<gtk::gdk_pixbuf::Pixbuf> {
    let surf      = wv.favicon()?;
    let img_surf  = cairo::ImageSurface::try_from(surf).ok()?;
    let (w, h)    = (img_surf.width(), img_surf.height());
    if w <= 0 || h <= 0 { return None; }
    let pb = gtk::gdk::pixbuf_get_from_surface(&img_surf, 0, 0, w, h)?;
    pb.scale_simple(FAVICON_PX, FAVICON_PX, InterpType::Bilinear).or(Some(pb))
}
