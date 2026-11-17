use gtk::gdk_pixbuf::InterpType;
use gtk::prelude::*;
use gtk::{cairo, IconSize, Image};
use webkit2gtk::{WebView, WebViewExt};

pub const FAVICON_PX: i32 = 16;

/// Branche `connect_favicon_notify` — appelé une fois à la création du label.
pub fn bind(img: &Image, wv: &WebView) {
    // Weak : le handler vit sur le WebView, il ne doit pas retenir l'image
    // (qui appartient au label d'onglet, lui-même lié au notebook).
    let i = img.downgrade();
    wv.connect_favicon_notify(move |wv| {
        if let Some(i) = i.upgrade() { refresh(&i, wv); }
    });
}

/// Charge la favicon ; repli sur l'icône générique si absente/non-image.
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
    let surf     = wv.favicon()?;
    let img_surf = cairo::ImageSurface::try_from(surf).ok()?;
    let (w, h)   = (img_surf.width(), img_surf.height());
    if w <= 0 || h <= 0 {
        return None;
    }
    let pb = gtk::gdk::pixbuf_get_from_surface(&img_surf, 0, 0, w, h)?;
    pb.scale_simple(FAVICON_PX, FAVICON_PX, InterpType::Bilinear).or(Some(pb))
}
