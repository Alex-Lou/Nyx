use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib::ControlFlow;
use gtk::prelude::*;

/// Fondu d'apparition d'un widget — opacité 0→1, ease-out (1 - (1-p)²).
/// Discret et rapide. No-op sans compositeur (Xvfb headless) ; visible en
/// session réelle (WSLg, Linux natif).
///
/// Pas de variante `fade_out` pour l'instant : GTK3 popdown les popovers
/// instantanément, donc un fondu sortant serait invisible. Ce helper
/// rejoindra ce module quand on aura un conteneur dont on contrôle la
/// destruction (fenêtre top-level dédiée).
pub fn fade_in(widget: &impl IsA<gtk::Widget>) {
    const FRAMES: u32 = 14; // ~14 × 16 ms ≈ 224 ms
    let w = widget.clone().upcast::<gtk::Widget>();
    w.set_opacity(0.0);

    let step = Rc::new(Cell::new(0u32));
    gtk::glib::timeout_add_local(Duration::from_millis(16), move || {
        let s = step.get() + 1;
        step.set(s);
        let p = (s as f64 / FRAMES as f64).min(1.0);
        w.set_opacity(1.0 - (1.0 - p) * (1.0 - p));
        if s >= FRAMES { ControlFlow::Break } else { ControlFlow::Continue }
    });
}
