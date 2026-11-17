//! Petits helpers GTK partagés par les popovers et barres de l'UI.

use gtk::prelude::*;
use gtk::{Box as GtkBox, Button, Orientation, Popover, PositionType, Widget, Window};

/// Bouton plat au style `nyx-nav-btn`.
pub fn nav_button(label: &str, tooltip: &str) -> Button {
    let btn = Button::with_label(label);
    btn.style_context().add_class("nyx-nav-btn");
    btn.set_relief(gtk::ReliefStyle::None);
    btn.set_tooltip_text(Some(tooltip));
    btn
}

/// Tronque à `max` caractères (sûr pour l'UTF-8) en ajoutant « … ».
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

/// Fenêtre toplevel contenant `w`, si elle existe.
pub fn toplevel_window(w: &impl IsA<Widget>) -> Option<Window> {
    w.toplevel().and_then(|t| t.downcast::<Window>().ok())
}

/// Popover ancré sous `anchor` + boîte verticale racine (marges 10 px).
pub fn popover_shell(anchor: &impl IsA<Widget>) -> (Popover, GtkBox) {
    let pop = Popover::new(Some(anchor));
    pop.set_position(PositionType::Bottom);
    pop.style_context().add_class("nyx-bm-pop");

    let root = GtkBox::new(Orientation::Vertical, 8);
    root.set_margin_top(10);
    root.set_margin_bottom(10);
    root.set_margin_start(10);
    root.set_margin_end(10);
    (pop, root)
}
