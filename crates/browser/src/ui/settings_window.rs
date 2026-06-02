use std::sync::Arc;

use gtk::prelude::*;
use gtk::{Window, WindowPosition};
use webkit2gtk::{WebView, WebViewExt};

use crate::pages::{self, settings as settings_page};
use crate::state::bookmarks::Bookmarks;
use crate::state::settings::Settings;
use crate::ui::chrome;
use crate::web::{self, nyxguard::NyxGuard};

/// Fenêtre Paramètres : flottante, redimensionnable et déplaçable (gérée par
/// le WM), non-bloquante pour continuer à naviguer. Héberge la page settings
/// dans sa propre WebView — l'auto-save `nyx://apply` y fonctionne comme dans
/// un onglet (même filtre de politique).
pub fn build(parent: Option<&Window>, blocker: Arc<NyxGuard>, prefs: Settings, bm: Bookmarks) -> Window {
    let win = Window::builder()
        .title("Paramètres — Nyx")
        .default_width(680)
        .default_height(740)
        .resizable(true)
        .build();
    win.style_context().add_class("nyx-modal");
    chrome::apply_titlebar(&win, "Paramètres — Nyx");
    win.set_modal(false);
    if let Some(p) = parent {
        win.set_transient_for(Some(p));
        win.set_position(WindowPosition::CenterOnParent);
    } else {
        win.set_position(WindowPosition::Center);
    }

    let wv = WebView::new();
    web::configure(&wv, blocker, prefs.clone(), bm);
    wv.load_html(&settings_page::html(&prefs.borrow()), Some(&pages::assets_base_uri()));
    win.add(&wv);

    win
}
