use gtk::gdk::keys::constants as key;
use gtk::gdk::ModifierType;
use gtk::glib::translate::IntoGlib;
use gtk::prelude::*;
use gtk::{AccelFlags, AccelGroup, ApplicationWindow, Entry};
use webkit2gtk::WebViewExt;

use crate::settings::Settings;
use crate::tabs::TabBar;

pub fn wire(window: &ApplicationWindow, tabs: &TabBar, url_bar: &Entry, settings: Settings) {
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
    // Ctrl+, → paramètres
    bind(&accel, key::comma.into_glib(), ctrl, flags, {
        let t = tabs.clone(); move |_| { t.open_settings(); true }
    });
    // Ctrl+D → ajouter favori (géré dans window.rs via le bouton ☆,
    // ici on délègue en simulant le même comportement)
    bind(&accel, key::d.into_glib(), ctrl, flags, {
        let t = tabs.clone();
        let s = settings.clone();
        move |_| {
            if let Some(wv) = t.current_webview() {
                let url = wv.uri().map(|u| u.to_string()).unwrap_or_default();
                // Sprint 2 : accès direct aux bookmarks ici via Arc partagé
                let _ = (url, s.borrow().language.id());
            }
            true
        }
    });
}

fn bind<F>(accel: &AccelGroup, key: u32, mods: ModifierType, flags: AccelFlags, cb: F)
where F: Fn(u32) -> bool + 'static {
    accel.connect_accel_group(key, mods, flags, move |_, _, k, _| cb(k));
}
