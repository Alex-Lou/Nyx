use gtk::gdk_pixbuf::InterpType;
use gtk::prelude::*;
use gtk::{cairo, Image};
use webkit2gtk::{WebView, WebViewExt};

use crate::ui::icons::{self, Icon};

pub const FAVICON_PX: i32 = 16;

/// Branche `connect_favicon_notify` — appelé une fois à la création du label.
pub fn bind(img: &Image, wv: &WebView) {
    let i = img.clone();
    wv.connect_favicon_notify(move |wv| refresh(&i, wv));
}

/// Charge la favicon ; repli sur l'icône générique si absente/non-image.
pub fn refresh(img: &Image, wv: &WebView) {
    match try_load(wv) {
        Some(pb) => img.set_from_pixbuf(Some(&pb)),
        None     => set_fallback(img),
    }
}

/// Repli : un croissant de lune Nyx dessiné (pas l'icône symbolique GTK).
pub fn set_fallback(img: &Image) {
    match moon_pixbuf() {
        Some(pb) => img.set_from_pixbuf(Some(&pb)),
        None => img.clear(),
    }
}

fn moon_pixbuf() -> Option<gtk::gdk_pixbuf::Pixbuf> {
    let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, FAVICON_PX, FAVICON_PX).ok()?;
    {
        let cr = cairo::Context::new(&surf).ok()?;
        let f = f64::from(FAVICON_PX);
        icons::draw(&cr, Icon::Moon, f, f, (0.482, 0.549, 0.871, 0.9)); // lune Nyx #7b8cde
    }
    surf.flush();
    let (w, h) = (surf.width(), surf.height());
    gtk::gdk::pixbuf_get_from_surface(&surf, 0, 0, w, h)
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
