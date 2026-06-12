//! Popover shelf — anchor sur le bouton ⤓, fade in/out via Revealer crossfade.
//!
//! Structure interne :
//!   ```
//!   Popover
//!   └─ Revealer (Crossfade, 220 ms)
//!      └─ Vertical Box (root)
//!         ├─ header (titre + clear + open dir + ⋮)
//!         └─ ScrolledWindow
//!            └─ ListBox (lignes ou « Aucun téléchargement »)
//!   ```
//!
//! Le `Revealer` gère le fade-in à l'ouverture ; GTK gère le fade-out
//! par défaut (zoom-scale sur popdown). `refresh()` est appelé à chaque
//! show + après chaque mutation locale (clear / remove).

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib::ControlFlow;
use gtk::prelude::*;
use gtk::{
    Box as GtkBox, Label, ListBox, Orientation, Popover, PositionType, Revealer,
    RevealerTransitionType, ScrolledWindow, Window,
};

use crate::state::downloads::DownloadsHandle;
use crate::state::settings::Settings;
use crate::ui::anim;
use crate::ui::toast::ToastHandle;

use super::{header, row};

const FADE_MS:         u32 = 220;
const POPOVER_WIDTH:   i32 = 360;
const LIST_MIN_HEIGHT: i32 = 280;

/// Callback type unique partagé header ↔ rows ↔ popover.show.
pub(super) type Refresh = Rc<dyn Fn() + 'static>;

/// Construit le popover et l'attache au bouton.
pub fn build(
    anchor: &gtk::Button,
    handle: DownloadsHandle,
    settings: Settings,
    toaster: ToastHandle,
) -> Popover {
    let pop = Popover::new(Some(anchor));
    pop.set_position(PositionType::Bottom);
    pop.style_context().add_class("nyx-dl-pop");

    let revealer = Revealer::new();
    revealer.set_transition_type(RevealerTransitionType::Crossfade);
    revealer.set_transition_duration(FADE_MS);

    let root = GtkBox::new(Orientation::Vertical, 8);
    root.set_size_request(POPOVER_WIDTH, -1);
    root.set_margin_top(10);
    root.set_margin_bottom(10);
    root.set_margin_start(10);
    root.set_margin_end(10);

    let list = ListBox::new();
    list.style_context().add_class("nyx-dl-list");
    list.set_selection_mode(gtk::SelectionMode::None);

    let scroll = ScrolledWindow::builder()
        .min_content_height(LIST_MIN_HEIGHT)
        .build();
    scroll.add(&list);

    let parent_win = anchor.toplevel().and_then(|t| t.downcast::<Window>().ok());

    // Refresh : Rc<dyn Fn()>, partagé. Tous les chemins de mutation locale
    // (clear all, suppression de ligne, ouverture du popover) l'appellent.
    let refresh: Refresh = {
        let (list, handle, settings, parent) =
            (list.clone(), handle.clone(), settings.clone(), parent_win.clone());
        Rc::new(move || rerender(&list, &handle, &settings, parent.clone()))
    };

    // Closure de fermeture du shelf — propagée jusqu'au menu ⋮ pour que
    // « Choisir le dossier… » puisse fermer le shelf AVANT d'ouvrir le
    // FileChooser (sinon le popover modal bloque le dialog sur certains
    // compositeurs / WSL).
    let close_shelf: Rc<dyn Fn()> = {
        let p = pop.clone();
        Rc::new(move || p.popdown())
    };

    let head = header::build(
        parent_win, handle, settings, refresh.clone(),
        close_shelf, toaster,
    );

    root.pack_start(&head, false, false, 0);
    root.pack_start(&scroll, true, true, 0);

    revealer.add(&root);
    pop.add(&revealer);
    root.show_all();
    revealer.show();

    wire_show_close(&pop, &revealer, &root, refresh);
    pop
}

/// Fade-in à chaque show, fade-out best-effort à chaque close, +
/// polling timer 400 ms qui rafraîchit la liste pendant que le popover
/// est ouvert (live progress sur les DL InProgress).
fn wire_show_close(
    pop: &Popover, revealer: &Revealer, root: &GtkBox, refresh: Refresh,
) {
    // Flag d'activité du timer (cellule pour pouvoir mutate depuis show/close).
    let timer_alive = Rc::new(Cell::new(false));

    // Fondu doux complémentaire au Revealer.
    {
        let root = root.clone();
        pop.connect_show(move |_| anim::fade_in(&root));
    }
    // Show : reveal + initial refresh + start polling.
    {
        let r = revealer.clone();
        let refresh = refresh.clone();
        let timer_alive = timer_alive.clone();
        pop.connect_show(move |_| {
            r.set_reveal_child(false);
            refresh();
            let r2 = r.clone();
            gtk::glib::idle_add_local_once(move || r2.set_reveal_child(true));

            // Démarre le polling si pas déjà actif.
            if !timer_alive.get() {
                timer_alive.set(true);
                let ta = timer_alive.clone();
                let rf = refresh.clone();
                gtk::glib::timeout_add_local(Duration::from_millis(400), move || {
                    if ta.get() { rf(); ControlFlow::Continue }
                    else        { ControlFlow::Break }
                });
            }
        });
    }
    // Close : stoppe le polling + reset le revealer.
    {
        let r = revealer.clone();
        let timer_alive = timer_alive;
        pop.connect_closed(move |_| {
            timer_alive.set(false);
            r.set_reveal_child(false);
        });
    }
}

/// Re-render la liste depuis le store. Appelée à chaque mutation locale.
fn rerender(
    list: &ListBox,
    handle: &DownloadsHandle,
    settings: &Settings,
    parent: Option<Window>,
) {
    for child in list.children() {
        list.remove(&child);
    }
    let store = handle.borrow();
    if store.is_empty() {
        let empty = Label::new(Some("Aucun téléchargement"));
        empty.style_context().add_class("nyx-dl-empty");
        empty.set_margin_top(24);
        empty.set_margin_bottom(24);
        list.add(&empty);
        list.show_all();
        return;
    }
    // Snapshot pour libérer le borrow avant de connecter les handlers
    // qui muteront le store.
    let snapshot: Vec<_> = store.entries().to_vec();
    drop(store);

    // Closure de refresh pour les rows (suppression).
    let row_refresh: Refresh = {
        let (list, handle, settings, parent) =
            (list.clone(), handle.clone(), settings.clone(), parent.clone());
        Rc::new(move || rerender(&list, &handle, &settings, parent.clone()))
    };

    for entry in &snapshot {
        let r = row::build(
            entry, handle.clone(), settings.clone(),
            parent.clone(), row_refresh.clone(),
        );
        list.add(&r);
    }
    list.show_all();
}
