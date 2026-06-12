//! Handle GTK partagé du `DownloadStore` — wrapper `Rc<RefCell<…>>`.
//!
//! Toute l'UI consomme `DownloadsHandle`. Le futur bridge WebKit muerra
//! le store via le même handle (single-threaded GTK = `Rc<RefCell<…>>`,
//! pas de `Mutex`).

use std::cell::RefCell;
use std::rc::Rc;

use nyx_core::downloads::DownloadStore;

pub type DownloadsHandle = Rc<RefCell<DownloadStore>>;

pub fn new() -> DownloadsHandle {
    Rc::new(RefCell::new(DownloadStore::new()))
}
