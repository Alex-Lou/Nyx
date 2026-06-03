use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib::ControlFlow;
use gtk::prelude::*;

const FADE_FRAMES: u32 = 14;
const FRAME_MS:    u64 = 16; // ~14 × 16 ms ≈ 224 ms

/// Fondu d'apparition d'un widget — opacité 0→1, ease-out (1 - (1-p)²).
/// Discret et rapide. No-op sans compositeur (Xvfb headless) ; visible en
/// session réelle (WSLg, Linux natif).
pub fn fade_in(widget: &impl IsA<gtk::Widget>) {
    let w = widget.clone().upcast::<gtk::Widget>();
    w.set_opacity(0.0);

    let step = Rc::new(Cell::new(0u32));
    gtk::glib::timeout_add_local(Duration::from_millis(FRAME_MS), move || {
        let s = step.get() + 1;
        step.set(s);
        let p = (s as f64 / FADE_FRAMES as f64).min(1.0);
        w.set_opacity(1.0 - (1.0 - p) * (1.0 - p));
        if s >= FADE_FRAMES { ControlFlow::Break } else { ControlFlow::Continue }
    });
}

/// Fondu de disparition — opacité 1→0 puis callback `done` (typiquement
/// `popdown()` ou `hide()` du conteneur). Symétrique de [`fade_in`].
pub fn fade_out_then<F>(widget: &impl IsA<gtk::Widget>, done: F)
where
    F: FnOnce() + 'static,
{
    let w = widget.clone().upcast::<gtk::Widget>();
    let step = Rc::new(Cell::new(0u32));
    let done = Rc::new(Cell::new(Some(done)));
    gtk::glib::timeout_add_local(Duration::from_millis(FRAME_MS), move || {
        let s = step.get() + 1;
        step.set(s);
        let p = (s as f64 / FADE_FRAMES as f64).min(1.0);
        // ease-in (p²) : démarre doux, finit vif — symétrique de fade_in.
        w.set_opacity(1.0 - p * p);
        if s >= FADE_FRAMES {
            if let Some(cb) = done.take() { cb(); }
            ControlFlow::Break
        } else {
            ControlFlow::Continue
        }
    });
}
