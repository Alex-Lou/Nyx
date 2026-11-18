//! Écran d'accueil animé du premier lancement : ciel étoilé, halo de
//! nébuleuse, croissant qui se lève, vague qui se trace, puis le mot « Nyx ».
//!
//! Dessin Cairo piloté par l'horloge de frame GTK (`add_tick_callback`) →
//! synchronisé sur l'écran, sans WebKit ni timer. Bloquant (boucle locale)
//! pour s'insérer avant la création du coffre ; clic ou touche = passer.

use std::cell::Cell;
use std::f64::consts::PI;
use std::rc::Rc;

use gtk::cairo::{Context, LinearGradient, RadialGradient};
use gtk::glib::{self, ControlFlow};
use gtk::prelude::*;

const W: i32 = 560;
const H: i32 = 380;
/// Durée totale (s), fondu de sortie compris.
const TOTAL: f64 = 3.0;
const FADE_OUT: f64 = 0.45;

// Palette Nyx (tokens du thème).
const BG: (f64, f64, f64) = (0.020, 0.027, 0.043);      // #05070b
const NEBULA: (f64, f64, f64) = (0.616, 0.431, 0.969);  // #9d6ef7
const AURORA: (f64, f64, f64) = (0.290, 0.949, 0.784);  // #4af2c8
const STAR: (f64, f64, f64) = (0.784, 0.839, 1.0);      // #c8d6ff
const MOON: (f64, f64, f64) = (0.910, 0.835, 0.600);    // or pâle du logo

/// Joue l'animation puis rend la main. Ne fait rien si les animations GTK
/// sont désactivées (préférence d'accessibilité).
pub fn play() {
    let animations = gtk::Settings::default()
        .map(|s| s.is_gtk_enable_animations())
        .unwrap_or(true);
    if !animations {
        return;
    }

    let win = gtk::Window::builder()
        .title("Nyx")
        .decorated(false)
        .resizable(false)
        .default_width(W)
        .default_height(H)
        .window_position(gtk::WindowPosition::CenterAlways)
        .type_hint(gtk::gdk::WindowTypeHint::Splashscreen)
        .app_paintable(true)
        .build();
    let area = gtk::DrawingArea::new();
    win.add(&area);

    let stars = Rc::new(starfield(70));
    let elapsed = Rc::new(Cell::new(0.0f64));
    {
        let (stars, elapsed) = (stars.clone(), elapsed.clone());
        area.connect_draw(move |a, cr| {
            let (w, h) = (a.allocated_width() as f64, a.allocated_height() as f64);
            draw(cr, w, h, elapsed.get(), &stars);
            glib::Propagation::Stop
        });
    }

    let main_loop = glib::MainLoop::new(None, false);
    let done = Rc::new(Cell::new(false));
    let finish = {
        let (main_loop, done) = (main_loop.clone(), done.clone());
        move || {
            if !done.replace(true) {
                main_loop.quit();
            }
        }
    };

    // Passer : clic, touche, ou fermeture de la fenêtre.
    win.add_events(gtk::gdk::EventMask::BUTTON_PRESS_MASK | gtk::gdk::EventMask::KEY_PRESS_MASK);
    {
        let f = finish.clone();
        win.connect_button_press_event(move |_, _| { f(); glib::Propagation::Stop });
        let f = finish.clone();
        win.connect_key_press_event(move |_, _| { f(); glib::Propagation::Stop });
        let f = finish.clone();
        win.connect_delete_event(move |_, _| { f(); glib::Propagation::Stop });
    }

    let start = Rc::new(Cell::new(None::<i64>));
    {
        let f = finish.clone();
        area.add_tick_callback(move |a, clock| {
            let now = clock.frame_time(); // µs
            let t0 = start.get().unwrap_or(now);
            start.set(Some(t0));
            let t = (now - t0) as f64 / 1e6;
            elapsed.set(t);
            a.queue_draw();
            if t >= TOTAL {
                f();
                return ControlFlow::Break;
            }
            ControlFlow::Continue
        });
    }

    win.show_all();
    main_loop.run();
    win.close();
}

/// Étoiles déterministes (LCG) : (x, y, rayon, phase) en coordonnées 0..1.
fn starfield(n: usize) -> Vec<(f64, f64, f64, f64)> {
    let mut seed: u32 = 0x9E37_79B9;
    let mut rnd = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        f64::from(seed >> 8) / f64::from(1u32 << 24)
    };
    (0..n).map(|_| (rnd(), rnd(), 0.4 + rnd() * 1.1, rnd() * 2.0 * PI)).collect()
}

/// Progression 0..1 de `t` sur la fenêtre [a, b].
fn span(t: f64, a: f64, b: f64) -> f64 { ((t - a) / (b - a)).clamp(0.0, 1.0) }
fn ease_out(p: f64) -> f64 { 1.0 - (1.0 - p).powi(3) }

fn draw(cr: &Context, w: f64, h: f64, t: f64, stars: &[(f64, f64, f64, f64)]) {
    // Alpha global : fondu de sortie (fiable même sans compositeur).
    let g = 1.0 - span(t, TOTAL - FADE_OUT, TOTAL);
    cr.set_source_rgb(BG.0, BG.1, BG.2);
    let _ = cr.paint();

    let (cx, cy) = (w * 0.5, h * 0.40);

    // Halo de nébuleuse qui s'épanouit.
    let bloom = ease_out(span(t, 0.0, 1.4));
    let halo = RadialGradient::new(cx, cy, 0.0, cx, cy, 70.0 + 150.0 * bloom);
    halo.add_color_stop_rgba(0.0, NEBULA.0, NEBULA.1, NEBULA.2, 0.30 * bloom * g);
    halo.add_color_stop_rgba(0.55, AURORA.0 * 0.4, AURORA.1 * 0.4, AURORA.2 * 0.6, 0.08 * bloom * g);
    halo.add_color_stop_rgba(1.0, BG.0, BG.1, BG.2, 0.0);
    let _ = cr.set_source(&halo);
    let _ = cr.paint();

    // Étoiles : apparition échelonnée + scintillement doux.
    let sky = span(t, 0.1, 1.2);
    for &(x, y, r, ph) in stars {
        let a = ((sky * 1.6 - x * 0.6).clamp(0.0, 1.0)) * (0.55 + 0.45 * (t * 2.2 + ph).sin()) * g;
        if a <= 0.01 { continue; }
        cr.set_source_rgba(STAR.0, STAR.1, STAR.2, a * 0.85);
        cr.arc(x * w, y * h, r, 0.0, 2.0 * PI);
        let _ = cr.fill();
    }

    draw_moon(cr, cx, cy, t, g);
    draw_wave(cr, cx, cy, t, g);
    draw_wordmark(cr, w, h, t, g);
}

/// Croissant qui se lève (montée + agrandissement) avec sa lueur.
fn draw_moon(cr: &Context, cx: f64, cy: f64, t: f64, g: f64) {
    let p = ease_out(span(t, 0.25, 1.35));
    if p <= 0.0 { return; }
    let r = 46.0 * (0.85 + 0.15 * p);
    let (mx, my) = (cx - 18.0, cy + 14.0 * (1.0 - p));

    let glow = RadialGradient::new(mx, my, r * 0.4, mx, my, r * 2.2);
    glow.add_color_stop_rgba(0.0, MOON.0, MOON.1, MOON.2, 0.22 * p * g);
    glow.add_color_stop_rgba(1.0, MOON.0, MOON.1, MOON.2, 0.0);
    let _ = cr.set_source(&glow);
    let _ = cr.paint();

    // Croissant = disque principal (clip) moins disque décalé (even-odd).
    let _ = cr.save();
    cr.arc(mx, my, r, 0.0, 2.0 * PI);
    cr.clip();
    cr.set_fill_rule(gtk::cairo::FillRule::EvenOdd);
    cr.arc(mx, my, r, 0.0, 2.0 * PI);
    cr.new_sub_path();
    cr.arc(mx + r * 0.42, my - r * 0.22, r * 0.86, 0.0, 2.0 * PI);
    let shade = LinearGradient::new(mx - r, my - r, mx + r, my + r);
    shade.add_color_stop_rgba(0.0, 1.0, 0.95, 0.80, p * g);
    shade.add_color_stop_rgba(1.0, MOON.0 * 0.75, MOON.1 * 0.7, MOON.2 * 0.6, p * g);
    let _ = cr.set_source(&shade);
    let _ = cr.fill();
    let _ = cr.restore();
}

/// Vague du logo, tracée progressivement (effet « trim path » via tirets).
fn draw_wave(cr: &Context, cx: f64, cy: f64, t: f64, g: f64) {
    let p = ease_out(span(t, 0.65, 1.75));
    if p <= 0.0 { return; }
    let path = |cr: &Context| {
        cr.move_to(cx - 120.0, cy + 58.0);
        cr.curve_to(cx - 40.0, cy + 100.0, cx + 40.0, cy + 30.0, cx + 70.0, cy - 8.0);
        cr.curve_to(cx + 88.0, cy - 30.0, cx + 112.0, cy - 30.0, cx + 124.0, cy - 14.0);
    };
    path(cr);
    let len = path_length(cr);
    cr.new_path();

    let grad = LinearGradient::new(cx - 120.0, 0.0, cx + 124.0, 0.0);
    grad.add_color_stop_rgba(0.0, 0.16, 0.30, 0.62, 0.0);
    grad.add_color_stop_rgba(0.45, 0.20, 0.52, 0.86, 0.9 * g);
    grad.add_color_stop_rgba(1.0, AURORA.0, AURORA.1, AURORA.2, g);
    for (width, alpha) in [(14.0, 0.18), (5.0, 1.0)] {
        path(cr);
        cr.set_line_width(width);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        cr.set_dash(&[len * p, len], 0.0);
        cr.push_group();
        let _ = cr.set_source(&grad);
        let _ = cr.stroke();
        let _ = cr.pop_group_to_source();
        let _ = cr.paint_with_alpha(alpha);
    }
    cr.set_dash(&[], 0.0);
}

/// Longueur du chemin courant (aplati en segments).
fn path_length(cr: &Context) -> f64 {
    let Ok(flat) = cr.copy_path_flat() else { return 0.0 };
    let (mut len, mut last, mut first) = (0.0, (0.0, 0.0), (0.0, 0.0));
    for seg in flat.iter() {
        match seg {
            gtk::cairo::PathSegment::MoveTo(p) => { last = p; first = p; }
            gtk::cairo::PathSegment::LineTo(p) => { len += (p.0 - last.0).hypot(p.1 - last.1); last = p; }
            gtk::cairo::PathSegment::ClosePath => { len += (first.0 - last.0).hypot(first.1 - last.1); last = first; }
            gtk::cairo::PathSegment::CurveTo(..) => {}
        }
    }
    len
}

/// « Nyx » qui monte en s'espaçant, puis la devise.
fn draw_wordmark(cr: &Context, w: f64, h: f64, t: f64, g: f64) {
    let p = ease_out(span(t, 1.15, 2.05));
    if p <= 0.0 { return; }
    cr.select_font_face("Inter", gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Normal);
    cr.set_font_size(54.0);
    let spacing = 10.0 + 12.0 * p;
    let letters = ["N", "y", "x"];
    let widths: Vec<f64> = letters.iter()
        .map(|l| cr.text_extents(l).map(|e| e.x_advance()).unwrap_or(30.0))
        .collect();
    let total = widths.iter().sum::<f64>() + spacing * 2.0;
    let (mut x, y) = ((w - total) / 2.0, h * 0.80 + 12.0 * (1.0 - p));

    let grad = LinearGradient::new(x, 0.0, x + total, 0.0);
    grad.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, p * g);
    grad.add_color_stop_rgba(0.5, STAR.0, STAR.1, STAR.2, p * g);
    grad.add_color_stop_rgba(1.0, NEBULA.0, NEBULA.1, NEBULA.2, p * g);
    let _ = cr.set_source(&grad);
    for (l, lw) in letters.iter().zip(&widths) {
        cr.move_to(x, y);
        let _ = cr.show_text(l);
        x += lw + spacing;
    }

    let q = ease_out(span(t, 1.6, 2.3));
    if q > 0.0 {
        cr.set_font_size(11.0);
        let tag = "UNE LUMIÈRE PLUS DOUCE";
        let tw = cr.text_extents(tag).map(|e| e.x_advance()).unwrap_or(150.0);
        cr.set_source_rgba(0.44, 0.47, 0.66, 0.85 * q * g);
        cr.move_to((w - tw) / 2.0, y + 30.0);
        let _ = cr.show_text(tag);
    }
}
