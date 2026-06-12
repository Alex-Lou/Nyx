use std::rc::Rc;

use gtk::prelude::*;
use gtk::{Box as GtkBox, Button, Image, Label, Notebook, Orientation};
use webkit2gtk::{LoadEvent, WebView, WebViewExt};

use crate::adblock::AdBlocker;
use crate::webview;

#[derive(Clone)]
pub struct TabBar {
    pub notebook: Notebook,
    blocker: Rc<AdBlocker>,
}

impl TabBar {
    pub fn new(blocker: Rc<AdBlocker>) -> Self {
        let notebook = Notebook::builder()
            .scrollable(true)
            .show_border(false)
            .build();
        Self { notebook, blocker }
    }

    /// Ouvre un nouvel onglet et retourne la WebView créée.
    pub fn open(&self, url: &str) -> WebView {
        let webview = WebView::new();
        self.attach(&webview);
        webview.load_uri(url);
        webview
    }

    /// Onglet pour une WebView liée (window.open / target=_blank — Sprint 1.8).
    /// WebKit chargera lui-même le contenu de la vue retournée.
    fn open_related(&self, related: &WebView) -> WebView {
        let webview = WebView::with_related_view(related);
        self.attach(&webview);
        webview
    }

    /// Câblage commun : config privacy/adblock, label d'onglet, signal create.
    fn attach(&self, webview: &WebView) {
        webview.set_vexpand(true);
        webview.set_hexpand(true);

        let tab_label = build_tab_label("Nouveau", webview, &self.notebook);
        webview::configure(webview, self.blocker.clone(), tab_label.on_block);

        // Liens ouvrant une nouvelle fenêtre → nouvel onglet
        let tabs = self.clone();
        webview.connect_create(move |wv, _| {
            let new = tabs.open_related(wv);
            Some(new.upcast())
        });

        let page_idx = self.notebook.append_page(webview, Some(&tab_label.widget));
        self.notebook.set_current_page(Some(page_idx));
        webview.show();
    }

    /// Ferme l'onglet actif.
    pub fn close_current(&self) {
        if let Some(page) = self.notebook.current_page() {
            self.notebook.remove_page(Some(page));
        }
    }

    /// WebView de l'onglet actif, si disponible.
    pub fn active_webview(&self) -> Option<WebView> {
        current_webview(&self.notebook)
    }
}

/// WebView de l'onglet actif d'un notebook, si disponible.
pub fn current_webview(nb: &Notebook) -> Option<WebView> {
    let page = nb.current_page()?;
    nb.nth_page(Some(page))?.downcast::<WebView>().ok()
}

struct TabLabel {
    widget: GtkBox,
    /// Incrémente le badge de pubs bloquées (Sprint 3.5).
    on_block: Box<dyn Fn()>,
}

fn build_tab_label(title: &str, webview: &WebView, notebook: &Notebook) -> TabLabel {
    let favicon = Image::new();

    let label = Label::new(Some(title));
    label.set_max_width_chars(20);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let badge = Label::new(None);
    badge.style_context().add_class("nyx-tab-badge");
    badge.set_no_show_all(true);

    let close_btn = Button::with_label("×");
    close_btn.style_context().add_class("nyx-tab-close");
    close_btn.set_relief(gtk::ReliefStyle::None);

    let tab_box = GtkBox::new(Orientation::Horizontal, 6);
    tab_box.pack_start(&favicon, false, false, 0);
    tab_box.pack_start(&label, true, true, 0);
    tab_box.pack_start(&badge, false, false, 0);
    tab_box.pack_start(&close_btn, false, false, 0);
    tab_box.show_all();

    // Mettre à jour le titre quand la page change
    let lbl = label.clone();
    webview.connect_title_notify(move |wv| {
        let title = wv.title().unwrap_or_else(|| "…".into());
        lbl.set_text(&title);
        lbl.set_tooltip_text(Some(&title));
    });

    // Favicon (Sprint 1.4)
    let fav = favicon.clone();
    webview.connect_favicon_notify(move |wv| {
        match favicon_pixbuf(wv) {
            Some(pix) => fav.set_from_pixbuf(Some(&pix)),
            None => fav.clear(),
        }
    });

    // Nouveau chargement → compteur de blocages remis à zéro
    let bdg = badge.clone();
    webview.connect_load_changed(move |_, event| {
        if event == LoadEvent::Started {
            bdg.set_text("");
            bdg.hide();
        }
    });

    let bdg = badge.clone();
    let on_block = Box::new(move || {
        let n: u32 = bdg.text().parse().unwrap_or(0);
        bdg.set_text(&(n + 1).to_string());
        bdg.show();
    });

    // Fermer l'onglet
    let nb = notebook.clone();
    let wv = webview.clone();
    close_btn.connect_clicked(move |_| {
        if let Some(idx) = nb.page_num(&wv) {
            nb.remove_page(Some(idx));
        }
    });

    TabLabel { widget: tab_box, on_block }
}

/// Favicon de la WebView redimensionnée en 16×16 pour l'onglet.
fn favicon_pixbuf(wv: &WebView) -> Option<gtk::gdk_pixbuf::Pixbuf> {
    let surface = wv.favicon()?;
    let img = gtk::cairo::ImageSurface::try_from(surface).ok()?;
    let pix = gtk::gdk::pixbuf_get_from_surface(&img, 0, 0, img.width(), img.height())?;
    pix.scale_simple(16, 16, gtk::gdk_pixbuf::InterpType::Bilinear)
}
