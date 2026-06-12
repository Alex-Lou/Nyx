use std::sync::Arc;

use gtk::prelude::*;
use gtk::{Box as GtkBox, Button, Label, Notebook, Orientation};
use webkit2gtk::{WebView, WebViewExt};

use crate::adblock::AdBlocker;
use crate::webview;

#[derive(Clone)]
pub struct TabBar {
    pub notebook: Notebook,
    blocker: Arc<AdBlocker>,
}

impl TabBar {
    pub fn new(blocker: Arc<AdBlocker>) -> Self {
        let notebook = Notebook::builder()
            .scrollable(true)
            .show_border(false)
            .build();
        Self { notebook, blocker }
    }

    /// Ouvre un nouvel onglet et retourne la WebView créée.
    pub fn open(&self, url: &str) -> WebView {
        let webview = WebView::new();
        webview.set_vexpand(true);
        webview.set_hexpand(true);
        webview::configure(&webview, self.blocker.clone());

        let tab_label = build_tab_label("Nouveau", &webview, &self.notebook);

        let page_idx = self.notebook.append_page(&webview, Some(&tab_label));
        webview.load_uri(url);
        self.notebook.set_current_page(Some(page_idx));
        webview.show();

        webview
    }

    /// Ferme l'onglet actif.
    pub fn close_current(&self) {
        if let Some(page) = self.notebook.current_page() {
            self.notebook.remove_page(Some(page));
        }
    }

    /// WebView de l'onglet actif, si disponible.
    /// Utilisé par le Sprint 2 (historique, bookmarks depuis l'URL bar).
    #[allow(dead_code)]
    pub fn active_webview(&self) -> Option<WebView> {
        current_webview(&self.notebook)
    }
}

/// WebView de l'onglet actif d'un notebook, si disponible.
pub fn current_webview(nb: &Notebook) -> Option<WebView> {
    let page = nb.current_page()?;
    nb.nth_page(Some(page))?.downcast::<WebView>().ok()
}

fn build_tab_label(title: &str, webview: &WebView, notebook: &Notebook) -> GtkBox {
    let label = Label::new(Some(title));
    label.set_max_width_chars(20);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let close_btn = Button::with_label("×");
    close_btn.style_context().add_class("nyx-tab-close");
    close_btn.set_relief(gtk::ReliefStyle::None);

    let tab_box = GtkBox::new(Orientation::Horizontal, 6);
    tab_box.pack_start(&label, true, true, 0);
    tab_box.pack_start(&close_btn, false, false, 0);
    tab_box.show_all();

    // Mettre à jour le titre quand la page change
    let lbl = label.clone();
    webview.connect_title_notify(move |wv| {
        let title = wv.title().unwrap_or_else(|| "…".into());
        lbl.set_text(&title);
        lbl.set_tooltip_text(Some(&title));
    });

    // Fermer l'onglet
    let nb = notebook.clone();
    let wv = webview.clone();
    close_btn.connect_clicked(move |_| {
        if let Some(idx) = nb.page_num(&wv) {
            nb.remove_page(Some(idx));
        }
    });

    tab_box
}
