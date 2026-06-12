//! Blocage des sous-ressources pub (img, script, xhr…) — Sprint 3.3.
//!
//! WebKitGTK ne permet pas d'intercepter chaque requête réseau côté UI ;
//! le mécanisme officiel est le content filter (même format que Safari),
//! compilé une fois depuis EasyList/EasyPrivacy puis mis en cache disque.
//!
//! UNIQUE MODULE UNSAFE DU PROJET : les bindings webkit2gtk-rs 2.0
//! n'exposent pas UserContentFilter[Store], on passe par la FFI C.
//! Tout l'unsafe est confiné ici, derrière une API sûre.
#![allow(unsafe_code)]

use std::cell::RefCell;
use std::ffi::CString;
use std::ptr;

use glib::prelude::*;
use glib::translate::ToGlibPtr;
use gtk::gio;
use webkit2gtk::{UserContentManager, UserContentManagerExt};
use webkit2gtk_sys as wk;

const FILTER_ID: &[u8] = b"nyx-ads\0";

/// Handle refcounté sur un WebKitUserContentFilter (boxed type C).
struct Filter(*mut wk::WebKitUserContentFilter);

impl Clone for Filter {
    fn clone(&self) -> Self {
        unsafe { Filter(wk::webkit_user_content_filter_ref(self.0)) }
    }
}

impl Drop for Filter {
    fn drop(&mut self) {
        unsafe { wk::webkit_user_content_filter_unref(self.0) }
    }
}

thread_local! {
    static FILTER: RefCell<Option<Filter>> = const { RefCell::new(None) };
    /// UCM en attente du filtre (créés avant la fin de la compilation).
    static PENDING: RefCell<Vec<UserContentManager>> = const { RefCell::new(Vec::new()) };
    /// UCM équipés (weak : un onglet fermé disparaît tout seul) — permet au
    /// toggle des réglages de couper/réactiver le filtre partout.
    static APPLIED: RefCell<Vec<glib::WeakRef<UserContentManager>>> =
        const { RefCell::new(Vec::new()) };
    static ENABLED: RefCell<bool> = const { RefCell::new(true) };
}

/// Charge le content filter depuis le cache, ou le compile au premier
/// lancement. Asynchrone : les WebViews créées avant la fin sont mises
/// en attente puis équipées dès que le filtre est prêt.
pub fn init() {
    let dir = glib::user_cache_dir().join("nyx").join("filters");
    let _ = std::fs::create_dir_all(&dir);
    let Ok(c_dir) = CString::new(dir.to_string_lossy().as_bytes()) else { return };

    unsafe {
        let store = wk::webkit_user_content_filter_store_new(c_dir.as_ptr());
        wk::webkit_user_content_filter_store_load(
            store,
            FILTER_ID.as_ptr() as *const _,
            ptr::null_mut(),
            Some(on_load_ready),
            ptr::null_mut(),
        );
        // L'opération async tient sa propre référence sur le store.
        glib::gobject_ffi::g_object_unref(store as *mut _);
    }
}

/// Active le blocage de sous-ressources sur cette WebView.
pub fn apply_to(ucm: &UserContentManager) {
    APPLIED.with(|a| {
        let mut applied = a.borrow_mut();
        // Purge les onglets fermés (WeakRef morts) à chaque ajout → la liste
        // reste bornée au nombre d'onglets vivants, sans attendre un toggle.
        applied.retain(|w| w.upgrade().is_some());
        if !applied.iter().any(|w| w.upgrade().as_ref() == Some(ucm)) {
            applied.push(ucm.downgrade());
        }
    });
    if !ENABLED.with(|e| *e.borrow()) {
        return;
    }
    FILTER.with(|f| match f.borrow().as_ref() {
        Some(filter) => add(ucm, filter),
        None => PENDING.with(|p| {
            let mut pending = p.borrow_mut();
            if !pending.contains(ucm) {
                pending.push(ucm.clone());
            }
        }),
    });
}

/// Suit le toggle « bloqueur de pubs » des réglages : coupe ou réactive
/// le filtre sur tous les onglets, immédiatement.
pub fn set_enabled(enabled: bool) {
    let changed = ENABLED.with(|e| {
        let mut cur = e.borrow_mut();
        let changed = *cur != enabled;
        *cur = enabled;
        changed
    });
    if !changed {
        return;
    }
    APPLIED.with(|a| {
        let mut applied = a.borrow_mut();
        applied.retain(|weak| {
            let Some(ucm) = weak.upgrade() else { return false };
            if enabled {
                FILTER.with(|f| {
                    if let Some(filter) = f.borrow().as_ref() {
                        add(&ucm, filter);
                    }
                });
            } else {
                ucm.remove_all_filters();
            }
            true
        });
    });
}

fn ready(filter: Filter) {
    FILTER.with(|f: &RefCell<Option<Filter>>| *f.borrow_mut() = Some(filter.clone()));
    if !ENABLED.with(|e| *e.borrow()) {
        PENDING.with(|p: &RefCell<Vec<UserContentManager>>| p.borrow_mut().clear());
        return;
    }
    PENDING.with(|pending: &RefCell<Vec<UserContentManager>>| {
        for ucm in pending.borrow_mut().drain(..) {
            add(&ucm, &filter);
        }
    });
}

fn add(ucm: &UserContentManager, filter: &Filter) {
    unsafe {
        let ucm_ptr = ucm.to_glib_none().0;
        wk::webkit_user_content_manager_remove_all_filters(ucm_ptr);
        wk::webkit_user_content_manager_add_filter(ucm_ptr, filter.0);
    }
}

unsafe extern "C" fn on_load_ready(
    source: *mut glib::gobject_ffi::GObject,
    result: *mut gio::ffi::GAsyncResult,
    _: glib::ffi::gpointer,
) {
    let store = source as *mut wk::WebKitUserContentFilterStore;
    let mut err = ptr::null_mut();
    let filter =
        wk::webkit_user_content_filter_store_load_finish(store, result, &mut err);
    if !filter.is_null() {
        ready(Filter(filter));
        return;
    }
    if !err.is_null() {
        glib::ffi::g_error_free(err);
    }

    // Premier lancement : compiler le JSON depuis les listes bundlées.
    eprintln!("nyx: compilation des filtres pub (une seule fois)…");
    let json = crate::web::nyxguard::content_blocking_json();
    let bytes = glib::Bytes::from_owned(json.into_bytes());
    wk::webkit_user_content_filter_store_save(
        store,
        FILTER_ID.as_ptr() as *const _,
        bytes.to_glib_none().0,
        ptr::null_mut(),
        Some(on_save_ready),
        ptr::null_mut(),
    );
}

unsafe extern "C" fn on_save_ready(
    source: *mut glib::gobject_ffi::GObject,
    result: *mut gio::ffi::GAsyncResult,
    _: glib::ffi::gpointer,
) {
    let store = source as *mut wk::WebKitUserContentFilterStore;
    let mut err = ptr::null_mut();
    let filter =
        wk::webkit_user_content_filter_store_save_finish(store, result, &mut err);
    if filter.is_null() {
        let reason = if err.is_null() {
            String::from("raison inconnue")
        } else {
            let msg = std::ffi::CStr::from_ptr((*err).message)
                .to_string_lossy()
                .into_owned();
            glib::ffi::g_error_free(err);
            msg
        };
        eprintln!("nyx: compilation des filtres pub échouée ({reason}) — blocage navigation seul");
        return;
    }
    eprintln!("nyx: filtres pub prêts");
    ready(Filter(filter));
}
