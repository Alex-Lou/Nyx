//! Système de toasts — notifications non-bloquantes en haut-droite.
//!
//! Architecture :
//!   - [`ToastHost`] héberge un `GtkBox` vertical attaché à un `Overlay`
//!     parent (typiquement la fenêtre principale).
//!   - [`new`] crée le host avec son widget prêt à packer.
//!   - [`ToastHost::push`] ajoute un toast, auto-dismiss après ~3.5 s.
//!   - Cap à 5 toasts actifs ; le plus ancien est évincé si on dépasse.
//!
//! 4 niveaux : `Info` (lune), `Success` (aurore), `Warning` (ambre),
//! `Error` (rose-rouge). Icônes Adwaita symboliques + bordure gauche
//! colorée. Tous les styles dans `assets/theme.css` sous `.nyx-toast*`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{Box as GtkBox, IconSize, Image, Label, Orientation};

const TOAST_DURATION_MS: u64 = 3500;
const MAX_TOASTS:        usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

pub type ToastHandle = Rc<ToastHost>;

pub struct ToastHost {
    container: GtkBox,
    active:    RefCell<VecDeque<GtkBox>>,
}

/// Crée un host vide. Le widget est destiné à être ajouté en overlay sur
/// la fenêtre principale via `Overlay::add_overlay`.
pub fn new() -> ToastHandle {
    let container = GtkBox::new(Orientation::Vertical, 8);
    container.style_context().add_class("nyx-toast-host");
    container.set_halign(gtk::Align::End);
    container.set_valign(gtk::Align::Start);
    container.set_margin_top(56);   // sous la navbar
    container.set_margin_end(20);
    Rc::new(ToastHost {
        container,
        active: RefCell::new(VecDeque::new()),
    })
}

impl ToastHost {
    /// Widget à packer dans un `Overlay`. Set `pass_through` sur l'overlay
    /// pour que les clics passent à travers les zones vides du host.
    pub fn widget(&self) -> &gtk::Widget {
        self.container.upcast_ref()
    }

    /// Affiche un toast. Auto-dismiss après [`TOAST_DURATION_MS`].
    /// Si la pile dépasse [`MAX_TOASTS`], le plus ancien est retiré.
    pub fn push(self: &Rc<Self>, level: ToastLevel, message: &str) {
        if self.active.borrow().len() >= MAX_TOASTS {
            if let Some(oldest) = self.active.borrow_mut().pop_front() {
                if oldest.parent().is_some() {
                    self.container.remove(&oldest);
                }
            }
        }

        let toast = build_toast(level, message);
        // pack_start : le plus récent en haut (au-dessus).
        self.container.pack_start(&toast, false, false, 0);
        toast.show_all();
        self.active.borrow_mut().push_back(toast.clone());

        let host = self.clone();
        let t = toast.clone();
        gtk::glib::timeout_add_local_once(
            Duration::from_millis(TOAST_DURATION_MS),
            move || host.dismiss(&t),
        );
    }

    fn dismiss(&self, widget: &GtkBox) {
        let mut active = self.active.borrow_mut();
        if let Some(pos) = active.iter().position(|w| w == widget) {
            active.remove(pos);
        }
        // Le widget peut avoir été retiré par l'éviction si MAX_TOASTS atteint.
        if widget.parent().is_some() {
            self.container.remove(widget);
        }
    }
}

// ─── Construction d'un toast ───────────────────────────────────────────────

fn build_toast(level: ToastLevel, message: &str) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 10);
    row.style_context().add_class("nyx-toast");
    row.style_context().add_class(class_for(level));
    row.set_margin_top(4);
    row.set_margin_bottom(4);

    let icon = Image::from_icon_name(Some(icon_for(level)), IconSize::Button);
    icon.style_context().add_class("nyx-toast-icon");
    row.pack_start(&icon, false, false, 0);

    let label = Label::new(Some(message));
    label.set_xalign(0.0);
    label.set_line_wrap(true);
    label.set_max_width_chars(48);
    label.style_context().add_class("nyx-toast-text");
    row.pack_start(&label, true, true, 0);
    row
}

fn class_for(level: ToastLevel) -> &'static str {
    match level {
        ToastLevel::Info    => "nyx-toast-info",
        ToastLevel::Success => "nyx-toast-success",
        ToastLevel::Warning => "nyx-toast-warning",
        ToastLevel::Error   => "nyx-toast-error",
    }
}

fn icon_for(level: ToastLevel) -> &'static str {
    match level {
        ToastLevel::Info    => "dialog-information-symbolic",
        ToastLevel::Success => "object-select-symbolic",
        ToastLevel::Warning => "dialog-warning-symbolic",
        ToastLevel::Error   => "dialog-error-symbolic",
    }
}
