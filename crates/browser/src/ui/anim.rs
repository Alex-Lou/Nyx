use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib::ControlFlow;
use gtk::prelude::*;

/// Fondu d'apparition d'une fenêtre — opacité 0→1, discret et rapide (~220 ms,
/// cubic-out approximé). No-op sans compositeur (Xvfb headless) ; visible en
/// session réelle (WSLg, Linux natif).
pub fn fade_in(widget: &impl IsA<gtk::Widget>) {
    let w = widget.clone().upcast::<gtk::Widget>();
    w.set_opacity(0.0);

    const FRAMES: u32 = 14; // ~14 × 16 ms ≈ 224 ms
    let step = Rc::new(Cell::new(0u32));
    gtk::glib::timeout_add_local(Duration::from_millis(16), move || {
        let s = step.get() + 1;
        step.set(s);
        let p = (s as f64 / FRAMES as f64).min(1.0);
        // ease-out (1 - (1-p)^2) : démarre vif, finit en douceur.
        w.set_opacity(1.0 - (1.0 - p) * (1.0 - p));
        if s >= FRAMES { ControlFlow::Break } else { ControlFlow::Continue }
    });
}
