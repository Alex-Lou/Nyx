use std::rc::Rc;

use gtk::gdk::keys::constants as key;
use gtk::gdk::ModifierType;
use gtk::glib::translate::IntoGlib;
use gtk::prelude::*;
use gtk::{AccelFlags, AccelGroup, ApplicationWindow, Entry};
use vault::Vault;
use webkit2gtk::WebViewExt;

use crate::state::bookmarks::Bookmarks;
use crate::ui::findbar::FindBar;
use crate::ui::navbar;
use crate::ui::tabs::TabBar;

/// Ctrl+T nouvel onglet · Ctrl+W fermer · Ctrl+L adresse · Ctrl+R recharger
/// Ctrl+Tab suivant · Ctrl+, paramètres · Ctrl+D favori · Ctrl+F rechercher
/// Ctrl+H historique · Ctrl + / − / 0 zoom
pub fn wire(
    window: &ApplicationWindow,
    tabs: &TabBar,
    url_bar: &Entry,
    bm: &Bookmarks,
    vault: &Rc<Vault>,
    findbar: &FindBar,
) {
    let accel = AccelGroup::new();
    window.add_accel_group(&accel);
    let ctrl  = ModifierType::CONTROL_MASK;
    let flags = AccelFlags::VISIBLE;

    bind(&accel, key::l.into_glib(), ctrl, flags, {
        let ub = url_bar.clone();
        move |_| { ub.grab_focus(); ub.select_region(0, -1); true }
    });
    bind(&accel, key::t.into_glib(), ctrl, flags, {
        let t = tabs.clone(); move |_| { t.open_new_tab(); true }
    });
    bind(&accel, key::w.into_glib(), ctrl, flags, {
        let t = tabs.clone(); move |_| { t.close_current(); true }
    });
    bind(&accel, key::r.into_glib(), ctrl, flags, {
        let t = tabs.clone(); move |_| { t.with_current(|wv| wv.reload()); true }
    });
    bind(&accel, key::Tab.into_glib(), ctrl, flags, {
        let t = tabs.clone(); move |_| { t.next_tab(); true }
    });
    bind(&accel, key::comma.into_glib(), ctrl, flags, {
        let t = tabs.clone(); move |_| { t.open_settings(); true }
    });
    bind(&accel, key::d.into_glib(), ctrl, flags, {
        let t = tabs.clone(); let b = bm.clone(); let v = vault.clone(); let ub = url_bar.clone();
        move |_| { navbar::bookmark_current(&t, &b, &v, &ub); true }
    });

    // Ctrl+F → recherche dans la page (Sprint 4.2)
    bind(&accel, key::f.into_glib(), ctrl, flags, {
        let fb = findbar.clone(); move |_| { fb.open(); true }
    });

    // Ctrl+H → historique (page interne, données du vault)
    bind(&accel, key::h.into_glib(), ctrl, flags, {
        let t = tabs.clone();
        move |_| { t.with_current(|wv| wv.load_uri("nyx://history")); true }
    });

    // Ctrl + / − / 0 → zoom (Sprint 4.5) ; '=' = '+' sans Shift
    for k in [key::plus, key::equal] {
        bind(&accel, k.into_glib(), ctrl, flags, {
            let t = tabs.clone(); move |_| { zoom_by(&t, 0.1); true }
        });
    }
    bind(&accel, key::minus.into_glib(), ctrl, flags, {
        let t = tabs.clone(); move |_| { zoom_by(&t, -0.1); true }
    });
    bind(&accel, key::_0.into_glib(), ctrl, flags, {
        let t = tabs.clone(); move |_| { t.with_current(|wv| wv.set_zoom_level(1.0)); true }
    });
}

fn zoom_by(tabs: &TabBar, delta: f64) {
    tabs.with_current(|wv| {
        wv.set_zoom_level((wv.zoom_level() + delta).clamp(0.3, 3.0));
    });
}

fn bind<F>(accel: &AccelGroup, key: u32, mods: ModifierType, flags: AccelFlags, cb: F)
where
    F: Fn(u32) -> bool + 'static,
{
    accel.connect_accel_group(key, mods, flags, move |_, _, k, _| cb(k));
}
