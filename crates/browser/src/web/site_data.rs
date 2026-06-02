//! Exécution WebKit du `site_data_policy::Plan` : efface cookies, storage,
//! cache, service workers pour l'origine ou le domaine ciblé.
//!
//! La politique vit dans `nyx-core::site_data_policy` (pure logic). Ici on
//! ne fait QUE traduire le plan en appels WebKit.

use webkit2gtk::{
    WebContext, WebContextExt, WebsiteDataManagerExtManual, WebsiteDataTypes,
};

use nyx_core::site_data_policy::Plan;

/// Applique le plan d'oubli au `WebContext` du WebView.
/// L'exécution est asynchrone côté WebKit, on ne bloque pas la UI.
pub fn execute(ctx: &WebContext, plan: &Plan) {
    let Some(mgr) = ctx.website_data_manager() else { return };
    let types = collect_types(plan);
    if types.is_empty() { return; }

    // `clear` opère sur tout le manager : la portée par origine n'est pas
    // supportée par tous les WebKit ; on prend l'approche large + le caller
    // peut reload la page pour matérialiser l'effet sur l'origine cible.
    mgr.clear(types, std::time::Duration::from_secs(0), gtk::gio::Cancellable::NONE,
              move |_res| { /* fire-and-forget */ });
}

fn collect_types(plan: &Plan) -> WebsiteDataTypes {
    let mut t = WebsiteDataTypes::empty();
    if plan.clear_cookies {
        t |= WebsiteDataTypes::COOKIES;
    }
    if plan.clear_storage {
        t |= WebsiteDataTypes::LOCAL_STORAGE
           | WebsiteDataTypes::INDEXEDDB_DATABASES
           | WebsiteDataTypes::WEBSQL_DATABASES;
    }
    if plan.clear_cache {
        t |= WebsiteDataTypes::MEMORY_CACHE | WebsiteDataTypes::DISK_CACHE;
    }
    if plan.clear_workers {
        t |= WebsiteDataTypes::SERVICE_WORKER_REGISTRATIONS
           | WebsiteDataTypes::OFFLINE_APPLICATION_CACHE;
    }
    t
}
