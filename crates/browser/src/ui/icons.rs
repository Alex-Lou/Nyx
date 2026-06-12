//! Jeu d'icônes Nyx dessiné au vecteur (Cairo) — pas de loader SVG requis,
//! net à toute densité, et recoloré en direct selon l'état du bouton (hover,
//! actif) via le `StyleContext`. Remplace les glyphes texte « façon terminal ».

use gtk::cairo::{Context, LineCap, LineJoin};
use gtk::prelude::*;
use gtk::{Button, DrawingArea};

#[derive(Clone, Copy)]
pub enum Icon {
    Back, Forward, Reload, Home, Reader, Keys, Star, Plus, Settings,
    CloseTab, Moon,
}

const BTN_PX: i32 = 20;

/// Bouton de navigation : icône vectorielle recolorée selon l'état CSS.
pub fn button(icon: Icon, tooltip: &str) -> Button {
    let btn = Button::new();
    btn.style_context().add_class("nyx-nav-btn");
    btn.set_relief(gtk::ReliefStyle::None);
    btn.set_tooltip_text(Some(tooltip));

    let area = DrawingArea::new();
    area.set_size_request(BTN_PX, BTN_PX);

    // Couleur = foreground CSS du bouton dans son état courant → suit
    // automatiquement `.nyx-nav-btn:hover/:active` du thème.
    let b = btn.clone();
    area.connect_draw(move |area, cr| {
        let (w, h) = (area.allocated_width() as f64, area.allocated_height() as f64);
        let rgba = b.style_context().color(b.state_flags());
        draw(cr, icon, w, h, (rgba.red(), rgba.green(), rgba.blue(), rgba.alpha()));
        gtk::glib::Propagation::Proceed
    });

    // Redessine quand l'état du bouton change (hover/active) pour recolorer.
    let a = area.clone();
    btn.connect_state_flags_changed(move |_, _| a.queue_draw());

    btn.set_image(Some(&area));
    btn.set_always_show_image(true);
    btn
}

/// Couleur RGBA normalisée 0..1 pour le tracé d'icône.
pub type Rgba = (f64, f64, f64, f64);

/// Rend une icône dans un `Pixbuf` `px×px` — pour les emplacements qui
/// n'acceptent pas un widget (ex. icône d'une `Entry`).
pub fn pixbuf(icon: Icon, px: i32, rgba: Rgba) -> Option<gtk::gdk_pixbuf::Pixbuf> {
    let surf = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, px, px).ok()?;
    {
        let cr = Context::new(&surf).ok()?;
        draw(&cr, icon, f64::from(px), f64::from(px), rgba);
    }
    surf.flush();
    gtk::gdk::pixbuf_get_from_surface(&surf, 0, 0, px, px)
}

/// Dessine `icon` centré dans `w×h`, couleur (r,g,b,a) normalisée 0..1.
#[allow(clippy::many_single_char_names)]
pub fn draw(cr: &Context, icon: Icon, w: f64, h: f64, (r, g, b, a): Rgba) {
    cr.set_source_rgba(r, g, b, a);
    cr.set_line_width((w / 14.0).max(1.3));
    cr.set_line_cap(LineCap::Round);
    cr.set_line_join(LineJoin::Round);

    // Repère normalisé : tout est tracé dans une boîte 24×24 centrée.
    let s = w.min(h) / 24.0;
    let (cx, cy) = (w / 2.0, h / 2.0);
    let p = |x: f64, y: f64| ((x - 12.0) * s + cx, (y - 12.0) * s + cy);
    let m = |cr: &Context, x: f64, y: f64| { let (px, py) = p(x, y); cr.move_to(px, py); };
    let l = |cr: &Context, x: f64, y: f64| { let (px, py) = p(x, y); cr.line_to(px, py); };

    match icon {
        Icon::Back => { m(cr, 14.5, 6.0); l(cr, 8.5, 12.0); l(cr, 14.5, 18.0); cr.stroke().ok(); }
        Icon::Forward => { m(cr, 9.5, 6.0); l(cr, 15.5, 12.0); l(cr, 9.5, 18.0); cr.stroke().ok(); }
        Icon::Reload => {
            let (px, py) = p(12.0, 12.0);
            cr.arc(px, py, 6.0 * s, -2.4, 1.7);
            cr.stroke().ok();
            // pointe de flèche
            m(cr, 17.0, 5.0); l(cr, 17.6, 9.2); l(cr, 13.4, 8.0); cr.stroke().ok();
        }
        Icon::Home => {
            m(cr, 5.0, 12.0); l(cr, 12.0, 5.5); l(cr, 19.0, 12.0); cr.stroke().ok();
            m(cr, 7.0, 10.3); l(cr, 7.0, 18.5); l(cr, 17.0, 18.5); l(cr, 17.0, 10.3); cr.stroke().ok();
        }
        Icon::Reader => {
            for (i, y) in [7.5, 11.0, 14.5, 18.0].iter().enumerate() {
                m(cr, 6.0, *y);
                l(cr, if i == 3 { 14.0 } else { 18.0 }, *y);
            }
            cr.stroke().ok();
        }
        Icon::Keys => {
            let (px, py) = p(9.0, 9.0);
            cr.arc(px, py, 4.0 * s, 0.0, std::f64::consts::TAU);
            cr.stroke().ok();
            m(cr, 11.8, 11.8); l(cr, 18.5, 18.5); cr.stroke().ok();
            m(cr, 16.5, 16.5); l(cr, 18.5, 14.5); cr.stroke().ok();
        }
        Icon::Star => {
            star_path(cr, &p, false);
            cr.stroke().ok();
        }
        Icon::Plus => {
            m(cr, 12.0, 6.0); l(cr, 12.0, 18.0);
            m(cr, 6.0, 12.0); l(cr, 18.0, 12.0);
            cr.stroke().ok();
        }
        Icon::Settings => {
            let (px, py) = p(12.0, 12.0);
            cr.arc(px, py, 3.0 * s, 0.0, std::f64::consts::TAU);
            cr.stroke().ok();
            for k in 0..6 {
                let ang = k as f64 * std::f64::consts::PI / 3.0;
                let (x1, y1) = (12.0 + ang.cos() * 6.0, 12.0 + ang.sin() * 6.0);
                let (x2, y2) = (12.0 + ang.cos() * 9.0, 12.0 + ang.sin() * 9.0);
                m(cr, x1, y1); l(cr, x2, y2);
            }
            cr.stroke().ok();
        }
        Icon::CloseTab => {
            m(cr, 7.5, 7.5); l(cr, 16.5, 16.5);
            m(cr, 16.5, 7.5); l(cr, 7.5, 16.5);
            cr.stroke().ok();
        }
        Icon::Moon => {
            // Croissant : disque plein évidé — l'identité Nyx.
            let (px, py) = p(13.0, 12.0);
            cr.arc(px, py, 7.5 * s, 1.9, 4.6);
            let (qx, qy) = p(15.5, 12.0);
            cr.arc_negative(qx, qy, 7.0 * s, 4.6, 1.9);
            cr.close_path();
            cr.fill().ok();
        }
    }
}

fn star_path(cr: &Context, p: &impl Fn(f64, f64) -> (f64, f64), _fill: bool) {
    let (cx, cy) = (12.0, 12.5);
    for k in 0..=10 {
        let ang = -std::f64::consts::FRAC_PI_2 + k as f64 * std::f64::consts::PI / 5.0;
        let rad = if k % 2 == 0 { 7.5 } else { 3.1 };
        let (x, y) = (cx + ang.cos() * rad, cy + ang.sin() * rad);
        let (px, py) = p(x, y);
        if k == 0 { cr.move_to(px, py); } else { cr.line_to(px, py); }
    }
    cr.close_path();
}

